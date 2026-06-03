// SPDX-License-Identifier: GPL-3.0-only

use super::UiText;

pub(super) const fn text(key: UiText) -> &'static str {
    route_text!(key, language_text, convert_text, dialog_text, settings_text)
}

pub(super) fn unclip_target_count(count: usize) -> String {
    if count == 1 {
        "Cible : 1 plugin".to_owned()
    } else {
        format!("Cibles : {count} plugins")
    }
}

pub(super) fn unclip_target_overflow(count: usize) -> String {
    if count == 1 {
        "... et 1 autre".to_owned()
    } else {
        format!("... et {count} autres")
    }
}

const fn language_text(key: UiText) -> &'static str {
    match key {
        UiText::Language => "Langue",
        UiText::EnglishLanguage => "Anglais",
        UiText::FrenchLanguage => "Français",
        UiText::GermanLanguage => "Allemand",
        UiText::RussianLanguage => "Russe",
        UiText::SpanishLanguage => "Espagnol",
        UiText::SwedishLanguage => "Suédois",
        UiText::Convert => "Convertir",
        UiText::Unclip => "Unclip",
        UiText::Settings => "Paramètres",
        UiText::General => "Général",
        UiText::Save => "Enregistrer",
        UiText::Cancel => "Annuler",
        UiText::Add => "Ajouter",
        UiText::Discard => "Ignorer",
        UiText::Close => "Fermer",
        UiText::Up => "Haut",
        UiText::Down => "Bas",
        UiText::ShowPreviousItems => "Afficher les éléments précédents",
        UiText::ShowNextItems => "Afficher les éléments suivants",
        _ => unreachable!(),
    }
}

const fn convert_text(key: UiText) -> &'static str {
    match key {
        UiText::RunOptions => "Options d’exécution",
        UiText::TargetPlugins => "Plugins cibles",
        UiText::AddFiles => "Ajouter des fichiers...",
        UiText::AddTargetPath => "Ajouter cible/chemin",
        UiText::SetUnclipOutputPlugin => "Définir la sortie...",
        UiText::ClearUnclipOutputPlugin => "Effacer le plugin de sortie",
        UiText::RemoveSelectedTarget => "Retirer la cible sélectionnée",
        UiText::ClearTargets => "Effacer les cibles",
        UiText::EmptyTargetList => "Aucun plugin cible ajouté.",
        UiText::TargetPathEntry => "Nom ou chemin du plugin",
        UiText::WriteChangesToPlugin => "Écrire les changements dans le plugin",
        UiText::StartConversion => "Démarrer la conversion",
        UiText::WriteChanges => "Écrire les changements",
        UiText::InspectPlugin => "Inspecter le plugin",
        UiText::DryRun => "Simulation",
        UiText::DebugDiagnostics => "Diagnostics de débogage",
        UiText::AutoEnableGeneratedPlugins => "Activer automatiquement les plugins générés",
        UiText::SaveAsDefaults => "Enregistrer par défaut",
        UiText::ResetFromSaved => "Rétablir depuis l’enregistrement",
        UiText::RunOptionsDiffer => "Les options d’exécution diffèrent des valeurs enregistrées.",
        UiText::RunOptionsMatch => {
            "Les options d’exécution correspondent aux valeurs enregistrées."
        }
        UiText::SaveOrDiscardSettingsBeforeDefaults => {
            "Enregistrez ou ignorez les changements des Paramètres avant de les enregistrer par défaut."
        }
        UiText::ClearOutput => "Effacer la sortie",
        UiText::CopyOutput => "Copier la sortie",
        UiText::OpenOutputDir => "Ouvrir le dossier de sortie",
        UiText::OpenLog => "Ouvrir le journal",
        UiText::WritingUnclipBatch => "Écriture du lot Unclip...",
        UiText::InspectingUnclipBatch => "Inspection du lot Unclip...",
        UiText::UnclipWrite => "Écriture Unclip",
        UiText::UnclipInspection => "Inspection Unclip",
        UiText::UnclipTargetPending => "En attente",
        UiText::UnclipTargetRunning => "En cours",
        UiText::UnclipTargetSucceeded => "Réussi",
        UiText::UnclipTargetFailed => "Échoué",
        UiText::UnclipTargetSkipped => "Ignoré",
        UiText::UnclipTargetCancelled => "Annulé",
        _ => unreachable!(),
    }
}

