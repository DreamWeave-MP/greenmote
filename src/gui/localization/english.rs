// SPDX-License-Identifier: GPL-3.0-only

use super::UiText;

pub(super) const fn text(key: UiText) -> &'static str {
    route_text!(key, language_text, convert_text, dialog_text, settings_text)
}

pub(super) fn unclip_target_count(count: usize) -> String {
    if count == 1 {
        "Target: 1 plugin".to_owned()
    } else {
        format!("Targets: {count} plugins")
    }
}

pub(super) fn unclip_target_overflow(count: usize) -> String {
    format!("... and {count} more")
}

const fn language_text(key: UiText) -> &'static str {
    match key {
        UiText::Language => "Language",
        UiText::EnglishLanguage => "English",
        UiText::FrenchLanguage => "French",
        UiText::GermanLanguage => "German",
        UiText::RussianLanguage => "Russian",
        UiText::SpanishLanguage => "Spanish",
        UiText::SwedishLanguage => "Swedish",
        UiText::Convert => "Convert",
        UiText::Unclip => "Unclip",
        UiText::Settings => "Settings",
        UiText::General => "General",
        UiText::Save => "Save",
        UiText::Cancel => "Cancel",
        UiText::Add => "Add",
        UiText::Discard => "Discard",
        UiText::Close => "Close",
        UiText::Up => "Up",
        UiText::Down => "Down",
        UiText::ShowPreviousItems => "Show previous items",
        UiText::ShowNextItems => "Show next items",
        _ => unreachable!(),
    }
}

const fn convert_text(key: UiText) -> &'static str {
    match key {
        UiText::RunOptions => "Run options",
        UiText::TargetPlugins => "Target plugins",
        UiText::AddFiles => "Add files...",
        UiText::AddTargetPath => "Add target/path",
        UiText::SetUnclipOutputPlugin => "Set output...",
        UiText::ClearUnclipOutputPlugin => "Clear output plugin",
        UiText::RemoveSelectedTarget => "Remove selected target",
        UiText::ClearTargets => "Clear targets",
        UiText::EmptyTargetList => "No target plugins added.",
        UiText::TargetPathEntry => "Plugin name or path",
        UiText::StartConversion => "Start conversion",
        UiText::WriteChanges => "Write changes",
        UiText::InspectPlugin => "Inspect plugin",
        UiText::DryRun => "Dry run",
        UiText::DebugDiagnostics => "Debug diagnostics",
        UiText::AutoEnableGeneratedPlugins => "Auto-enable generated plugins",
        UiText::SaveAsDefaults => "Save as defaults",
        UiText::ResetFromSaved => "Reset from saved",
        UiText::RunOptionsDiffer => "Run options differ from saved defaults.",
        UiText::RunOptionsMatch => "Run options match saved defaults.",
        UiText::SaveOrDiscardSettingsBeforeDefaults => {
            "Save or discard Settings changes before saving these as defaults."
        }
        UiText::ClearOutput => "Clear output",
        UiText::CopyOutput => "Copy output",
        UiText::OpenOutputDir => "Open output dir",
        UiText::OpenLog => "Open log",
        UiText::WritingUnclipBatch => "Writing Unclip batch...",
        UiText::InspectingUnclipBatch => "Inspecting Unclip batch...",
        UiText::UnclipWrite => "Unclip write",
        UiText::UnclipInspection => "Unclip inspection",
        UiText::UnclipTargetPending => "Pending",
        UiText::UnclipTargetRunning => "Running",
        UiText::UnclipTargetSucceeded => "Succeeded",
        UiText::UnclipTargetFailed => "Failed",
        UiText::UnclipTargetSkipped => "Skipped",
        UiText::UnclipTargetCancelled => "Cancelled",
        _ => unreachable!(),
    }
}

const fn dialog_text(key: UiText) -> &'static str {
    match key {
        UiText::ConfirmUnclipWriteTitle => "Confirm Unclip write",
        UiText::ConfirmUnclipWriteMessage => {
            "Unclip will modify the selected target plugin(s) and create backup files."
        }
        UiText::EnabledWriteActions => "Enabled write actions:",
        UiText::UnsavedSettingsTitle => "Unsaved settings",
        UiText::UnsavedSettingsMessage => "Settings have unsaved changes.",
        UiText::SaveBeforeContinuing => "Save them before continuing?",
        UiText::MalformedConfigTitle => "Malformed config",
        UiText::MalformedConfigMessage => "greenmote.toml could not be loaded.",
        UiText::ReplaceWithDefaults => "Replace it with defaults before continuing.",
        UiText::BackupBeforeReplacing => {
            "The current file will be moved aside as a .bak file first."
        }
        UiText::BackupRegenerateContinue => "Back up, regenerate, and continue",
        UiText::OpenMwConfigNotFoundTitle => "OpenMW config not found",
        UiText::OpenMwConfigNotFoundMessage => {
            "Greenmote could not find or load an OpenMW configuration file."
        }
        UiText::ChooseOpenMwConfigBeforeContinuing => {
            "Choose a valid OpenMW config path before continuing."
        }
        UiText::SelectOpenMwConfig => "Select OpenMW Config",
        UiText::SelectUnclipTargetPlugins => "Select Unclip Target Plugins",
        UiText::SelectUnclipOutputPlugin => "Select Unclip output plugin",
        _ => unreachable!(),
    }
}

