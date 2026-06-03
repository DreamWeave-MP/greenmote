// SPDX-License-Identifier: GPL-3.0-only

use super::UiText;

pub(super) const fn text(key: UiText) -> &'static str {
    route_text!(key, language_text, convert_text, dialog_text, settings_text)
}

pub(super) fn unclip_target_count(count: usize) -> String {
    if count == 1 {
        "Objetivo: 1 plugin".to_owned()
    } else {
        format!("Objetivos: {count} plugins")
    }
}

pub(super) fn unclip_target_overflow(count: usize) -> String {
    format!("... y {count} más")
}

const fn language_text(key: UiText) -> &'static str {
    match key {
        UiText::Language => "Idioma",
        UiText::EnglishLanguage => "Inglés",
        UiText::FrenchLanguage => "Francés",
        UiText::GermanLanguage => "Alemán",
        UiText::RussianLanguage => "Ruso",
        UiText::SpanishLanguage => "Español",
        UiText::SwedishLanguage => "Sueco",
        UiText::Convert => "Convertir",
        UiText::Unclip => "Unclip",
        UiText::Settings => "Ajustes",
        UiText::General => "General",
        UiText::Save => "Guardar",
        UiText::Cancel => "Cancelar",
        UiText::Add => "Añadir",
        UiText::Discard => "Descartar",
        UiText::Close => "Cerrar",
        UiText::Up => "Arriba",
        UiText::Down => "Abajo",
        UiText::ShowPreviousItems => "Mostrar elementos anteriores",
        UiText::ShowNextItems => "Mostrar elementos siguientes",
        _ => unreachable!(),
    }
}

const fn convert_text(key: UiText) -> &'static str {
    match key {
        UiText::RunOptions => "Opciones de ejecución",
        UiText::TargetPlugins => "Plugins objetivo",
        UiText::AddFiles => "Añadir archivos...",
        UiText::AddTargetPath => "Añadir objetivo/ruta",
        UiText::SetUnclipOutputPlugin => "Definir salida...",
        UiText::ClearUnclipOutputPlugin => "Borrar plugin de salida",
        UiText::RemoveSelectedTarget => "Quitar objetivo seleccionado",
        UiText::ClearTargets => "Borrar objetivos",
        UiText::EmptyTargetList => "No hay plugins objetivo añadidos.",
        UiText::TargetPathEntry => "Nombre o ruta del plugin",
        UiText::StartConversion => "Iniciar conversión",
        UiText::WriteChanges => "Escribir cambios",
        UiText::InspectPlugin => "Inspeccionar plugin",
        UiText::DryRun => "Simulación",
        UiText::DebugDiagnostics => "Diagnósticos de depuración",
        UiText::AutoEnableGeneratedPlugins => "Activar automáticamente plugins generados",
        UiText::SaveAsDefaults => "Guardar como predeterminado",
        UiText::ResetFromSaved => "Restaurar desde guardado",
        UiText::RunOptionsDiffer => "Las opciones de ejecución difieren de los valores guardados.",
        UiText::RunOptionsMatch => "Las opciones de ejecución coinciden con los valores guardados.",
        UiText::SaveOrDiscardSettingsBeforeDefaults => {
            "Guarda o descarta los cambios de Ajustes antes de guardarlos como predeterminados."
        }
        UiText::ClearOutput => "Borrar salida",
        UiText::CopyOutput => "Copiar salida",
        UiText::OpenOutputDir => "Abrir carpeta de salida",
        UiText::OpenLog => "Abrir registro",
        UiText::WritingUnclipBatch => "Escribiendo lote de Unclip...",
        UiText::InspectingUnclipBatch => "Inspeccionando lote de Unclip...",
        UiText::UnclipWrite => "Escritura de Unclip",
        UiText::UnclipInspection => "Inspección de Unclip",
        UiText::UnclipTargetPending => "Pendiente",
        UiText::UnclipTargetRunning => "En curso",
        UiText::UnclipTargetSucceeded => "Correcto",
        UiText::UnclipTargetFailed => "Fallido",
        UiText::UnclipTargetSkipped => "Omitido",
        UiText::UnclipTargetCancelled => "Cancelado",
        _ => unreachable!(),
    }
}

