//! An export as it runs: stopped by its card's Cancel, the canvas waited for no longer than
//! [`LOCK_MOST`], and its stages and their times in the log, so a report names the stage an export
//! that never ended got to.
//!
//! An export takes an id as it begins ([`ExportRun::begin`]). Cancel stops the export under way
//! and one the app has made ready to begin ([`Exports::arm`], as its card shows) but that hasn't
//! begun yet; the next export goes on. Each stage begun checks it, and so do the waits for the
//! canvas, so an export stops at its next layer or frame, or as soon as the canvas is free.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::MobileError;

/// What a cancelled export fails with.
pub(crate) const CANCELLED: &str = "the export was cancelled";

/// The longest an export waits for the canvas while something else holds it (a frame stuck on the
/// GPU, say): then it gives up, saying so, rather than leave its card up for good.
pub(crate) const LOCK_MOST: Duration = Duration::from_secs(30);

/// How long each try for the canvas waits before Cancel and the time are looked at again.
const LOCK_SLICE: Duration = Duration::from_millis(25);

/// A session's exports, for Cancel and for what stage the one under way is at.
pub(crate) struct Exports {
    /// The id of the last export begun (none yet: 0).
    begun: AtomicU64,
    /// The id the next export to begin takes, made ready by the app (see [`Exports::arm`]).
    armed: AtomicU64,
    /// Exports with an id up to this one stop.
    cancelled: AtomicU64,
    /// The stage the export under way is at, for the app's log (empty with none).
    stage: parking_lot::Mutex<String>,
    /// [`LOCK_MOST`], shorter in tests (ms).
    lock_most_ms: AtomicU64,
}

impl Default for Exports {
    fn default() -> Exports {
        Exports {
            begun: AtomicU64::new(0),
            armed: AtomicU64::new(0),
            cancelled: AtomicU64::new(0),
            stage: parking_lot::Mutex::new(String::new()),
            lock_most_ms: AtomicU64::new(u64::try_from(LOCK_MOST.as_millis()).unwrap_or(u64::MAX)),
        }
    }
}

impl Exports {
    /// An export is about to begin (its card shows): a Cancel from now stops it, even one that
    /// comes before it has begun.
    pub(crate) fn arm(&self) {
        let next = self.begun.load(Ordering::SeqCst).saturating_add(1);
        self.armed.fetch_max(next, Ordering::SeqCst);
    }

    /// Stops the export under way, and the one made ready to begin; the next begins as asked.
    pub(crate) fn cancel(&self) {
        let upto = self.begun.load(Ordering::SeqCst).max(self.armed.load(Ordering::SeqCst));
        self.cancelled.fetch_max(upto, Ordering::SeqCst);
    }

    /// The stage the export under way is at (empty with none).
    pub(crate) fn stage(&self) -> String {
        self.stage.lock().clone()
    }

    fn begin(&self) -> u64 {
        self.begun.fetch_add(1, Ordering::SeqCst).saturating_add(1)
    }

    fn is_cancelled(&self, id: u64) -> bool {
        id <= self.cancelled.load(Ordering::SeqCst)
    }

    fn lock_most(&self) -> Duration {
        Duration::from_millis(self.lock_most_ms.load(Ordering::Relaxed))
    }

    #[cfg(test)]
    pub(crate) fn set_lock_most(&self, most: Duration) {
        self.lock_most_ms.store(u64::try_from(most.as_millis()).unwrap_or(u64::MAX), Ordering::Relaxed);
    }
}

