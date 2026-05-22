//! Build `Get-Printer-Attributes` / `Validate-Job` IPP responses.

use ipp::attribute::{IppAttribute, IppAttributes};
use ipp::model::DelimiterTag;
use ipp::prelude::*;
use ipp::request::IppRequestResponse;
use ipp::value::IppValue;

use crate::printer::{IppPrinterState, PrinterRecord};

fn kw(s: &str) -> IppValue {
    IppValue::Keyword(s.try_into().expect("keyword"))
}

fn mime(s: &str) -> IppValue {
    IppValue::MimeMediaType(s.try_into().expect("mime"))
}

fn uri(s: &str) -> IppValue {
    IppValue::Uri(s.try_into().expect("uri"))
}

fn charset(s: &str) -> IppValue {
    IppValue::Charset(s.try_into().expect("charset"))
}

fn lang(s: &str) -> IppValue {
    IppValue::NaturalLanguage(s.try_into().expect("language"))
}

fn attr(name: &str, value: IppValue) -> IppAttribute {
    IppAttribute::new(name.try_into().expect("attr name"), value)
}

fn add(attrs: &mut IppAttributes, tag: DelimiterTag, name: &str, value: IppValue) {
    attrs.add(tag, attr(name, value));
}

fn add_array_keyword(attrs: &mut IppAttributes, tag: DelimiterTag, name: &str, items: &[&str]) {
    let values: Vec<IppValue> = items.iter().map(|s| kw(s)).collect();
    add(attrs, tag, name, IppValue::Array(values));
}

/// Advertise localhost when the server bound to an unspecified address.
fn advertise_host(host: &str) -> &str {
    if host == "0.0.0.0" || host == "::" || host.is_empty() {
        "localhost"
    } else {
        host
    }
}

/// Build the advertised printer URI for a record.
fn printer_uri(record: &PrinterRecord, host: &str, port: u16) -> String {
    format!(
        "ipp://{}:{}/ipp/print/{}",
        advertise_host(host),
        port,
        record.config.name
    )
}

