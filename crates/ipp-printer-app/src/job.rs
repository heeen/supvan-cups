//! Per-server job registry: allocates job-ids, tracks state for
//! `Get-Jobs` / `Get-Job-Attributes` / `Cancel-Job`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::RwLock;

use crate::flags::PrinterReason;

pub type JobId = u32;

/// IPP `job-state` enum (RFC 8011 §5.3.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum JobState {
    Pending = 3,
    Held = 4,
    Processing = 5,
    ProcessingStopped = 6,
    Canceled = 7,
    Aborted = 8,
    Completed = 9,
}

impl JobState {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Canceled | Self::Aborted | Self::Completed)
    }
}

/// One job in the per-server registry.
#[derive(Debug, Clone)]
pub struct JobRecord {
    pub id: JobId,
    pub printer_name: String,
    pub state: JobState,
    pub reasons: PrinterReason,
    pub message: String,
    pub created_at: SystemTime,
    pub completed_at: Option<SystemTime>,
    /// Flipped by `Cancel-Job` so the worker can short-circuit.
    pub cancel_flag: Arc<AtomicBool>,
}

impl JobRecord {
    /// Seconds since epoch for `time-at-creation` / `time-at-completed`.
    pub fn created_secs(&self) -> i32 {
        secs_since_epoch(self.created_at)
    }

    pub fn completed_secs(&self) -> Option<i32> {
        self.completed_at.map(secs_since_epoch)
    }

    pub fn is_canceled(&self) -> bool {
        self.cancel_flag.load(Ordering::Acquire)
    }
}

fn secs_since_epoch(t: SystemTime) -> i32 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i32)
        .unwrap_or(0)
}

/// Shared job registry. Cheap to clone.
#[derive(Clone)]
pub struct JobRegistry {
    inner: Arc<RwLock<Inner>>,
}

struct Inner {
    next_id: u32,
    jobs: Vec<JobRecord>,
}

impl Default for JobRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl JobRegistry {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(Inner {
                next_id: 1,
                jobs: Vec::new(),
            })),
        }
    }

    /// Allocate a new pending job for `printer_name`. Returns a clone of the
    /// record so the caller can stash the `JobId` and `cancel_flag` without
    /// holding the registry lock.
    pub fn create(&self, printer_name: String) -> JobRecord {
        let mut g = self.inner.write();
        let id = g.next_id;
        g.next_id = g.next_id.wrapping_add(1).max(1);
        let rec = JobRecord {
            id,
            printer_name,
            state: JobState::Pending,
            reasons: PrinterReason::empty(),
            message: String::new(),
            created_at: SystemTime::now(),
            completed_at: None,
            cancel_flag: Arc::new(AtomicBool::new(false)),
        };
        g.jobs.push(rec.clone());
        rec
    }

    pub fn get(&self, id: JobId) -> Option<JobRecord> {
        self.inner.read().jobs.iter().find(|j| j.id == id).cloned()
    }

    pub fn jobs_for_printer(&self, printer_name: &str) -> Vec<JobRecord> {
        self.inner
            .read()
            .jobs
            .iter()
            .filter(|j| j.printer_name == printer_name)
            .cloned()
            .collect()
    }

    pub fn set_state(&self, id: JobId, state: JobState) {
        let mut g = self.inner.write();
        if let Some(j) = g.jobs.iter_mut().find(|j| j.id == id) {
            j.state = state;
            if state.is_terminal() && j.completed_at.is_none() {
                j.completed_at = Some(SystemTime::now());
            }
        }
    }

    /// Mark a job as failed with IPP reasons + message.
    pub fn set_failure(&self, id: JobId, reasons: PrinterReason, message: String) {
        let mut g = self.inner.write();
        if let Some(j) = g.jobs.iter_mut().find(|j| j.id == id) {
            j.state = JobState::Aborted;
            j.reasons = reasons;
            j.message = message;
            j.completed_at = Some(SystemTime::now());
        }
    }

    /// Request cancellation. Returns the new state, or `None` if no such job.
    /// Already-terminal jobs are left alone.
    pub fn cancel(&self, id: JobId) -> Option<JobState> {
        let mut g = self.inner.write();
        let j = g.jobs.iter_mut().find(|j| j.id == id)?;
        if j.state.is_terminal() {
            return Some(j.state);
        }
        j.cancel_flag.store(true, Ordering::Release);
        j.state = JobState::Canceled;
        j.completed_at = Some(SystemTime::now());
        Some(j.state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinct_ids() {
        let reg = JobRegistry::new();
        let a = reg.create("p".into());
        let b = reg.create("p".into());
        assert_ne!(a.id, b.id);
        assert_eq!(a.state, JobState::Pending);
    }

    #[test]
    fn cancel_flips_flag_and_state() {
        let reg = JobRegistry::new();
        let j = reg.create("p".into());
        let flag = j.cancel_flag.clone();
        assert!(!flag.load(Ordering::Acquire));
        assert_eq!(reg.cancel(j.id), Some(JobState::Canceled));
        assert!(flag.load(Ordering::Acquire));
        assert_eq!(reg.get(j.id).unwrap().state, JobState::Canceled);
    }

    #[test]
    fn cancel_terminal_is_noop() {
        let reg = JobRegistry::new();
        let j = reg.create("p".into());
        reg.set_state(j.id, JobState::Completed);
        assert_eq!(reg.cancel(j.id), Some(JobState::Completed));
        assert!(!reg.get(j.id).unwrap().cancel_flag.load(Ordering::Acquire));
    }

    #[test]
    fn failure_records_reasons_and_message() {
        let reg = JobRegistry::new();
        let j = reg.create("p".into());
        reg.set_failure(j.id, PrinterReason::MEDIA_EMPTY, "no labels".into());
        let after = reg.get(j.id).unwrap();
        assert_eq!(after.state, JobState::Aborted);
        assert_eq!(after.reasons, PrinterReason::MEDIA_EMPTY);
        assert_eq!(after.message, "no labels");
    }

    #[test]
    fn jobs_for_printer_filters() {
        let reg = JobRegistry::new();
        let _ = reg.create("a".into());
        let _ = reg.create("b".into());
        let _ = reg.create("a".into());
        assert_eq!(reg.jobs_for_printer("a").len(), 2);
        assert_eq!(reg.jobs_for_printer("b").len(), 1);
        assert_eq!(reg.jobs_for_printer("c").len(), 0);
    }
}