const fn settings_text(key: UiText) -> &'static str {
    match key {
        UiText::OpenMwConfig => "OpenMW config",
        UiText::OpenMwPlugins => "OpenMW plugins",
        UiText::UsingOpenMwAutodetection => "Using OpenMW autodetection.",
        UiText::OpenMwConfigCannotChangeWhileRunning => {
            "OpenMW config cannot be changed while a run is active."
        }
        UiText::ConvertOutputDirectory => "Convert output directory",
        UiText::OutputFromDataLocal => {
            "From the selected OpenMW configuration's data-local setting."
        }
        UiText::OutputCliOverride => "Overridden for this conversion run.",
        UiText::OutputWorkingDirectoryFallback => {
            "OpenMW has no data-local setting, so Greenmote will write to the current working directory shown above. OpenMW can only load the generated files if this folder is configured as data-local or data=."
        }
        UiText::GrassIdPatterns => "Grass ID patterns",
        UiText::NoGrassIdPatterns => "No grass ID patterns configured.",
        UiText::ExcludePatterns => "Exclude patterns",
        UiText::NoExcludePatterns => "No exclude patterns configured.",
        UiText::IgnoredPlugins => "Ignored plugins",
        UiText::NoIgnoredPlugins => "No ignored plugins configured.",
        UiText::RunOptionsConfiguredOnConvert => {
            "Run options are configured on the Convert screen."
        }
        UiText::WriteActions => "Write actions",
        UiText::TerrainZAction => "Terrain Z (terrain-z)",
        UiText::WaterDeleteAction => "Delete refs crossing exterior water (water-delete)",
        UiText::RoadDeleteAction => "Delete refs on road textures (road-delete)",
        UiText::StaticDeleteAction => "Delete fully static-occluded refs (static-delete)",
        UiText::StaticMoveAction => "Move static-occluded refs (static-move)",
        UiText::OrientAction => "Orient refs to terrain (orient)",
        UiText::NoWriteActionsWarning => "Warning: write mode will produce no policy actions.",
        UiText::PolicyNumbers => "Policy numbers",
        UiText::OriginHeightTolerance => "Origin height tolerance",
        UiText::OriginHeightToleranceTooltip => {
            "Maximum reference origin/terrain Z delta treated as already on terrain. Config key: origin_epsilon."
        }
        UiText::OrientationTolerance => "Orientation tolerance",
        UiText::OrientationToleranceTooltip => {
            "Maximum tilt angle in degrees treated as already aligned to terrain. Config key: orientation_epsilon."
        }
        UiText::RelocationStepDistance => "Relocation step distance",
        UiText::RelocationStepDistanceTooltip => {
            "Horizontal distance between static-bounds relocation probes. Config key: relocation_step."
        }
        UiText::RelocationProbeRings => "Relocation probe rings",
        UiText::RelocationProbeRingsTooltip => {
            "Number of relocation probe rings to try for static-bounds moves. Config key: relocation_steps."
        }
        UiText::RegexFiltersHelp => {
            "Regex filters use case-insensitive full-ID regexes. Exclude wins over include. Empty include means all."
        }
        UiText::IncludeGrassIds => "Include grass IDs",
        UiText::NoIncludeGrassIds => "No include grass ID filters configured.",
        UiText::ExcludeGrassIds => "Exclude grass IDs",
        UiText::NoExcludeGrassIds => "No exclude grass ID filters configured.",
        UiText::IncludeOccluderIds => "Include occluder IDs",
        UiText::NoIncludeOccluderIds => "No include occluder ID filters configured.",
        UiText::ExcludeOccluderIds => "Exclude occluder IDs",
        UiText::NoExcludeOccluderIds => "No exclude occluder ID filters configured.",
        UiText::IncludeRoadTexturePaths => "Include road texture paths",
        UiText::NoIncludeRoadTexturePaths => "No extra road texture path filters configured.",
        UiText::ExcludeRoadTexturePaths => "Exclude road texture paths",
        UiText::NoExcludeRoadTexturePaths => "No road texture path exclusions configured.",
        UiText::AddGrassIdPatternTitle => "Add grass ID pattern",
        UiText::AddExcludePatternTitle => "Add exclude pattern",
        UiText::AddIgnoredPluginTitle => "Add ignored plugin",
        UiText::AddIncludeGrassIdRegexTitle => "Add include grass ID regex",
        UiText::AddExcludeGrassIdRegexTitle => "Add exclude grass ID regex",
        UiText::AddIncludeOccluderIdRegexTitle => "Add include occluder ID regex",
        UiText::AddExcludeOccluderIdRegexTitle => "Add exclude occluder ID regex",
        UiText::AddIncludeRoadTexturePathRegexTitle => "Add include road texture path regex",
        UiText::AddExcludeRoadTexturePathRegexTitle => "Add exclude road texture path regex",
        UiText::GrassIdPatternPrompt => "Grass ID pattern",
        UiText::ExcludePatternPrompt => "Exclude pattern",
        UiText::IgnoredPluginPrompt => "Ignored plugin",
        UiText::IncludeGrassIdRegexPrompt => "Include grass ID regex",
        UiText::ExcludeGrassIdRegexPrompt => "Exclude grass ID regex",
        UiText::IncludeOccluderIdRegexPrompt => "Include occluder ID regex",
        UiText::ExcludeOccluderIdRegexPrompt => "Exclude occluder ID regex",
        UiText::IncludeRoadTexturePathRegexPrompt => "Include road texture path regex",
        UiText::ExcludeRoadTexturePathRegexPrompt => "Exclude road texture path regex",
        _ => unreachable!(),
    }
}
