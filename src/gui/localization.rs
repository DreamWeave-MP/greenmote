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
            | UiText::TargetPlugins
            | UiText::AddFiles
            | UiText::AddTargetPath
            | UiText::RemoveSelectedTarget
            | UiText::ClearTargets
            | UiText::EmptyTargetList
            | UiText::TargetPathEntry
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
            | UiText::OpenLog
            | UiText::WritingUnclipBatch
            | UiText::InspectingUnclipBatch
            | UiText::UnclipWrite
            | UiText::UnclipInspection
            | UiText::UnclipTargetPending
            | UiText::UnclipTargetRunning
            | UiText::UnclipTargetSucceeded
            | UiText::UnclipTargetFailed
            | UiText::UnclipTargetSkipped => $convert($key),
            UiText::ConfirmUnclipWriteTitle
            | UiText::ConfirmUnclipWriteMessage
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
            | UiText::SelectUnclipTargetPlugins => $dialogs($key),
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
    TargetPlugins,
    AddFiles,
    AddTargetPath,
    RemoveSelectedTarget,
    ClearTargets,
    EmptyTargetList,
    TargetPathEntry,
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
    WritingUnclipBatch,
    InspectingUnclipBatch,
    UnclipWrite,
    UnclipInspection,
    UnclipTargetPending,
    UnclipTargetRunning,
    UnclipTargetSucceeded,
    UnclipTargetFailed,
    UnclipTargetSkipped,
    ConfirmUnclipWriteTitle,
    ConfirmUnclipWriteMessage,
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
    SelectUnclipTargetPlugins,
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

    pub(super) fn unclip_target_count(self, count: usize) -> String {
        match self.language {
            UiLanguage::English => english::unclip_target_count(count),
            UiLanguage::French => french::unclip_target_count(count),
            UiLanguage::German => german::unclip_target_count(count),
            UiLanguage::Russian => russian::unclip_target_count(count),
            UiLanguage::Spanish => spanish::unclip_target_count(count),
            UiLanguage::Swedish => swedish::unclip_target_count(count),
        }
    }

    pub(super) fn unclip_target_overflow(self, count: usize) -> String {
        match self.language {
            UiLanguage::English => english::unclip_target_overflow(count),
            UiLanguage::French => french::unclip_target_overflow(count),
            UiLanguage::German => german::unclip_target_overflow(count),
            UiLanguage::Russian => russian::unclip_target_overflow(count),
            UiLanguage::Spanish => spanish::unclip_target_overflow(count),
            UiLanguage::Swedish => swedish::unclip_target_overflow(count),
        }
    }

    pub(super) fn unclip_finished_status(
        self,
        label: &str,
        succeeded: usize,
        failed: usize,
        skipped: usize,
    ) -> String {
        if failed == 0 && skipped == 0 {
            return match self.language {
                UiLanguage::English => format!("{label} finished."),
                UiLanguage::French => format!("{label} terminée."),
                UiLanguage::German => format!("{label} abgeschlossen."),
                UiLanguage::Russian => format!("{label} завершена."),
                UiLanguage::Spanish => format!("{label} finalizada."),
                UiLanguage::Swedish => format!("{label} slutförd."),
            };
        }

        match self.language {
            UiLanguage::English => {
                format!(
                    "{label} finished: {succeeded} succeeded, {failed} failed, {skipped} skipped."
                )
            }
            UiLanguage::French => format!(
                "{label} terminée : {succeeded} réussis, {failed} échoués, {skipped} ignorés."
            ),
            UiLanguage::German => format!(
                "{label} abgeschlossen: {succeeded} erfolgreich, {failed} fehlgeschlagen, {skipped} übersprungen."
            ),
            UiLanguage::Russian => format!(
                "{label} завершена: успешно: {succeeded}, с ошибкой: {failed}, пропущено: {skipped}."
            ),
            UiLanguage::Spanish => format!(
                "{label} finalizada: {succeeded} correctos, {failed} fallidos, {skipped} omitidos."
            ),
            UiLanguage::Swedish => format!(
                "{label} slutförd: {succeeded} lyckades, {failed} misslyckades, {skipped} hoppades över."
            ),
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
        assert_eq!(
            localizer.text(UiText::ConfirmUnclipWriteMessage),
            "Unclip will modify the selected target plugin(s) and create backup files."
        );
        assert_eq!(localizer.showing_items(1, 6, 9), "Showing 1-6 of 9");
        assert_eq!(localizer.unclip_target_count(1), "Target: 1 plugin");
        assert_eq!(localizer.unclip_target_count(6), "Targets: 6 plugins");
        assert_eq!(localizer.unclip_target_overflow(2), "... and 2 more");
        assert_eq!(localizer.text(UiText::UnclipTargetPending), "Pending");
        assert_eq!(
            localizer.unclip_finished_status("Unclip write", 2, 1, 1),
            "Unclip write finished: 2 succeeded, 1 failed, 1 skipped."
        );

        let mut french = Localizer::default();
        french.set_language(UiLanguage::French);
        assert_eq!(french.text(UiText::Settings), "Paramètres");
        assert_eq!(french.showing_items(1, 6, 9), "Affichage de 1 à 6 sur 9");
        assert_eq!(french.unclip_target_count(6), "Cibles : 6 plugins");
        assert_eq!(french.unclip_target_overflow(1), "... et 1 autre");
        assert_eq!(french.unclip_target_overflow(2), "... et 2 autres");

        let mut german = Localizer::default();
        german.set_language(UiLanguage::German);
        assert_eq!(german.text(UiText::Settings), "Einstellungen");
        assert_eq!(german.unclip_target_count(6), "Ziele: 6 Plugins");
        assert_eq!(german.unclip_target_overflow(1), "... und 1 weiteres");
        assert_eq!(german.unclip_target_overflow(2), "... und 2 weitere");

        let mut russian = Localizer::default();
        russian.set_language(UiLanguage::Russian);
        assert_eq!(russian.text(UiText::Settings), "Настройки");
        assert_eq!(
            russian.text(UiText::ConfirmUnclipWriteMessage),
            "Unclip изменит выбранные целевые плагины и создаст резервные копии."
        );
        assert!(
            !russian
                .text(UiText::ConfirmUnclipWriteMessage)
                .contains("plugins")
        );
        assert_eq!(russian.unclip_target_count(1), "Цель: 1 плагин");
        assert_eq!(russian.unclip_target_count(2), "Цели: 2 плагина");
        assert_eq!(russian.unclip_target_count(5), "Цели: 5 плагинов");
        assert_eq!(russian.unclip_target_count(21), "Цели: 21 плагин");
        assert_eq!(russian.unclip_target_overflow(12), "... и еще 12 плагинов");
        assert_eq!(russian.text(UiText::TargetPlugins), "Целевые плагины");
        assert_eq!(russian.text(UiText::UnclipTargetSkipped), "Пропущено");
        assert_eq!(
            russian.unclip_finished_status("Запись Unclip", 2, 1, 1),
            "Запись Unclip завершена: успешно: 2, с ошибкой: 1, пропущено: 1."
        );

        let mut spanish = Localizer::default();
        spanish.set_language(UiLanguage::Spanish);
        assert_eq!(spanish.text(UiText::Settings), "Ajustes");
        assert_eq!(spanish.unclip_target_count(6), "Objetivos: 6 plugins");
        assert_eq!(spanish.unclip_target_overflow(2), "... y 2 más");

        let mut swedish = Localizer::default();
        swedish.set_language(UiLanguage::Swedish);
        assert_eq!(swedish.text(UiText::Settings), "Inställningar");
        assert_eq!(swedish.unclip_target_count(6), "Mål: 6 plugin");
        assert_eq!(swedish.unclip_target_overflow(2), "... och 2 till");
    }
}