const fn dialog_text(key: UiText) -> &'static str {
    match key {
        UiText::ConfirmUnclipWriteTitle => "Confirmer l’écriture Unclip",
        UiText::ConfirmUnclipWriteMessage => {
            "Unclip va modifier les plugins cibles sélectionnés et créer des fichiers de sauvegarde."
        }
        UiText::EnabledWriteActions => "Actions d’écriture activées :",
        UiText::UnsavedSettingsTitle => "Paramètres non enregistrés",
        UiText::UnsavedSettingsMessage => {
            "Les paramètres comportent des changements non enregistrés."
        }
        UiText::SaveBeforeContinuing => "Les enregistrer avant de continuer ?",
        UiText::MalformedConfigTitle => "Configuration mal formée",
        UiText::MalformedConfigMessage => "greenmote.toml n’a pas pu être chargé.",
        UiText::ReplaceWithDefaults => {
            "Remplacez-le par les valeurs par défaut avant de continuer."
        }
        UiText::BackupBeforeReplacing => "Le fichier actuel sera d’abord déplacé en fichier .bak.",
        UiText::BackupRegenerateContinue => "Sauvegarder, régénérer et continuer",
        UiText::OpenMwConfigNotFoundTitle => "Configuration OpenMW introuvable",
        UiText::OpenMwConfigNotFoundMessage => {
            "Greenmote n’a pas pu trouver ou charger un fichier de configuration OpenMW."
        }
        UiText::ChooseOpenMwConfigBeforeContinuing => {
            "Choisissez un chemin de configuration OpenMW valide avant de continuer."
        }
        UiText::SelectOpenMwConfig => "Sélectionner la configuration OpenMW",
        UiText::SelectUnclipTargetPlugins => "Sélectionner les plugins cibles Unclip",
        UiText::SelectUnclipOutputPlugin => "Sélectionner le plugin de sortie Unclip",
        _ => unreachable!(),
    }
}

