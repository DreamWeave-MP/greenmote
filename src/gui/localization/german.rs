// SPDX-License-Identifier: GPL-3.0-only

use super::UiText;

pub(super) const fn text(key: UiText) -> &'static str {
    route_text!(key, language_text, convert_text, dialog_text, settings_text)
}

pub(super) fn unclip_target_count(count: usize) -> String {
    if count == 1 {
        "Ziel: 1 Plugin".to_owned()
    } else {
        format!("Ziele: {count} Plugins")
    }
}

pub(super) fn unclip_target_overflow(count: usize) -> String {
    if count == 1 {
        "... und 1 weiteres".to_owned()
    } else {
        format!("... und {count} weitere")
    }
}

const fn language_text(key: UiText) -> &'static str {
    match key {
        UiText::Language => "Sprache",
        UiText::EnglishLanguage => "Englisch",
        UiText::FrenchLanguage => "Französisch",
        UiText::GermanLanguage => "Deutsch",
        UiText::RussianLanguage => "Russisch",
        UiText::SpanishLanguage => "Spanisch",
        UiText::SwedishLanguage => "Schwedisch",
        UiText::Convert => "Konvertieren",
        UiText::Unclip => "Unclip",
        UiText::Settings => "Einstellungen",
        UiText::General => "Allgemein",
        UiText::Save => "Speichern",
        UiText::Cancel => "Abbrechen",
        UiText::Add => "Hinzufügen",
        UiText::Discard => "Verwerfen",
        UiText::Close => "Schließen",
        UiText::Up => "Hoch",
        UiText::Down => "Runter",
        UiText::ShowPreviousItems => "Vorherige Elemente anzeigen",
        UiText::ShowNextItems => "Nächste Elemente anzeigen",
        _ => unreachable!(),
    }
}

const fn convert_text(key: UiText) -> &'static str {
    match key {
        UiText::RunOptions => "Ausführungsoptionen",
        UiText::TargetPlugins => "Ziel-Plugins",
        UiText::AddFiles => "Dateien hinzufügen...",
        UiText::AddTargetPath => "Ziel/Pfad hinzufügen",
        UiText::RemoveSelectedTarget => "Ausgewähltes Ziel entfernen",
        UiText::ClearTargets => "Ziele löschen",
        UiText::EmptyTargetList => "Keine Ziel-Plugins hinzugefügt.",
        UiText::TargetPathEntry => "Plugin-Name oder Pfad",
        UiText::WriteChangesToPlugin => "Änderungen ins Plugin schreiben",
        UiText::StartConversion => "Konvertierung starten",
        UiText::WriteChanges => "Änderungen schreiben",
        UiText::InspectPlugin => "Plugin prüfen",
        UiText::DryRun => "Probelauf",
        UiText::DebugDiagnostics => "Debug-Diagnose",
        UiText::AutoEnableGeneratedPlugins => "Generierte Plugins automatisch aktivieren",
        UiText::SaveAsDefaults => "Als Standard speichern",
        UiText::ResetFromSaved => "Aus Gespeichertem zurücksetzen",
        UiText::RunOptionsDiffer => {
            "Ausführungsoptionen weichen von den gespeicherten Standards ab."
        }
        UiText::RunOptionsMatch => "Ausführungsoptionen entsprechen den gespeicherten Standards.",
        UiText::SaveOrDiscardSettingsBeforeDefaults => {
            "Änderungen in Einstellungen speichern oder verwerfen, bevor diese als Standard gespeichert werden."
        }
        UiText::ClearOutput => "Ausgabe löschen",
        UiText::CopyOutput => "Ausgabe kopieren",
        UiText::OpenOutputDir => "Ausgabeordner öffnen",
        UiText::OpenLog => "Log öffnen",
        UiText::WritingUnclipBatch => "Unclip-Stapel wird geschrieben...",
        UiText::InspectingUnclipBatch => "Unclip-Stapel wird geprüft...",
        UiText::UnclipWrite => "Unclip-Schreiben",
        UiText::UnclipInspection => "Unclip-Prüfung",
        UiText::UnclipTargetPending => "Ausstehend",
        UiText::UnclipTargetRunning => "Läuft",
        UiText::UnclipTargetSucceeded => "Erfolgreich",
        UiText::UnclipTargetFailed => "Fehlgeschlagen",
        UiText::UnclipTargetSkipped => "Übersprungen",
        UiText::UnclipTargetCancelled => "Abgebrochen",
        _ => unreachable!(),
    }
}