/// One export under way: its id, what it makes, and its stages so far with the time each took.
pub(crate) struct ExportRun<'a> {
    exports: &'a Exports,
    id: u64,
    what: RefCell<String>,
    started: Instant,
    /// The stages ended, by kind in the order first begun, and the time spent in each kind.
    spent: RefCell<Vec<(&'static str, Duration)>>,
    /// The stage under way: its kind, what of it (which layer...), and since when.
    now: RefCell<(&'static str, String, Instant)>,
    /// Made for no one's card (a package for a plugin's folder): not stopped by Cancel, nor told
    /// as the stage under way, and logged only when it fails.
    quiet: bool,
}

impl<'a> ExportRun<'a> {
    /// An export of `what` (its format and size, for the log) begun now.
    pub(crate) fn begin(exports: &'a Exports, what: impl Into<String>) -> ExportRun<'a> {
        let run = ExportRun {
            exports,
            id: exports.begin(),
            what: RefCell::new(what.into()),
            started: Instant::now(),
            spent: RefCell::new(Vec::new()),
            now: RefCell::new(("beginning", String::new(), Instant::now())),
            quiet: false,
        };
        run.show();
        run
    }

    /// As [`ExportRun::begin`], for an export no one waits on (see [`ExportRun::quiet`]).
    pub(crate) fn begin_quiet(exports: &'a Exports, what: impl Into<String>) -> ExportRun<'a> {
        ExportRun {
            exports,
            id: 0,
            what: RefCell::new(what.into()),
            started: Instant::now(),
            spent: RefCell::new(Vec::new()),
            now: RefCell::new(("beginning", String::new(), Instant::now())),
            quiet: true,
        }
    }

    /// Adds to what is said of the export (its size, once known).
    pub(crate) fn say(&self, more: &str) {
        let mut what = self.what.borrow_mut();
        if !what.is_empty() {
            what.push_str(", ");
        }
        what.push_str(more);
    }

    /// The export goes on to stage `kind` (`detail`: which layer or frame of it), unless it was
    /// cancelled.
    pub(crate) fn stage(&self, kind: &'static str, detail: impl Into<String>) -> Result<(), MobileError> {
        self.check()?;
        let at = Instant::now();
        let (was, _, since) = std::mem::replace(&mut *self.now.borrow_mut(), (kind, detail.into(), at));
        self.spend(was, at.saturating_duration_since(since));
        self.show();
        Ok(())
    }

    /// Fails with [`CANCELLED`] if the export was cancelled.
    pub(crate) fn check(&self) -> Result<(), MobileError> {
        if self.cancelled() { Err(MobileError::File { reason: CANCELLED.to_owned() }) } else { Ok(()) }
    }

    pub(crate) fn cancelled(&self) -> bool {
        !self.quiet && self.exports.is_cancelled(self.id)
    }

    fn spend(&self, kind: &'static str, time: Duration) {
        let mut spent = self.spent.borrow_mut();
        match spent.iter_mut().find(|(k, _)| *k == kind) {
            Some((_, t)) => *t += time,
            None => spent.push((kind, time)),
        }
    }

    fn show(&self) {
        if self.quiet {
            return;
        }
        let (kind, detail, _) = &*self.now.borrow();
        *self.exports.stage.lock() = said_stage(kind, detail);
    }

    /// The export's result, said in the log with every stage's time: made, cancelled (and at
    /// which stage) or failed (where, and why). `made` says what it made (its bytes...).
    pub(crate) fn end<T>(
        self,
        result: Result<T, MobileError>,
        made: impl FnOnce(&T) -> String,
    ) -> Result<T, MobileError> {
        // (a cancel that came through a writer fails it however that writer words it)
        let result = match result {
            Err(_) if self.cancelled() => Err(MobileError::File { reason: CANCELLED.to_owned() }),
            other => other,
        };
        let at = Instant::now();
        let (kind, detail, since) = self.now.borrow().clone();
        self.spend(kind, at.saturating_duration_since(since));
        let total = at.saturating_duration_since(self.started);
        let what = self.what.borrow().clone();
        let spent = self.spent.borrow().clone();
        let stage = said_stage(kind, &detail);
        match &result {
            Ok(value) if !self.quiet => log::info!("{}", ended_line(&what, &Ended::Made(made(value)), total, &spent)),
            Ok(_) => {}
            Err(MobileError::File { reason }) if reason == CANCELLED => {
                log::info!("{}", ended_line(&what, &Ended::Cancelled(stage), total, &spent));
            }
            Err(e) => log::warn!("{}", ended_line(&what, &Ended::Failed(stage, e.to_string()), total, &spent)),
        }
        if !self.quiet {
            self.exports.stage.lock().clear();
        }
        result
    }
}

/// How an export ended, for its line in the log.
pub(crate) enum Ended {
    /// Made, and what it made.
    Made(String),
    /// Cancelled at the stage named.
    Cancelled(String),
    /// Failed at the stage named, and why.
    Failed(String, String),
}

/// A stage as the log and the app name it: its kind, and which of it.
fn said_stage(kind: &str, detail: &str) -> String {
    if detail.is_empty() { kind.to_owned() } else { format!("{kind} ({detail})") }
}

/// An export's line in the log: what it was, how it ended after how long, and the time each kind
/// of stage took, in the order they began.
pub(crate) fn ended_line(what: &str, ended: &Ended, total: Duration, spent: &[(&'static str, Duration)]) -> String {
    let stages: Vec<String> = spent.iter().map(|(kind, t)| format!("{kind} {} ms", t.as_millis())).collect();
    let stages = stages.join(", ");
    let ms = total.as_millis();
    match ended {
        Ended::Made(made) => {
            let made = if made.is_empty() { String::new() } else { format!(", {made}") };
            format!("export {what}: made in {ms} ms ({stages}){made}")
        }
        Ended::Cancelled(at) => format!("export {what}: cancelled at {at} after {ms} ms ({stages})"),
        Ended::Failed(at, why) => format!("export {what}: failed at {at} after {ms} ms ({stages}): {why}"),
    }
}

impl crate::Shared {
    /// The canvas for an export (see [`crate::Shared::lock`]): waited for while it is held, but
    /// no longer than [`LOCK_MOST`] (then it fails, naming what held it), and not once the export
    /// is cancelled.
    #[track_caller]
    pub(crate) fn lock_for_export(
        &self,
        run: &ExportRun<'_>,
    ) -> Result<parking_lot::MutexGuard<'_, crate::State>, MobileError> {
        let at = std::panic::Location::caller();
        let most = self.exports.lock_most();
        let started = Instant::now();
        loop {
            if let Some(guard) = self.try_lock_for_at(LOCK_SLICE, at) {
                return Ok(guard);
            }
            run.check()?;
            if started.elapsed() >= most {
                let held = self.holder_said();
                return Err(crate::project::file_error(
                    "waiting for the canvas",
                    format!("it stayed busy for {:.1} s ({held})", most.as_secs_f32()),
                ));
            }
        }
    }

    /// The canvas for an export once it has finished remaking its caches (see
    /// [`crate::Shared::lock_finished`]), its waits as [`crate::Shared::lock_for_export`]'s.
    #[track_caller]
    pub(crate) fn lock_finished_for_export(
        &self,
        run: &ExportRun<'_>,
    ) -> Result<parking_lot::MutexGuard<'_, crate::State>, MobileError> {
        for _ in 0..1000 {
            self.finish_caches_with(|| self.lock_for_export(run))?;
            let guard = self.lock_for_export(run)?;
            if guard.canvas.as_ref().is_none_or(|c| !c.busy()) {
                return Ok(guard);
            }
        }
        self.lock_for_export(run)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_stops_the_export_under_way_and_not_the_next() {
        let exports = Exports::default();
        let first = ExportRun::begin(&exports, "PNG");
        assert!(first.check().is_ok());
        exports.cancel();
        assert!(first.cancelled());
        assert!(first.stage("reading the picture", "").is_err());
        // no new export made ready (as the desktop does): the next begins as asked
        let second = ExportRun::begin(&exports, "PNG");
        assert!(!second.cancelled());
        assert!(second.stage("reading the picture", "").is_ok());
    }

    #[test]
    fn a_cancel_before_the_export_has_begun_stops_it_as_it_begins() {
        let exports = Exports::default();
        // the card shows (the export made ready), Cancel comes before the core has begun it
        exports.arm();
        exports.cancel();
        let run = ExportRun::begin(&exports, "PSD");
        assert!(run.cancelled());
        // the export after, made ready again, isn't
        exports.arm();
        assert!(!ExportRun::begin(&exports, "PSD").cancelled());
    }

    #[test]
    fn a_cancelled_export_still_ending_stays_cancelled_as_the_next_begins() {
        let exports = Exports::default();
        let old = ExportRun::begin(&exports, "PNG");
        exports.cancel();
        exports.arm();
        let new = ExportRun::begin(&exports, "PNG");
        assert!(old.cancelled());
        assert!(!new.cancelled());
    }

    #[test]
    fn an_export_for_no_ones_card_isnt_cancelled() {
        let exports = Exports::default();
        let quiet = ExportRun::begin_quiet(&exports, "package");
        exports.arm();
        exports.cancel();
        assert!(!quiet.cancelled());
        assert!(quiet.stage("writing", "").is_ok());
    }

    #[test]
    fn the_stage_under_way_is_told_and_cleared_as_the_export_ends() {
        let exports = Exports::default();
        let run = ExportRun::begin(&exports, "PSD");
        run.stage("reading layers", "3 of 12").unwrap();
        assert_eq!(exports.stage(), "reading layers (3 of 12)");
        let _ = run.end(Ok(()), |_| String::new());
        assert_eq!(exports.stage(), "");
    }

    #[test]
    fn the_log_line_names_each_kind_of_stage_with_its_time() {
        let ms = Duration::from_millis;
        let spent = [("waiting for the canvas", ms(2)), ("reading layers", ms(1200)), ("writing", ms(220))];
        assert_eq!(
            ended_line("PSD 2048×2048", &Ended::Made("3456 bytes".into()), ms(1422), &spent),
            "export PSD 2048×2048: made in 1422 ms (waiting for the canvas 2 ms, reading layers 1200 ms, writing 220 ms), 3456 bytes"
        );
        assert_eq!(
            ended_line("PSD", &Ended::Cancelled("reading layers (3 of 12)".into()), ms(912), &spent[..2]),
            "export PSD: cancelled at reading layers (3 of 12) after 912 ms (waiting for the canvas 2 ms, reading layers 1200 ms)"
        );
        assert_eq!(
            ended_line(
                "PNG",
                &Ended::Failed("reading the picture".into(), "the GPU device was lost".into()),
                ms(5),
                &[]
            ),
            "export PNG: failed at reading the picture after 5 ms (): the GPU device was lost"
        );
    }

    #[test]
    fn a_stage_begun_again_adds_to_its_kinds_time() {
        let exports = Exports::default();
        let run = ExportRun::begin(&exports, "PSD");
        run.stage("reading layers", "1 of 3").unwrap();
        run.stage("writing", "").unwrap();
        run.stage("reading layers", "2 of 3").unwrap();
        run.stage("writing", "").unwrap();
        let kinds: Vec<&str> = run.spent.borrow().iter().map(|(k, _)| *k).collect();
        assert_eq!(kinds, ["beginning", "reading layers", "writing"]);
    }
}
