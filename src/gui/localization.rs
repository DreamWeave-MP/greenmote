macro_rules! route_text {
    ($key:expr, $language:ident, $convert:ident, $dialogs:ident, $settings:ident) => {
        match $key {
            UiText::Language
            | UiText::EnglishLanguage
            | UiText::FrenchLanguage
            | UiText::GermanLanguage
            | UiText::RussianLanguage
            | UiText::SpanishLanguage
            | UiText::SwedishLanguage
            | UiText::Convert
            | UiText::Unclip
            | UiText::Settings
            | UiText::General
            | UiText::Browse
            | UiText::Save
            | UiText::Cancel
            | UiText::Add
            | UiText::Discard
            | UiText::Close
            | UiText::Up
            | UiText::Down
            | UiText::ShowPreviousItems
            | UiText::ShowNextItems => $language($key),
            UiText::RunOptions
            | UiText::TargetPlugin
            | UiText::MeshGeneratorIni
            | UiText::WriteChangesToPlugin
            | UiText::DetailedRefDiagnostics
            | UiText::DetailedRefDiagnosticsTooltip
            | UiText::StartConversion
            | UiText::WriteChanges
            | UiText::InspectPlugin
            | UiText::DryRun
            | UiText::DebugDiagnostics
            | UiText::AutoEnableGeneratedPlugins
            | UiText::SaveAsDefaults
            | UiText::ResetFromSaved
            | UiText::RunOptionsDiffer
            | UiText::RunOptionsMatch
            | UiText::SaveOrDiscardSettingsBeforeDefaults
            | UiText::ClearOutput
            | UiText::CopyOutput
            | UiText::OpenOutputDir
            | UiText::OpenLog => $convert($key),
            UiText::ConfirmUnclipWriteTitle
            | UiText::ConfirmUnclipWriteMessage
            | UiText::Target
            | UiText::EnabledWriteActions
            | UiText::UnsavedSettingsTitle
            | UiText::UnsavedSettingsMessage
            | UiText::SaveBeforeContinuing
            | UiText::MalformedConfigTitle
            | UiText::MalformedConfigMessage
            | UiText::ReplaceWithDefaults
            | UiText::BackupBeforeReplacing
            | UiText::BackupRegenerateContinue
            | UiText::OpenMwConfigNotFoundTitle
            | UiText::OpenMwConfigNotFoundMessage
            | UiText::ChooseOpenMwConfigBeforeContinuing
            | UiText::SelectOpenMwConfig
            | UiText::SelectUnclipTargetPlugin
            | UiText::SelectMeshGeneratorIni => $dialogs($key),
            UiText::OpenMwConfig
            | UiText::OpenMwPlugins
            | UiText::UsingOpenMwAutodetection
            | UiText::OpenMwConfigCannotChangeWhileRunning
            | UiText::ConvertOutputDirectory
            | UiText::OutputFromDataLocal
            | UiText::OutputCliOverride
            | UiText::OutputWorkingDirectoryFallback
            | UiText::GrassIdPatterns
            | UiText::NoGrassIdPatterns
            | UiText::ExcludePatterns
            | UiText::NoExcludePatterns
            | UiText::IgnoredPlugins
            | UiText::NoIgnoredPlugins
            | UiText::RunOptionsConfiguredOnConvert
            | UiText::WriteActions
            | UiText::TerrainZAction
            | UiText::StaticDeleteAction
            | UiText::StaticMoveAction
            | UiText::OrientAction
            | UiText::NoWriteActionsWarning
            | UiText::PolicyNumbers
            | UiText::OriginHeightTolerance
            | UiText::OriginHeightToleranceTooltip
            | UiText::OrientationTolerance
            | UiText::OrientationToleranceTooltip
            | UiText::RelocationStepDistance
            | UiText::RelocationStepDistanceTooltip
            | UiText::RelocationProbeRings
            | UiText::RelocationProbeRingsTooltip
            | UiText::RegexFiltersHelp
            | UiText::IncludeGrassIds
            | UiText::NoIncludeGrassIds
            | UiText::ExcludeGrassIds
            | UiText::NoExcludeGrassIds
            | UiText::IncludeOccluderIds
            | UiText::NoIncludeOccluderIds
            | UiText::ExcludeOccluderIds
            | UiText::NoExcludeOccluderIds
            | UiText::AddGrassIdPatternTitle
            | UiText::AddExcludePatternTitle
            | UiText::AddIgnoredPluginTitle
            | UiText::AddIncludeGrassIdRegexTitle
            | UiText::AddExcludeGrassIdRegexTitle
            | UiText::AddIncludeOccluderIdRegexTitle
            | UiText::AddExcludeOccluderIdRegexTitle
            | UiText::GrassIdPatternPrompt
            | UiText::ExcludePatternPrompt
            | UiText::IgnoredPluginPrompt
            | UiText::IncludeGrassIdRegexPrompt
            | UiText::ExcludeGrassIdRegexPrompt
            | UiText::IncludeOccluderIdRegexPrompt
            | UiText::ExcludeOccluderIdRegexPrompt => $settings($key),
        }
    };
}