const fn dialog_text(key: UiText) -> &'static str {
    match key {
        UiText::ConfirmUnclipWriteTitle => "Unclip-Schreiben bestätigen",
        UiText::ConfirmUnclipWriteMessage => {
            "Unclip wird die ausgewählten Ziel-Plugins ändern und Sicherungsdateien erstellen."
        }
        UiText::EnabledWriteActions => "Aktivierte Schreibaktionen:",
        UiText::UnsavedSettingsTitle => "Ungespeicherte Einstellungen",
        UiText::UnsavedSettingsMessage => "Einstellungen haben ungespeicherte Änderungen.",
        UiText::SaveBeforeContinuing => "Vor dem Fortfahren speichern?",
        UiText::MalformedConfigTitle => "Fehlerhafte Konfiguration",
        UiText::MalformedConfigMessage => "greenmote.toml konnte nicht geladen werden.",
        UiText::ReplaceWithDefaults => "Vor dem Fortfahren durch Standardwerte ersetzen.",
        UiText::BackupBeforeReplacing => {
            "Die aktuelle Datei wird zuerst als .bak-Datei verschoben."
        }
        UiText::BackupRegenerateContinue => "Sichern, neu erzeugen und fortfahren",
        UiText::OpenMwConfigNotFoundTitle => "OpenMW-Konfiguration nicht gefunden",
        UiText::OpenMwConfigNotFoundMessage => {
            "Greenmote konnte keine OpenMW-Konfigurationsdatei finden oder laden."
        }
        UiText::ChooseOpenMwConfigBeforeContinuing => {
            "Wähle einen gültigen OpenMW-Konfigurationspfad, bevor du fortfährst."
        }
        UiText::SelectOpenMwConfig => "OpenMW-Konfiguration auswählen",
        UiText::SelectUnclipTargetPlugins => "Unclip-Ziel-Plugins auswählen",
        _ => unreachable!(),
    }
}

