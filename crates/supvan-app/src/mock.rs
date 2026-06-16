//! Realistic mock printer simulator for the `SUPVAN_MOCK=1` dev workflow.
//!
//! Controlled by env vars, parsed once on first access:
//!
//! | Var | Effect |
//! |---|---|
//! | `SUPVAN_MOCK_DELAY_MS` | sleep this long per `transfer_page` |
//! | `SUPVAN_MOCK_FAIL` | comma-separated reason tokens; fail the *next* print |
//! | `SUPVAN_MOCK_FAIL_REPEAT=1` | re-arm the single-shot fail after consumption |
//! | `SUPVAN_MOCK_STICKY` | comma-separated reason tokens for `printer-state-reasons` |
//! | `SUPVAN_MOCK_RECOVER_AFTER_MS` | sticky reasons auto-clear after N ms from server start |
//!
//! Tokens (parser is case-insensitive on the hyphenated form):
//! `media-empty`, `label-not-installed`, `media-jam`, `label-rw-error`,
//! `label-mode-error`, `ribbon-rw-error`, `ribbon-end`, `media-needed`,
//! `cover-open`, `head-temp-high`, `other`, `offline` (alias `offline-report`).

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use ipp_printer_app::{JobFailure, PrinterReason};
use supvan_proto::status::PrinterStatus;

use crate::job::failure_from_status;

#[derive(Clone)]
struct ParsedStatus {
    reasons: PrinterReason,
    status: PrinterStatus,
}

impl Default for ParsedStatus {
    fn default() -> Self {
        Self {
            reasons: PrinterReason::empty(),
            status: PrinterStatus::default(),
        }
    }
}

pub struct MockController {
    delay: Duration,
    fail_repeat: bool,
    fail_template: Option<ParsedStatus>,
    next_fail: Mutex<Option<ParsedStatus>>,
    sticky_reasons: PrinterReason,
    sticky_until: Option<Instant>,
}

pub fn controller() -> &'static MockController {
    static C: OnceLock<MockController> = OnceLock::new();
    C.get_or_init(MockController::from_env)
}

impl MockController {
    fn from_env() -> Self {
        Self::new(
            std::env::var("SUPVAN_MOCK_DELAY_MS").ok().as_deref(),
            std::env::var("SUPVAN_MOCK_FAIL_REPEAT").ok().as_deref(),
            std::env::var("SUPVAN_MOCK_FAIL").ok().as_deref(),
            std::env::var("SUPVAN_MOCK_STICKY").ok().as_deref(),
            std::env::var("SUPVAN_MOCK_RECOVER_AFTER_MS").ok().as_deref(),
            Instant::now(),
        )
    }

    fn new(
        delay_ms: Option<&str>,
        fail_repeat: Option<&str>,
        fail_tokens: Option<&str>,
        sticky_tokens: Option<&str>,
        recover_ms: Option<&str>,
        started_at: Instant,
    ) -> Self {
        let delay = delay_ms
            .and_then(|s| s.parse::<u64>().ok())
            .map(Duration::from_millis)
            .unwrap_or(Duration::ZERO);
        let fail_repeat = fail_repeat == Some("1");
        let fail_template = fail_tokens
            .filter(|s| !s.is_empty())
            .map(parse_status);
        let sticky = sticky_tokens
            .filter(|s| !s.is_empty())
            .map(parse_status)
            .unwrap_or_default();
        let sticky_until = recover_ms
            .and_then(|s| s.parse::<u64>().ok())
            .map(|ms| started_at + Duration::from_millis(ms));

        Self {
            delay,
            fail_repeat,
            next_fail: Mutex::new(fail_template.clone()),
            fail_template,
            sticky_reasons: sticky.reasons,
            sticky_until,
        }
    }

    pub fn delay(&self) -> Duration {
        self.delay
    }

    /// Consume the queued single-shot failure (if any). If
    /// `SUPVAN_MOCK_FAIL_REPEAT=1`, refill from the template so the next call
    /// will fail too.
    pub fn take_print_failure(&self) -> Option<JobFailure> {
        let mut g = self.next_fail.lock().ok()?;
        let parsed = g.take()?;
        if self.fail_repeat {
            *g = self.fail_template.clone();
        }
        Some(failure_from_status(&parsed.status, "mock"))
    }