const fn dialog_text(key: UiText) -> &'static str {
    match key {
        UiText::ConfirmUnclipWriteTitle => "Confirmar escritura de Unclip",
        UiText::ConfirmUnclipWriteMessage => {
            "Unclip modificará los plugins objetivo seleccionados y creará archivos de copia de seguridad."
        }
        UiText::EnabledWriteActions => "Acciones de escritura activadas:",
        UiText::UnsavedSettingsTitle => "Ajustes sin guardar",
        UiText::UnsavedSettingsMessage => "Los ajustes tienen cambios sin guardar.",
        UiText::SaveBeforeContinuing => "¿Guardarlos antes de continuar?",
        UiText::MalformedConfigTitle => "Configuración mal formada",
        UiText::MalformedConfigMessage => "No se pudo cargar greenmote.toml.",
        UiText::ReplaceWithDefaults => {
            "Reemplázalo por los valores predeterminados antes de continuar."
        }
        UiText::BackupBeforeReplacing => "El archivo actual se moverá primero a un archivo .bak.",
        UiText::BackupRegenerateContinue => "Respaldar, regenerar y continuar",
        UiText::OpenMwConfigNotFoundTitle => "Configuración de OpenMW no encontrada",
        UiText::OpenMwConfigNotFoundMessage => {
            "Greenmote no pudo encontrar o cargar un archivo de configuración de OpenMW."
        }
        UiText::ChooseOpenMwConfigBeforeContinuing => {
            "Elige una ruta válida de configuración de OpenMW antes de continuar."
        }
        UiText::SelectOpenMwConfig => "Seleccionar configuración de OpenMW",
        UiText::SelectUnclipTargetPlugins => "Seleccionar plugins objetivo de Unclip",
        UiText::SelectUnclipOutputPlugin => "Seleccionar plugin de salida de Unclip",
        _ => unreachable!(),
    }
}

