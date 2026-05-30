//! Progress and cancellation primitives for conversion runs.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Coarse conversion phases reported to progress observers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ConversionPhase {
    /// Loading plugins for source record planning.
    LoadingStaticPlugins,
    /// Selecting matching source records.
    PlanningStatics,
    /// Loading plugins for exterior cell scanning.
    LoadingCellPlugins,
    /// Scanning exterior cell references.
    ScanningCells,
    /// Resolving source meshes before output writes.
    ResolvingMeshes,
    /// Writing generated plugin files.
    WritingPlugins,
    /// Copying resolved meshes into the output tree.
    CopyingMeshes,
    /// Updating `OpenMW` configuration when auto-enable is requested.
    AutoEnabling,
    /// Writing `greenmote.log`.
    WritingLog,
}

impl ConversionPhase {
    /// Returns an English label suitable for progress displays.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::LoadingStaticPlugins => "Loading plugins for source record planning",
            Self::PlanningStatics => "Planning matching source records",
            Self::LoadingCellPlugins => "Loading plugins for cell scanning",
            Self::ScanningCells => "Scanning exterior cells",
            Self::ResolvingMeshes => "Resolving meshes",
            Self::WritingPlugins => "Writing output plugins",
            Self::CopyingMeshes => "Copying meshes",
            Self::AutoEnabling => "Updating OpenMW config",
            Self::WritingLog => "Writing greenmote.log",
        }
    }
}

/// Progress notification emitted during conversion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ConversionEvent {
    /// A new conversion phase has started.
    PhaseStarted(ConversionPhase),
    /// Counted progress within a conversion phase.
    Progress {
        /// Phase that is currently reporting progress.
        phase: ConversionPhase,
        /// Completed work count.
        current: usize,
        /// Total known work count for the phase.
        total: usize,
    },
}

/// Receives conversion progress events.
///
/// Implementations must be safe to call from Rayon worker threads. Counted progress events from
/// parallel phases are monotonic by work completed, but their delivery order is not guaranteed.
/// Consumers that display the latest progress should treat counted progress as completed-work
/// samples and display the maximum observed `current` for each phase/total, not the most recently
/// delivered sample.
pub type EventSink<'a> = dyn Fn(ConversionEvent) + Sync + 'a;

/// Shared cancellation flag for long-running conversion work.
///
/// Clones observe the same atomic flag. Cancellation is cooperative: callers set the flag with
/// [`CancellationToken::cancel`], and worker phases check [`CancellationToken::is_cancelled`] at
/// phase-specific boundaries.
#[derive(Clone, Debug, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    /// Requests cooperative cancellation for all clones of this token.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    /// Returns whether cancellation has been requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

pub fn emit_phase(events: &EventSink<'_>, phase: ConversionPhase) {
    events(ConversionEvent::PhaseStarted(phase));
}

pub fn emit_progress(events: &EventSink<'_>, phase: ConversionPhase, current: usize, total: usize) {
    events(ConversionEvent::Progress {
        phase,
        current,
        total,
    });
}

#[cfg(test)]
mod tests {
    use super::CancellationToken;

    #[test]
    fn cancellation_token_is_shared_between_clones() {
        let token = CancellationToken::default();
        let clone = token.clone();

        clone.cancel();

        assert!(token.is_cancelled());
    }
}
