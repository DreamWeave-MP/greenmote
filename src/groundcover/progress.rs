#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConversionPhase {
    LoadingStaticPlugins,
    PlanningStatics,
    LoadingCellPlugins,
    ScanningCells,
    ResolvingMeshes,
    WritingPlugins,
    CopyingMeshes,
    AutoEnabling,
    WritingLog,
}

impl ConversionPhase {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::LoadingStaticPlugins => "Loading plugins for static planning",
            Self::PlanningStatics => "Planning matching statics",
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConversionEvent {
    PhaseStarted(ConversionPhase),
    Progress {
        phase: ConversionPhase,
        current: usize,
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