    /// Sticky `printer-state-reasons`, surfaced by `KsDevice::status`.
    /// Returns `empty()` once `SUPVAN_MOCK_RECOVER_AFTER_MS` has elapsed.
    pub fn current_reasons(&self) -> PrinterReason {
        if let Some(deadline) = self.sticky_until {
            if Instant::now() >= deadline {
                return PrinterReason::empty();
            }
        }
        self.sticky_reasons
    }

}

fn parse_status(s: &str) -> ParsedStatus {
    let mut p = ParsedStatus::default();
    for tok in s.split(',').map(str::trim).filter(|t| !t.is_empty()) {
        match tok.to_ascii_lowercase().as_str() {
            "media-empty" | "label-end" => {
                p.status.label_end = true;
                p.reasons |= PrinterReason::MEDIA_EMPTY;
            }
            "label-not-installed" => {
                p.status.label_not_installed = true;
                p.reasons |= PrinterReason::MEDIA_EMPTY;
            }
            "media-jam" | "label-rw-error" => {
                p.status.label_rw_error = true;
                p.reasons |= PrinterReason::MEDIA_JAM;
            }
            "label-mode-error" => {
                p.status.label_mode_error = true;
                p.reasons |= PrinterReason::MEDIA_JAM;
            }
            "ribbon-rw-error" => {
                p.status.ribbon_rw_error = true;
                p.reasons |= PrinterReason::MEDIA_JAM;
            }
            "ribbon-end" | "media-needed" => {
                p.status.ribbon_end = true;
                p.reasons |= PrinterReason::MEDIA_NEEDED;
            }
            "cover-open" => {
                p.status.cover_open = true;
                p.reasons |= PrinterReason::COVER_OPEN;
            }
            "head-temp-high" | "other" => {
                p.status.head_temp_high = true;
                p.reasons |= PrinterReason::OTHER;
            }
            "offline" | "offline-report" => {
                p.reasons |= PrinterReason::OFFLINE;
            }
            unknown => {
                log::warn!("mock: ignoring unknown reason token '{unknown}'");
            }
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctrl(
        fail: Option<&str>,
        repeat: Option<&str>,
        sticky: Option<&str>,
        recover_ms: Option<&str>,
    ) -> MockController {
        MockController::new(None, repeat, fail, sticky, recover_ms, Instant::now())
    }

    #[test]
    fn parses_multi_token_status() {
        let p = parse_status("media-empty,cover-open");
        assert!(p.reasons.contains(PrinterReason::MEDIA_EMPTY));
        assert!(p.reasons.contains(PrinterReason::COVER_OPEN));
        assert!(p.status.label_end);
        assert!(p.status.cover_open);
    }

    #[test]
    fn ignores_unknown_token() {
        let p = parse_status("media-empty,banana,cover-open");
        assert!(p.reasons.contains(PrinterReason::MEDIA_EMPTY));
        assert!(p.reasons.contains(PrinterReason::COVER_OPEN));
    }

    #[test]
    fn single_shot_fail_consumed_once() {
        let c = ctrl(Some("media-empty"), None, None, None);
        let first = c.take_print_failure();
        assert!(first.is_some());
        assert!(first
            .unwrap()
            .printer_reasons
            .contains(PrinterReason::MEDIA_EMPTY));
        assert!(c.take_print_failure().is_none());
    }

    #[test]
    fn fail_repeat_rearms() {
        let c = ctrl(Some("ribbon-rw-error"), Some("1"), None, None);
        for _ in 0..3 {
            let f = c.take_print_failure().expect("rearmed");
            assert!(f.printer_reasons.contains(PrinterReason::MEDIA_JAM));
        }
    }

    #[test]
    fn sticky_reasons_surface_then_clear() {
        // 1ms recovery deadline so the second read is past the deadline.
        let c = ctrl(None, None, Some("cover-open"), Some("1"));
        assert!(c.current_reasons().contains(PrinterReason::COVER_OPEN));
        std::thread::sleep(Duration::from_millis(5));
        assert!(c.current_reasons().is_empty());
    }

    #[test]
    fn sticky_with_no_recovery_persists() {
        let c = ctrl(None, None, Some("media-empty"), None);
        assert!(c.current_reasons().contains(PrinterReason::MEDIA_EMPTY));
        std::thread::sleep(Duration::from_millis(5));
        assert!(c.current_reasons().contains(PrinterReason::MEDIA_EMPTY));
    }

    #[test]
    fn no_fail_when_unset() {
        let c = ctrl(None, None, None, None);
        assert!(c.take_print_failure().is_none());
        assert!(c.current_reasons().is_empty());
    }
}