/// Build a successful Get-Printer-Attributes response.
pub fn get_printer_attributes(
    version: IppVersion,
    request_id: u32,
    record: &PrinterRecord,
    host: &str,
    port: u16,
) -> Result<IppRequestResponse, ipp::parser::IppParseError> {
    let mut resp =
        IppRequestResponse::new_response(version, StatusCode::SuccessfulOk, request_id)?;
    let attrs = resp.attributes_mut();
    let cfg = &record.config;
    let printer_uri_str = printer_uri(record, host, port);
    let more_info = format!(
        "http://{}:{}/",
        advertise_host(host),
        port
    );

    // --- Required header / identity attributes ---
    let p = DelimiterTag::PrinterAttributes;
    add(attrs, p, "printer-uri-supported", uri(&printer_uri_str));
    add(attrs, p, "uri-authentication-supported", kw("none"));
    add(attrs, p, "uri-security-supported", kw("none"));
    add(
        attrs,
        p,
        "printer-name",
        IppValue::NameWithoutLanguage(cfg.name.as_str().try_into().unwrap()),
    );
    add(
        attrs,
        p,
        "printer-location",
        IppValue::TextWithoutLanguage("".try_into().unwrap()),
    );
    add(
        attrs,
        p,
        "printer-info",
        IppValue::TextWithoutLanguage(cfg.make_and_model.as_str().try_into().unwrap()),
    );
    add(
        attrs,
        p,
        "printer-make-and-model",
        IppValue::TextWithoutLanguage(cfg.make_and_model.as_str().try_into().unwrap()),
    );
    add(attrs, p, "printer-more-info", uri(&more_info));
    add(
        attrs,
        p,
        "printer-uuid",
        uri(&format!("urn:uuid:{}", record.uuid)),
    );
    add(
        attrs,
        p,
        "printer-up-time",
        IppValue::Integer(uptime_secs() as i32),
    );

    // --- State ---
    add(attrs, p, "printer-state", IppValue::Enum(record.state as i32));
    let reason_kws: Vec<&str> = record.reasons.ipp_keywords();
    add_array_keyword(attrs, p, "printer-state-reasons", &reason_kws);
    add(attrs, p, "printer-is-accepting-jobs", IppValue::Boolean(true));
    add(attrs, p, "queued-job-count", IppValue::Integer(0));

    // --- Versions / operations / charset / language ---
    add_array_keyword(attrs, p, "ipp-versions-supported", &["1.1", "2.0", "2.1"]);
    add_array_keyword(
        attrs,
        p,
        "operations-supported",
        &[
            "Print-Job",
            "Validate-Job",
            "Get-Printer-Attributes",
            "Get-Jobs",
            "Get-Job-Attributes",
            "Cancel-Job",
        ],
    );
    add(
        attrs,
        p,
        "charset-configured",
        charset("utf-8"),
    );
    add(
        attrs,
        p,
        "charset-supported",
        IppValue::Array(vec![charset("utf-8")]),
    );
    add(attrs, p, "natural-language-configured", lang("en"));
    add(
        attrs,
        p,
        "natural-language-supported",
        IppValue::Array(vec![lang("en")]),
    );
    add(
        attrs,
        p,
        "generated-natural-language-supported",
        IppValue::Array(vec![lang("en")]),
    );
    add_array_keyword(attrs, p, "compression-supported", &["none"]);

    // --- Document format ---
    // PWG raster is the IPP Everywhere required format; the unified CUPS reader
    // also handles legacy CUPS raster v1/v2 if a client picks that path.
    add(
        attrs,
        p,
        "document-format-supported",
        IppValue::Array(vec![
            mime("image/pwg-raster"),
            mime("application/vnd.cups-raster"),
            mime("application/octet-stream"),
        ]),
    );
    add(
        attrs,
        p,
        "document-format-default",
        mime("image/pwg-raster"),
    );
    // PWG raster type for the everywhere driver.
    add_array_keyword(
        attrs,
        p,
        "pwg-raster-document-type-supported",
        &["black_1"],
    );
    add(
        attrs,
        p,
        "pwg-raster-document-resolution-supported",
        IppValue::Array(vec![IppValue::Resolution {
            cross_feed: cfg.dpi,
            feed: cfg.dpi,
            units: 3,
        }]),
    );
    add_array_keyword(
        attrs,
        p,
        "urf-supported",
        &["W8", "SRGB24", "CP1", "RS203"],
    );

    // --- Color / sides / orientation ---
    add(attrs, p, "color-supported", IppValue::Boolean(false));
    add_array_keyword(attrs, p, "print-color-mode-supported", &["monochrome"]);
    add(attrs, p, "print-color-mode-default", kw("monochrome"));
    add_array_keyword(attrs, p, "sides-supported", &["one-sided"]);
    add(attrs, p, "sides-default", kw("one-sided"));
    add(attrs, p, "orientation-requested-default", IppValue::Enum(3));

    // --- Resolution ---
    add(
        attrs,
        p,
        "printer-resolution-default",
        IppValue::Resolution {
            cross_feed: cfg.dpi,
            feed: cfg.dpi,
            units: 3,
        },
    );
    add(
        attrs,
        p,
        "printer-resolution-supported",
        IppValue::Array(vec![IppValue::Resolution {
            cross_feed: cfg.dpi,
            feed: cfg.dpi,
            units: 3,
        }]),
    );

    // --- Media ---
    let media_kws: Vec<&str> = cfg.media_names.iter().map(|s| s.as_str()).collect();
    if !media_kws.is_empty() {
        add(attrs, p, "media-default", kw(media_kws[0]));
        add_array_keyword(attrs, p, "media-supported", &media_kws);

        // media-col-{default,supported} — required by IPP Everywhere.
        let default_size = cfg.media_sizes.first().copied().unwrap_or([4000, 3000]);
        add(
            attrs,
            p,
            "media-col-default",
            media_col(media_kws[0], default_size),
        );
        let media_cols: Vec<IppValue> = media_kws
            .iter()
            .zip(cfg.media_sizes.iter().copied().chain(std::iter::repeat(default_size)))
            .map(|(name, size)| media_col(name, size))
            .collect();
        add(attrs, p, "media-col-supported", IppValue::Array(media_cols));
    }

    // --- Job template defaults / supported (live in printer-attributes group) ---
    add(
        attrs,
        p,
        "copies-supported",
        IppValue::RangeOfInteger { min: 1, max: 999 },
    );
    add(attrs, p, "copies-default", IppValue::Integer(1));
    add(
        attrs,
        p,
        "print-quality-supported",
        IppValue::Array(vec![
            IppValue::Enum(3),
            IppValue::Enum(4),
            IppValue::Enum(5),
        ]),
    );
    add(attrs, p, "print-quality-default", IppValue::Enum(4));

    Ok(resp)
}

/// Validate-Job: same capability surface as Get-Printer-Attributes (success).
pub fn validate_job(
    version: IppVersion,
    request_id: u32,
    record: &PrinterRecord,
    host: &str,
    port: u16,
) -> Result<IppRequestResponse, ipp::parser::IppParseError> {
    get_printer_attributes(version, request_id, record, host, port)
}

/// Build the `Print-Job` accepted response for a freshly-allocated job.
pub fn print_job_accepted(
    version: IppVersion,
    request_id: u32,
    job: &crate::job::JobRecord,
    printer_uri_str: &str,
) -> Result<IppRequestResponse, ipp::parser::IppParseError> {
    let mut resp =
        IppRequestResponse::new_response(version, StatusCode::SuccessfulOk, request_id)?;
    let job_uri_str = format!("{printer_uri_str}/job/{}", job.id);
    let j = DelimiterTag::JobAttributes;
    add(resp.attributes_mut(), j, "job-uri", uri(&job_uri_str));
    add(
        resp.attributes_mut(),
        j,
        "job-id",
        IppValue::Integer(job.id as i32),
    );
    add(
        resp.attributes_mut(),
        j,
        "job-state",
        IppValue::Enum(job.state as i32),
    );
    add_array_keyword(
        resp.attributes_mut(),
        j,
        "job-state-reasons",
        &job_state_reason_keywords(job),
    );
    Ok(resp)
}