const fn settings_text(key: UiText) -> &'static str {
    match key {
        UiText::OpenMwConfig => "Configuration OpenMW",
        UiText::OpenMwPlugins => "Plugins OpenMW",
        UiText::UsingOpenMwAutodetection => "Utilisation de la détection automatique d’OpenMW.",
        UiText::OpenMwConfigCannotChangeWhileRunning => {
            "La configuration OpenMW ne peut pas être modifiée pendant une exécution."
        }
        UiText::ConvertOutputDirectory => "Dossier de sortie de conversion",
        UiText::OutputFromDataLocal => {
            "Depuis le paramètre data-local de la configuration OpenMW sélectionnée."
        }
        UiText::OutputCliOverride => "Remplacé pour cette exécution de conversion.",
        UiText::OutputWorkingDirectoryFallback => {
            "OpenMW n’a pas de paramètre data-local, donc Greenmote écrira dans le dossier de travail actuel affiché ci-dessus. OpenMW ne peut charger les fichiers générés que si ce dossier est configuré comme data-local ou data=."
        }
        UiText::GrassIdPatterns => "Motifs d’ID d’herbe",
        UiText::NoGrassIdPatterns => "Aucun motif d’ID d’herbe configuré.",
        UiText::ExcludePatterns => "Motifs d’exclusion",
        UiText::NoExcludePatterns => "Aucun motif d’exclusion configuré.",
        UiText::IgnoredPlugins => "Plugins ignorés",
        UiText::NoIgnoredPlugins => "Aucun plugin ignoré configuré.",
        UiText::RunOptionsConfiguredOnConvert => {
            "Les options d’exécution se configurent sur l’écran Convertir."
        }
        UiText::WriteActions => "Actions d’écriture",
        UiText::TerrainZAction => "Terrain Z (terrain-z)",
        UiText::StaticDeleteAction => {
            "Supprimer les refs entièrement occultées par des statics (static-delete)"
        }
        UiText::StaticMoveAction => "Déplacer les refs occultées par des statics (static-move)",
        UiText::OrientAction => "Orienter les refs vers le terrain (orient)",
        UiText::NoWriteActionsWarning => {
            "Avertissement : le mode écriture ne produira aucune action de politique."
        }
        UiText::PolicyNumbers => "Valeurs de politique",
        UiText::OriginHeightTolerance => "Tolérance de hauteur d’origine",
        UiText::OriginHeightToleranceTooltip => {
            "Delta maximal origine de référence/terrain Z traité comme déjà sur le terrain. Config key: origin_epsilon."
        }
        UiText::OrientationTolerance => "Tolérance d’orientation",
        UiText::OrientationToleranceTooltip => {
            "Angle d’inclinaison maximal en degrés traité comme déjà aligné au terrain. Config key: orientation_epsilon."
        }
        UiText::RelocationStepDistance => "Distance de pas de déplacement",
        UiText::RelocationStepDistanceTooltip => {
            "Distance horizontale entre les sondes de déplacement des limites statiques. Config key: relocation_step."
        }
        UiText::RelocationProbeRings => "Anneaux de sondes de déplacement",
        UiText::RelocationProbeRingsTooltip => {
            "Nombre d’anneaux de sondes à essayer pour les déplacements statiques. Config key: relocation_steps."
        }
        UiText::RegexFiltersHelp => {
            "Les filtres regex utilisent des regex d’ID complet insensibles à la casse. Exclure l’emporte sur inclure. Inclure vide signifie tous."
        }
        UiText::IncludeGrassIds => "Inclure les ID d’herbe",
        UiText::NoIncludeGrassIds => "Aucun filtre d’inclusion d’ID d’herbe configuré.",
        UiText::ExcludeGrassIds => "Exclure les ID d’herbe",
        UiText::NoExcludeGrassIds => "Aucun filtre d’exclusion d’ID d’herbe configuré.",
        UiText::IncludeOccluderIds => "Inclure les ID d’occulteurs",
        UiText::NoIncludeOccluderIds => "Aucun filtre d’inclusion d’ID d’occulteurs configuré.",
        UiText::ExcludeOccluderIds => "Exclure les ID d’occulteurs",
        UiText::NoExcludeOccluderIds => "Aucun filtre d’exclusion d’ID d’occulteurs configuré.",
        UiText::AddGrassIdPatternTitle => "Ajouter un motif d’ID d’herbe",
        UiText::AddExcludePatternTitle => "Ajouter un motif d’exclusion",
        UiText::AddIgnoredPluginTitle => "Ajouter un plugin ignoré",
        UiText::AddIncludeGrassIdRegexTitle => "Ajouter une regex d’inclusion d’ID d’herbe",
        UiText::AddExcludeGrassIdRegexTitle => "Ajouter une regex d’exclusion d’ID d’herbe",
        UiText::AddIncludeOccluderIdRegexTitle => "Ajouter une regex d’inclusion d’ID d’occulteurs",
        UiText::AddExcludeOccluderIdRegexTitle => "Ajouter une regex d’exclusion d’ID d’occulteurs",
        UiText::GrassIdPatternPrompt => "Motif d’ID d’herbe",
        UiText::ExcludePatternPrompt => "Motif d’exclusion",
        UiText::IgnoredPluginPrompt => "Plugin ignoré",
        UiText::IncludeGrassIdRegexPrompt => "Regex d’inclusion d’ID d’herbe",
        UiText::ExcludeGrassIdRegexPrompt => "Regex d’exclusion d’ID d’herbe",
        UiText::IncludeOccluderIdRegexPrompt => "Regex d’inclusion d’ID d’occulteurs",
        UiText::ExcludeOccluderIdRegexPrompt => "Regex d’exclusion d’ID d’occulteurs",
        _ => unreachable!(),
    }
}
