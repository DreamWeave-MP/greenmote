use super::UiText;

pub(super) const fn text(key: UiText) -> &'static str {
    route_text!(key, language_text, convert_text, dialog_text, settings_text)
}

const fn language_text(key: UiText) -> &'static str {
    match key {
        UiText::Language => "Språk",
        UiText::EnglishLanguage => "Engelska",
        UiText::FrenchLanguage => "Franska",
        UiText::GermanLanguage => "Tyska",
        UiText::RussianLanguage => "Ryska",
        UiText::SpanishLanguage => "Spanska",
        UiText::SwedishLanguage => "Svenska",
        UiText::Convert => "Konvertera",
        UiText::Unclip => "Unclip",
        UiText::Settings => "Inställningar",
        UiText::General => "Allmänt",
        UiText::Browse => "Bläddra...",
        UiText::Save => "Spara",
        UiText::Back => "Tillbaka",
        UiText::Cancel => "Avbryt",
        UiText::Add => "Lägg till",
        UiText::Discard => "Kasta",
        UiText::Close => "Stäng",
        UiText::Up => "Upp",
        UiText::Down => "Ned",
        UiText::ShowPreviousItems => "Visa föregående objekt",
        UiText::ShowNextItems => "Visa nästa objekt",
        _ => unreachable!(),
    }
}

const fn convert_text(key: UiText) -> &'static str {
    match key {
        UiText::RunOptions => "Köralternativ",
        UiText::TargetPlugin => "Målplugin",
        UiText::MeshGeneratorIni => "MeshGenerator-INI",
        UiText::WriteChangesToPlugin => "Skriv ändringar till plugin",
        UiText::DetailedRefDiagnostics => "Detaljerad logg",
        UiText::DetailedRefDiagnosticsTooltip => {
            "Skriver fullständig Unclip-diagnostik per referens till greenmote.log. Långsammare och kan skapa mycket stora loggfiler."
        }
        UiText::StartConversion => "Starta konvertering",
        UiText::WriteChanges => "Skriv ändringar",
        UiText::InspectPlugin => "Inspektera plugin",
        UiText::DryRun => "Torrkörning",
        UiText::DebugDiagnostics => "Felsökningsdiagnostik",
        UiText::AutoEnableGeneratedPlugins => "Aktivera genererade plugin automatiskt",
        UiText::SaveAsDefaults => "Spara som standard",
        UiText::ResetFromSaved => "Återställ från sparat",
        UiText::RunOptionsDiffer => "Köralternativen skiljer sig från sparad standard.",
        UiText::RunOptionsMatch => "Köralternativen matchar sparad standard.",
        UiText::SaveOrDiscardSettingsBeforeDefaults => {
            "Spara eller kasta ändringar i Inställningar innan du sparar dessa som standard."
        }
        UiText::ClearOutput => "Rensa utdata",
        UiText::CopyOutput => "Kopiera utdata",
        UiText::OpenOutputDir => "Öppna utdatakatalog",
        UiText::OpenLog => "Öppna logg",
        _ => unreachable!(),
    }
}

const fn dialog_text(key: UiText) -> &'static str {
    match key {
        UiText::ConfirmUnclipWriteTitle => "Bekräfta Unclip-skrivning",
        UiText::ConfirmUnclipWriteMessage => {
            "Unclip kommer att ändra målpluginet och skapa en säkerhetskopia."
        }
        UiText::Target => "Mål:",
        UiText::EnabledWriteActions => "Aktiverade skrivåtgärder:",
        UiText::UnsavedSettingsTitle => "Osparade inställningar",
        UiText::UnsavedSettingsMessage => "Inställningarna har osparade ändringar.",
        UiText::SaveBeforeContinuing => "Spara dem innan du fortsätter?",
        UiText::MalformedConfigTitle => "Felaktig config",
        UiText::MalformedConfigMessage => "greenmote.toml kunde inte läsas.",
        UiText::ReplaceWithDefaults => "Ersätt den med standardvärden innan du fortsätter.",
        UiText::BackupBeforeReplacing => "Den aktuella filen flyttas först undan som en .bak-fil.",
        UiText::BackupRegenerateContinue => "Säkerhetskopiera, återskapa och fortsätt",
        UiText::OpenMwConfigNotFoundTitle => "OpenMW-config hittades inte",
        UiText::OpenMwConfigNotFoundMessage => {
            "Greenmote kunde inte hitta eller läsa en OpenMW-konfigurationsfil."
        }
        UiText::ChooseOpenMwConfigBeforeContinuing => {
            "Välj en giltig OpenMW-configsökväg innan du fortsätter."
        }
        UiText::SelectOpenMwConfig => "Välj OpenMW-config",
        UiText::SelectUnclipTargetPlugin => "Välj Unclip-målplugin",
        UiText::SelectMeshGeneratorIni => "Välj MeshGenerator-INI",
        _ => unreachable!(),
    }
}