/// Build a `Get-Job-Attributes` response for a single job.
pub fn build_job_attrs_response(
    version: IppVersion,
    request_id: u32,
    job: &crate::job::JobRecord,
    printer_uri_str: &str,
) -> Result<IppRequestResponse, ipp::parser::IppParseError> {
    let mut resp =
        IppRequestResponse::new_response(version, StatusCode::SuccessfulOk, request_id)?;
    append_job_attrs(resp.attributes_mut(), job, printer_uri_str);
    Ok(resp)
}

/// Build a `Get-Jobs` response listing one job per group.
pub fn build_get_jobs_response(
    version: IppVersion,
    request_id: u32,
    jobs: &[crate::job::JobRecord],
    printer_uri_str: &str,
) -> Result<IppRequestResponse, ipp::parser::IppParseError> {
    let mut resp =
        IppRequestResponse::new_response(version, StatusCode::SuccessfulOk, request_id)?;
    // Each job goes in its own JobAttributes group. The `ipp` crate's `add`
    // merges all attrs with the same DelimiterTag into one group, which is
    // wrong for multi-job responses — we push raw groups instead.
    for job in jobs {
        let mut group = ipp::attribute::IppAttributeGroup::new(DelimiterTag::JobAttributes);
        for a in job_attrs_for_group(job, printer_uri_str) {
            group
                .attributes_mut()
                .insert(a.name().to_owned(), a);
        }
        resp.attributes_mut().groups_mut().push(group);
    }
    Ok(resp)
}

fn append_job_attrs(
    attrs: &mut IppAttributes,
    job: &crate::job::JobRecord,
    printer_uri_str: &str,
) {
    for a in job_attrs_for_group(job, printer_uri_str) {
        attrs.add(DelimiterTag::JobAttributes, a);
    }
}

fn job_attrs_for_group(
    job: &crate::job::JobRecord,
    printer_uri_str: &str,
) -> Vec<IppAttribute> {
    let job_uri_str = format!("{printer_uri_str}/job/{}", job.id);
    let mut out = vec![
        attr("job-uri", uri(&job_uri_str)),
        attr("job-id", IppValue::Integer(job.id as i32)),
        attr("job-printer-uri", uri(printer_uri_str)),
        attr(
            "job-name",
            IppValue::NameWithoutLanguage(
                format!("job-{}", job.id).as_str().try_into().unwrap(),
            ),
        ),
        attr("job-state", IppValue::Enum(job.state as i32)),
        attr("time-at-creation", IppValue::Integer(job.created_secs())),
    ];
    let reason_kws = job_state_reason_keywords(job);
    out.push(attr(
        "job-state-reasons",
        IppValue::Array(reason_kws.iter().map(|s| kw(s)).collect()),
    ));
    if !job.message.is_empty() {
        out.push(attr(
            "job-state-message",
            IppValue::TextWithoutLanguage(job.message.as_str().try_into().unwrap()),
        ));
    }
    if let Some(s) = job.completed_secs() {
        out.push(attr("time-at-completed", IppValue::Integer(s)));
    }
    out
}

fn job_state_reason_keywords(job: &crate::job::JobRecord) -> Vec<&'static str> {
    use crate::flags::PrinterReason;
    use crate::job::JobState;
    let mut out = Vec::new();
    if job.reasons.contains(PrinterReason::MEDIA_EMPTY) {
        out.push("job-completed-with-errors");
    }
    if job.reasons.contains(PrinterReason::MEDIA_JAM) {
        out.push("aborted-by-system");
    }
    if job.reasons.contains(PrinterReason::OFFLINE) {
        out.push("connection-error");
    }
    match job.state {
        JobState::Canceled => out.push("job-canceled-by-user"),
        JobState::Completed => out.push("job-completed-successfully"),
        JobState::Aborted if out.is_empty() => out.push("aborted-by-system"),
        _ => {}
    }
    if out.is_empty() {
        out.push("none");
    }
    out
}

pub fn set_printer_processing(record: &mut PrinterRecord) {
    record.state = IppPrinterState::Processing;
}

pub fn set_printer_idle(record: &mut PrinterRecord) {
    record.state = IppPrinterState::Idle;
}

/// Build a `media-col` collection with `media-size` (x/y in hundredths of mm)
/// and `media-size-name`. CUPS expects PWG dimensions in hundredths of mm.
fn media_col(name: &str, size_hmm: [i32; 2]) -> IppValue {
    use std::collections::BTreeMap;
    let mut size = BTreeMap::new();
    size.insert(
        "x-dimension".try_into().unwrap(),
        IppValue::Integer(size_hmm[0]),
    );
    size.insert(
        "y-dimension".try_into().unwrap(),
        IppValue::Integer(size_hmm[1]),
    );
    let mut col = BTreeMap::new();
    col.insert(
        "media-size".try_into().unwrap(),
        IppValue::Collection(size),
    );
    col.insert(
        "media-size-name".try_into().unwrap(),
        kw(name),
    );
    IppValue::Collection(col)
}

fn uptime_secs() -> u64 {
    use std::sync::OnceLock;
    use std::time::Instant;
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs()
}