mod english;
mod french;
mod german;
mod russian;
mod spanish;
mod swedish;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum UiLanguage {
    #[default]
    English,
    French,
    German,
    Russian,
    Spanish,
    Swedish,
}

impl UiLanguage {
    pub(super) const ALL: [Self; 6] = [
        Self::English,
        Self::French,
        Self::German,
        Self::Russian,
        Self::Spanish,
        Self::Swedish,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum UiText {
    Language,
    EnglishLanguage,
    FrenchLanguage,
    GermanLanguage,
    RussianLanguage,
    SpanishLanguage,
    SwedishLanguage,
    Convert,
    Unclip,
    Settings,
    General,
    RunOptions,
    TargetPlugin,
    MeshGeneratorIni,
    Browse,
    WriteChangesToPlugin,
    DetailedRefDiagnostics,
    DetailedRefDiagnosticsTooltip,
    StartConversion,
    WriteChanges,
    InspectPlugin,
    DryRun,
    DebugDiagnostics,
    AutoEnableGeneratedPlugins,
    SaveAsDefaults,
    ResetFromSaved,
    RunOptionsDiffer,
    RunOptionsMatch,
    SaveOrDiscardSettingsBeforeDefaults,
    Save,
    Cancel,
    Add,
    Discard,
    Close,
    ClearOutput,
    CopyOutput,
    OpenOutputDir,
    OpenLog,
    ConfirmUnclipWriteTitle,
    ConfirmUnclipWriteMessage,
    Target,
    EnabledWriteActions,
    UnsavedSettingsTitle,
    UnsavedSettingsMessage,
    SaveBeforeContinuing,
    MalformedConfigTitle,
    MalformedConfigMessage,
    ReplaceWithDefaults,
    BackupBeforeReplacing,
    BackupRegenerateContinue,
    OpenMwConfigNotFoundTitle,
    OpenMwConfigNotFoundMessage,
    ChooseOpenMwConfigBeforeContinuing,
    SelectOpenMwConfig,
    SelectUnclipTargetPlugin,
    SelectMeshGeneratorIni,
    OpenMwConfig,
    OpenMwPlugins,
    UsingOpenMwAutodetection,
    OpenMwConfigCannotChangeWhileRunning,
    ConvertOutputDirectory,
    OutputFromDataLocal,
    OutputCliOverride,
    OutputWorkingDirectoryFallback,
    GrassIdPatterns,
    NoGrassIdPatterns,
    ExcludePatterns,
    NoExcludePatterns,
    IgnoredPlugins,
    NoIgnoredPlugins,
    RunOptionsConfiguredOnConvert,
    WriteActions,
    TerrainZAction,
    StaticDeleteAction,
    StaticMoveAction,
    OrientAction,
    NoWriteActionsWarning,
    PolicyNumbers,
    OriginHeightTolerance,
    OriginHeightToleranceTooltip,
    OrientationTolerance,
    OrientationToleranceTooltip,
    RelocationStepDistance,
    RelocationStepDistanceTooltip,
    RelocationProbeRings,
    RelocationProbeRingsTooltip,
    RegexFiltersHelp,
    IncludeGrassIds,
    NoIncludeGrassIds,
    ExcludeGrassIds,
    NoExcludeGrassIds,
    IncludeOccluderIds,
    NoIncludeOccluderIds,
    ExcludeOccluderIds,
    NoExcludeOccluderIds,
    AddGrassIdPatternTitle,
    AddExcludePatternTitle,
    AddIgnoredPluginTitle,
    AddIncludeGrassIdRegexTitle,
    AddExcludeGrassIdRegexTitle,
    AddIncludeOccluderIdRegexTitle,
    AddExcludeOccluderIdRegexTitle,
    GrassIdPatternPrompt,
    ExcludePatternPrompt,
    IgnoredPluginPrompt,
    IncludeGrassIdRegexPrompt,
    ExcludeGrassIdRegexPrompt,
    IncludeOccluderIdRegexPrompt,
    ExcludeOccluderIdRegexPrompt,
    Up,
    Down,
    ShowPreviousItems,
    ShowNextItems,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Localizer {
    language: UiLanguage,
}

impl Localizer {
    pub(super) const fn language(self) -> UiLanguage {
        self.language
    }

    pub(super) fn set_language(&mut self, language: UiLanguage) {
        self.language = language;
    }

    pub(super) fn text(self, key: UiText) -> &'static str {
        match self.language {
            UiLanguage::English => english::text(key),
            UiLanguage::French => french::text(key),
            UiLanguage::German => german::text(key),
            UiLanguage::Russian => russian::text(key),
            UiLanguage::Spanish => spanish::text(key),
            UiLanguage::Swedish => swedish::text(key),
        }
    }

    pub(super) fn showing_items(self, start: usize, end: usize, total: usize) -> String {
        match self.language {
            UiLanguage::English => format!("Showing {start}-{end} of {total}"),
            UiLanguage::French => format!("Affichage de {start} à {end} sur {total}"),
            UiLanguage::German => format!("Zeige {start}-{end} von {total}"),
            UiLanguage::Russian => format!("Показаны {start}-{end} из {total}"),
            UiLanguage::Spanish => format!("Mostrando {start}-{end} de {total}"),
            UiLanguage::Swedish => format!("Visar {start}-{end} av {total}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn localizes_fixed_gui_labels() {
        let localizer = Localizer::default();

        assert_eq!(localizer.language(), UiLanguage::English);
        assert_eq!(localizer.text(UiText::Language), "Language");
        assert_eq!(localizer.text(UiText::StartConversion), "Start conversion");
        assert_eq!(localizer.showing_items(1, 6, 9), "Showing 1-6 of 9");

        let mut french = Localizer::default();
        french.set_language(UiLanguage::French);
        assert_eq!(french.text(UiText::Settings), "Paramètres");
        assert_eq!(french.showing_items(1, 6, 9), "Affichage de 1 à 6 sur 9");

        let mut german = Localizer::default();
        german.set_language(UiLanguage::German);
        assert_eq!(german.text(UiText::Settings), "Einstellungen");

        let mut russian = Localizer::default();
        russian.set_language(UiLanguage::Russian);
        assert_eq!(russian.text(UiText::Settings), "Настройки");

        let mut spanish = Localizer::default();
        spanish.set_language(UiLanguage::Spanish);
        assert_eq!(spanish.text(UiText::Settings), "Ajustes");

        let mut swedish = Localizer::default();
        swedish.set_language(UiLanguage::Swedish);
        assert_eq!(swedish.text(UiText::Settings), "Inställningar");
    }
}
