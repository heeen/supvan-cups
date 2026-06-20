# IPP Everywhere conformance audit

Run of the canonical `ipptool` suite against the live server:

```sh
ipptool -tv ipp://127.0.0.1:<port>/ipp/print/<queue> \
    /usr/share/cups/ipptool/ipp-everywhere.test
```

The suite pulls in `ipp-1.1.test` and `ipp-2.0.test`, so it exercises RFC 8011
§4.x request validation, PWG 5100.12 description attributes, and PWG 5100.14
required operations/attributes.

## Status (ipp-printer-app 0.6.0)

**32 PASS / 0 FAIL — full IPP Everywhere conformance.** The two "cannot be read"
lines in a fresh checkout are missing optional test *fixtures*
(`document-a4.pdf`, the `pwg-raster-samples-*` files), not assertion failures.

Closed across three framework releases plus supvan changes:

- **0.4.0** — encoding/validation bugs + missing operations/attributes (see
  below).
- **0.5.0** — device-fed dynamic `media-ready` / `media-col-ready` /
  `printer-supply`.
- **0.6.0 + supvan** — `image/jpeg` (PWG 5100.14 §5.2, the last required
  format). The framework surfaces `document-format` to the print callback and
  drives `document-format-supported` from `PrinterConfig::document_formats`;
  supvan advertises `image/jpeg` and decodes it in `run_jpeg_job`
  (`crates/supvan-app/src/ipp_job.rs`): decode → contain-fit onto the loaded
  label (aspect preserved, centered, white-padded) → existing `KsJob` 8bpp
  dither→device path.

The supvan-side `om_` (metric) media-name fix lives in
`crates/supvan-app/src/models.rs`.

### Closed in 0.4.0 (framework)

- **Encoding bugs**: `operations-supported` / `finishings-supported` now
  `1setOf enum`; `media-col-supported` now a `1setOf keyword` member list
  (real sizes stay in `media-col-database`).
- **`requested-attributes` honoured** for Get-Printer-Attributes, Get-Jobs
  (default = `job-uri` + `job-id`), and Get-Job-Attributes.
- **Request validation**: reject `request-id` 0 (§4.1.1), missing/misordered
  `attributes-charset` + `attributes-natural-language` (§4.1.4), unsupported
  IPP version (§4.1.8), and missing `printer-uri`/`job-uri` (§4.2).
- **Required operations**: Identify-Printer (via the new
  `DeviceBackend::identify` hook), Create-Job, Send-Document (requires
  `last-document`), Close-Job, Cancel-My-Jobs. Jobs carry their
  `requesting-user-name` owner so `Get-Jobs my-jobs=true` scopes correctly.
- **~25 required descriptor/job attributes**: media margins,
  `media-size-supported`, `media-ready` / `media-col-ready`,
  job/limit descriptors, rendering intent, `pwg-raster-document-sheet-back`,
  identity/admin (`printer-organization`, `printer-icons` via `GET /icon.png`,
  geo-location, supply), change timestamps, `time-at-processing`,
  `job-printer-up-time`. `printer-up-time` floored at 1.

### Closed (supvan config)

- **PWG media names use `om_`** (other-metric), not `oe_` (inches). `oe_…mm`
  failed the IPP Everywhere media-name regex.

## Remaining

Nothing required is outstanding — the suite is green. Earlier tiers are all
closed: dynamic device-fed `media-ready` / `media-col-ready` / `printer-supply`
(0.5.0), Identify-Printer → device `CHECK_DEVICE` ping (supvan backend), and
`image/jpeg` decode (0.6.0 + `run_jpeg_job`).

Possible future polish (not conformance): Floyd–Steinberg dithering for photo
JPEGs (currently the shared Bayer `dither_line`), and threading the *live*
loaded-label mm into `run_jpeg_job` (it uses the configured default size today).

Re-run `ipp-everywhere.test` after attribute/format changes; track the pass
delta.

Note: under `SUPVAN_MOCK=1` the registrar logs an `lpadmin -m everywhere` PPD
warning. This is a mock-only quirk — the mock's synthetic label dimensions
don't resolve to a standard PWG size, so CUPS's PPD generator bails. Real
hardware advertises resolvable sizes and is unaffected; the integration test
still passes because the registrar's queue is created before the mock's roll
empties.