const fn settings_text(key: UiText) -> &'static str {
    match key {
        UiText::OpenMwConfig => "OpenMW-config",
        UiText::OpenMwPlugins => "OpenMW-plugin",
        UiText::UsingOpenMwAutodetection => "Använder automatisk OpenMW-detektering.",
        UiText::OpenMwConfigCannotChangeWhileRunning => {
            "OpenMW-config kan inte ändras medan en körning pågår."
        }
        UiText::ConvertOutputDirectory => "Utdatakatalog för konvertering",
        UiText::OutputFromDataLocal => {
            "Från den valda OpenMW-konfigurationens data-local-inställning."
        }
        UiText::OutputCliOverride => "Åsidosatt för denna konverteringskörning.",
        UiText::OutputWorkingDirectoryFallback => {
            "OpenMW har ingen data-local-inställning, så Greenmote skriver till den aktuella arbetskatalogen som visas ovan. OpenMW kan bara läsa de genererade filerna om mappen är konfigurerad som data-local eller data=."
        }
        UiText::GrassIdPatterns | UiText::GrassIdPatternPrompt => "Grass ID-mönster",
        UiText::NoGrassIdPatterns => "Inga Grass ID-mönster konfigurerade.",
        UiText::ExcludePatterns | UiText::ExcludePatternPrompt => "Exkluderingsmönster",
        UiText::NoExcludePatterns => "Inga exkluderingsmönster konfigurerade.",
        UiText::IgnoredPlugins => "Ignorerade plugin",
        UiText::NoIgnoredPlugins => "Inga ignorerade plugin konfigurerade.",
        UiText::RunOptionsConfiguredOnConvert => {
            "Köralternativ konfigureras på Konvertera-skärmen."
        }
        UiText::WriteActions => "Skrivåtgärder",
        UiText::TerrainZAction => "Terräng Z (terrain-z)",
        UiText::StaticDeleteAction => "Ta bort helt statiskt blockerade refs (static-delete)",
        UiText::StaticMoveAction => "Flytta statiskt blockerade refs (static-move)",
        UiText::OrientAction => "Rikta refs mot terräng (orient)",
        UiText::NoWriteActionsWarning => "Varning: skrivläge ger inga policyåtgärder.",
        UiText::PolicyNumbers => "Policyvärden",
        UiText::OriginHeightTolerance => "Tolerans för origo-höjd",
        UiText::OriginHeightToleranceTooltip => {
            "Största referensorigo/terräng-Z-delta som behandlas som redan på terrängen. Config key: origin_epsilon."
        }
        UiText::OrientationTolerance => "Orienteringstolerans",
        UiText::OrientationToleranceTooltip => {
            "Största lutningsvinkel i grader som behandlas som redan anpassad till terrängen. Config key: orientation_epsilon."
        }
        UiText::RelocationStepDistance => "Flyttstegsavstånd",
        UiText::RelocationStepDistanceTooltip => {
            "Horisontellt avstånd mellan flyttprober för statiska gränser. Config key: relocation_step."
        }
        UiText::RelocationProbeRings => "Ringar för flyttprober",
        UiText::RelocationProbeRingsTooltip => {
            "Antal ringar med flyttprober att prova för statiska flyttar. Config key: relocation_steps."
        }
        UiText::RegexFiltersHelp => {
            "Regexfilter använder skiftlägesokänsliga full-ID-regexar. Exkludera vinner över inkludera. Tom inkludera betyder alla."
        }
        UiText::IncludeGrassIds => "Inkludera Grass ID:n",
        UiText::NoIncludeGrassIds => "Inga inkluderande Grass ID-filter konfigurerade.",
        UiText::ExcludeGrassIds => "Exkludera Grass ID:n",
        UiText::NoExcludeGrassIds => "Inga exkluderande Grass ID-filter konfigurerade.",
        UiText::IncludeOccluderIds => "Inkludera ockluderar-ID:n",
        UiText::NoIncludeOccluderIds => "Inga inkluderande ockluderar-ID-filter konfigurerade.",
        UiText::ExcludeOccluderIds => "Exkludera ockluderar-ID:n",
        UiText::NoExcludeOccluderIds => "Inga exkluderande ockluderar-ID-filter konfigurerade.",
        UiText::AddGrassIdPatternTitle => "Lägg till Grass ID-mönster",
        UiText::AddExcludePatternTitle => "Lägg till exkluderingsmönster",
        UiText::AddIgnoredPluginTitle => "Lägg till ignorerat plugin",
        UiText::AddIncludeGrassIdRegexTitle => "Lägg till inkluderande Grass ID-regex",
        UiText::AddExcludeGrassIdRegexTitle => "Lägg till exkluderande Grass ID-regex",
        UiText::AddIncludeOccluderIdRegexTitle => "Lägg till inkluderande ockluderar-ID-regex",
        UiText::AddExcludeOccluderIdRegexTitle => "Lägg till exkluderande ockluderar-ID-regex",
        UiText::IgnoredPluginPrompt => "Ignorerat plugin",
        UiText::IncludeGrassIdRegexPrompt => "Inkluderande Grass ID-regex",
        UiText::ExcludeGrassIdRegexPrompt => "Exkluderande Grass ID-regex",
        UiText::IncludeOccluderIdRegexPrompt => "Inkluderande ockluderar-ID-regex",
        UiText::ExcludeOccluderIdRegexPrompt => "Exkluderande ockluderar-ID-regex",
        _ => unreachable!(),
    }
}