const fn settings_text(key: UiText) -> &'static str {
    match key {
        UiText::OpenMwConfig => "OpenMW-Konfiguration",
        UiText::OpenMwPlugins => "OpenMW-Plugins",
        UiText::UsingOpenMwAutodetection => "OpenMW-Autodetektion wird verwendet.",
        UiText::OpenMwConfigCannotChangeWhileRunning => {
            "Die OpenMW-Konfiguration kann während eines laufenden Vorgangs nicht geändert werden."
        }
        UiText::ConvertOutputDirectory => "Ausgabeordner für Konvertierung",
        UiText::OutputFromDataLocal => {
            "Aus der data-local-Einstellung der ausgewählten OpenMW-Konfiguration."
        }
        UiText::OutputCliOverride => "Für diese Konvertierung überschrieben.",
        UiText::OutputWorkingDirectoryFallback => {
            "OpenMW hat keine data-local-Einstellung, daher schreibt Greenmote in den oben gezeigten aktuellen Arbeitsordner. OpenMW kann die generierten Dateien nur laden, wenn dieser Ordner als data-local oder data= konfiguriert ist."
        }
        UiText::GrassIdPatterns | UiText::GrassIdPatternPrompt => "Grass-ID-Muster",
        UiText::NoGrassIdPatterns => "Keine Grass-ID-Muster konfiguriert.",
        UiText::ExcludePatterns | UiText::ExcludePatternPrompt => "Ausschlussmuster",
        UiText::NoExcludePatterns => "Keine Ausschlussmuster konfiguriert.",
        UiText::IgnoredPlugins => "Ignorierte Plugins",
        UiText::NoIgnoredPlugins => "Keine ignorierten Plugins konfiguriert.",
        UiText::RunOptionsConfiguredOnConvert => {
            "Ausführungsoptionen werden auf dem Konvertieren-Bildschirm konfiguriert."
        }
        UiText::WriteActions => "Schreibaktionen",
        UiText::TerrainZAction => "Terrain Z (terrain-z)",
        UiText::StaticDeleteAction => "Vollständig statisch verdeckte Refs löschen (static-delete)",
        UiText::StaticMoveAction => "Statisch verdeckte Refs verschieben (static-move)",
        UiText::OrientAction => "Refs am Terrain ausrichten (orient)",
        UiText::NoWriteActionsWarning => "Warnung: Schreibmodus erzeugt keine Policy-Aktionen.",
        UiText::PolicyNumbers => "Richtlinienwerte",
        UiText::OriginHeightTolerance => "Ursprungshöhentoleranz",
        UiText::OriginHeightToleranceTooltip => {
            "Maximales Referenzursprung/Terrain-Z-Delta, das als bereits auf dem Terrain gilt. Config key: origin_epsilon."
        }
        UiText::OrientationTolerance => "Ausrichtungstoleranz",
        UiText::OrientationToleranceTooltip => {
            "Maximaler Neigungswinkel in Grad, der als bereits am Terrain ausgerichtet gilt. Config key: orientation_epsilon."
        }
        UiText::RelocationStepDistance => "Verschiebungsschrittweite",
        UiText::RelocationStepDistanceTooltip => {
            "Horizontaler Abstand zwischen Verschiebungsproben für statische Grenzen. Config key: relocation_step."
        }
        UiText::RelocationProbeRings => "Ringe für Verschiebungsproben",
        UiText::RelocationProbeRingsTooltip => {
            "Anzahl der Verschiebungsprobenringe für statische Verschiebungen. Config key: relocation_steps."
        }
        UiText::RegexFiltersHelp => {
            "Regex-Filter verwenden groß-/kleinschreibungsunabhängige Voll-ID-Regexe. Ausschluss gewinnt vor Einschluss. Leerer Einschluss bedeutet alle."
        }
        UiText::IncludeGrassIds => "Grass-IDs einschließen",
        UiText::NoIncludeGrassIds => "Keine einschließenden Grass-ID-Filter konfiguriert.",
        UiText::ExcludeGrassIds => "Grass-IDs ausschließen",
        UiText::NoExcludeGrassIds => "Keine ausschließenden Grass-ID-Filter konfiguriert.",
        UiText::IncludeOccluderIds => "Occluder-IDs einschließen",
        UiText::NoIncludeOccluderIds => "Keine einschließenden Occluder-ID-Filter konfiguriert.",
        UiText::ExcludeOccluderIds => "Occluder-IDs ausschließen",
        UiText::NoExcludeOccluderIds => "Keine ausschließenden Occluder-ID-Filter konfiguriert.",
        UiText::AddGrassIdPatternTitle => "Grass-ID-Muster hinzufügen",
        UiText::AddExcludePatternTitle => "Ausschlussmuster hinzufügen",
        UiText::AddIgnoredPluginTitle => "Ignoriertes Plugin hinzufügen",
        UiText::AddIncludeGrassIdRegexTitle => "Einschließende Grass-ID-Regex hinzufügen",
        UiText::AddExcludeGrassIdRegexTitle => "Ausschließende Grass-ID-Regex hinzufügen",
        UiText::AddIncludeOccluderIdRegexTitle => "Einschließende Occluder-ID-Regex hinzufügen",
        UiText::AddExcludeOccluderIdRegexTitle => "Ausschließende Occluder-ID-Regex hinzufügen",
        UiText::IgnoredPluginPrompt => "Ignoriertes Plugin",
        UiText::IncludeGrassIdRegexPrompt => "Einschließende Grass-ID-Regex",
        UiText::ExcludeGrassIdRegexPrompt => "Ausschließende Grass-ID-Regex",
        UiText::IncludeOccluderIdRegexPrompt => "Einschließende Occluder-ID-Regex",
        UiText::ExcludeOccluderIdRegexPrompt => "Ausschließende Occluder-ID-Regex",
        _ => unreachable!(),
    }
}