#[allow(clippy::too_many_lines)]
const fn settings_text(key: UiText) -> &'static str {
    match key {
        UiText::OpenMwConfig => "Configuración de OpenMW",
        UiText::OpenMwPlugins => "Plugins de OpenMW",
        UiText::UsingOpenMwAutodetection => "Usando autodetección de OpenMW.",
        UiText::OpenMwConfigCannotChangeWhileRunning => {
            "La configuración de OpenMW no se puede cambiar mientras hay una ejecución activa."
        }
        UiText::ConvertOutputDirectory => "Carpeta de salida de conversión",
        UiText::OutputFromDataLocal => {
            "Desde el ajuste data-local de la configuración de OpenMW seleccionada."
        }
        UiText::OutputCliOverride => "Sobrescrito para esta ejecución de conversión.",
        UiText::OutputWorkingDirectoryFallback => {
            "OpenMW no tiene ajuste data-local, así que Greenmote escribirá en la carpeta de trabajo actual mostrada arriba. OpenMW solo puede cargar los archivos generados si esta carpeta está configurada como data-local o data=."
        }
        UiText::GrassIdPatterns => "Patrones de Grass ID",
        UiText::NoGrassIdPatterns => "No hay patrones de Grass ID configurados.",
        UiText::ExcludePatterns => "Patrones de exclusión",
        UiText::NoExcludePatterns => "No hay patrones de exclusión configurados.",
        UiText::IgnoredPlugins => "Plugins ignorados",
        UiText::NoIgnoredPlugins => "No hay plugins ignorados configurados.",
        UiText::RunOptionsConfiguredOnConvert => {
            "Las opciones de ejecución se configuran en la pantalla Convertir."
        }
        UiText::WriteActions => "Acciones de escritura",
        UiText::TerrainZAction => "Terrain Z (terrain-z)",
        UiText::WaterDeleteAction => "Eliminar refs que crucen el agua exterior (water-delete)",
        UiText::RoadDeleteAction => "Eliminar refs sobre texturas de camino (road-delete)",
        UiText::StaticDeleteAction => {
            "Eliminar refs totalmente ocluidas por statics (static-delete)"
        }
        UiText::StaticMoveAction => "Mover refs ocluidas por statics (static-move)",
        UiText::OrientAction => "Orientar refs al terrain (orient)",
        UiText::NoWriteActionsWarning => {
            "Advertencia: el modo escritura no producirá acciones de política."
        }
        UiText::PolicyNumbers => "Valores de política",
        UiText::OriginHeightTolerance => "Tolerancia de altura de origen",
        UiText::OriginHeightToleranceTooltip => {
            "Delta máximo entre origen de referencia y terrain Z tratado como ya sobre el terrain. Config key: origin_epsilon."
        }
        UiText::OrientationTolerance => "Tolerancia de orientación",
        UiText::OrientationToleranceTooltip => {
            "Ángulo máximo de inclinación en grados tratado como ya alineado al terrain. Config key: orientation_epsilon."
        }
        UiText::RelocationStepDistance => "Distancia de paso de reubicación",
        UiText::RelocationStepDistanceTooltip => {
            "Distancia horizontal entre sondas de reubicación para límites static. Config key: relocation_step."
        }
        UiText::RelocationProbeRings => "Anillos de sondas de reubicación",
        UiText::RelocationProbeRingsTooltip => {
            "Número de anillos de sondas de reubicación a probar para movimientos static. Config key: relocation_steps."
        }
        UiText::RegexFiltersHelp => {
            "Los filtros regex usan regex de ID completo sin distinguir mayúsculas. Excluir gana sobre incluir. Incluir vacío significa todos."
        }
        UiText::IncludeGrassIds => "Incluir Grass ID",
        UiText::NoIncludeGrassIds => "No hay filtros de inclusión de Grass ID configurados.",
        UiText::ExcludeGrassIds => "Excluir Grass ID",
        UiText::NoExcludeGrassIds => "No hay filtros de exclusión de Grass ID configurados.",
        UiText::IncludeOccluderIds => "Incluir ID de occluders",
        UiText::NoIncludeOccluderIds => {
            "No hay filtros de inclusión de ID de occluders configurados."
        }
        UiText::ExcludeOccluderIds => "Excluir ID de occluders",
        UiText::NoExcludeOccluderIds => {
            "No hay filtros de exclusión de ID de occluders configurados."
        }
        UiText::IncludeRoadTexturePaths => "Incluir rutas de texturas de camino",
        UiText::NoIncludeRoadTexturePaths => {
            "No hay filtros adicionales de rutas de texturas de camino configurados."
        }
        UiText::ExcludeRoadTexturePaths => "Excluir rutas de texturas de camino",
        UiText::NoExcludeRoadTexturePaths => {
            "No hay exclusiones de rutas de texturas de camino configuradas."
        }
        UiText::AddGrassIdPatternTitle => "Añadir patrón de Grass ID",
        UiText::AddExcludePatternTitle => "Añadir patrón de exclusión",
        UiText::AddIgnoredPluginTitle => "Añadir plugin ignorado",
        UiText::AddIncludeGrassIdRegexTitle => "Añadir regex de inclusión de Grass ID",
        UiText::AddExcludeGrassIdRegexTitle => "Añadir regex de exclusión de Grass ID",
        UiText::AddIncludeOccluderIdRegexTitle => "Añadir regex de inclusión de ID de occluders",
        UiText::AddExcludeOccluderIdRegexTitle => "Añadir regex de exclusión de ID de occluders",
        UiText::AddIncludeRoadTexturePathRegexTitle => {
            "Añadir regex de inclusión de ruta de textura de camino"
        }
        UiText::AddExcludeRoadTexturePathRegexTitle => {
            "Añadir regex de exclusión de ruta de textura de camino"
        }
        UiText::GrassIdPatternPrompt => "Patrón de Grass ID",
        UiText::ExcludePatternPrompt => "Patrón de exclusión",
        UiText::IgnoredPluginPrompt => "Plugin ignorado",
        UiText::IncludeGrassIdRegexPrompt => "Regex de inclusión de Grass ID",
        UiText::ExcludeGrassIdRegexPrompt => "Regex de exclusión de Grass ID",
        UiText::IncludeOccluderIdRegexPrompt => "Regex de inclusión de ID de occluders",
        UiText::ExcludeOccluderIdRegexPrompt => "Regex de exclusión de ID de occluders",
        UiText::IncludeRoadTexturePathRegexPrompt => {
            "Regex de inclusión de ruta de textura de camino"
        }
        UiText::ExcludeRoadTexturePathRegexPrompt => {
            "Regex de exclusión de ruta de textura de camino"
        }
        _ => unreachable!(),
    }
}
