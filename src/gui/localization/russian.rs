use super::UiText;

pub(super) const fn text(key: UiText) -> &'static str {
    route_text!(key, language_text, convert_text, dialog_text, settings_text)
}

const fn language_text(key: UiText) -> &'static str {
    match key {
        UiText::Language => "Язык",
        UiText::EnglishLanguage => "Английский",
        UiText::FrenchLanguage => "Французский",
        UiText::GermanLanguage => "Немецкий",
        UiText::RussianLanguage => "Русский",
        UiText::SpanishLanguage => "Испанский",
        UiText::SwedishLanguage => "Шведский",
        UiText::Convert => "Конвертация",
        UiText::Unclip => "Unclip",
        UiText::Settings => "Настройки",
        UiText::General => "Общие",
        UiText::Browse => "Обзор...",
        UiText::Save => "Сохранить",
        UiText::Back => "Назад",
        UiText::Cancel => "Отмена",
        UiText::Add => "Добавить",
        UiText::Discard => "Сбросить",
        UiText::Close => "Закрыть",
        UiText::Up => "Вверх",
        UiText::Down => "Вниз",
        UiText::ShowPreviousItems => "Показать предыдущие элементы",
        UiText::ShowNextItems => "Показать следующие элементы",
        _ => unreachable!(),
    }
}

const fn convert_text(key: UiText) -> &'static str {
    match key {
        UiText::RunOptions => "Параметры запуска",
        UiText::TargetPlugin => "Целевой plugin",
        UiText::WriteChangesToPlugin => "Записать изменения в plugin",
        UiText::DetailedRefDiagnostics => "Показать подробную диагностику refs",
        UiText::DetailedRefDiagnosticsTooltip => {
            "Выводить отдельную запись диагностики для каждой проверенной размещенной ссылки вместо одной сводки. Включает ключ cell/ref, контакт с terrain, статическое перекрытие и состояние записи."
        }
        UiText::StartConversion => "Начать конвертацию",
        UiText::WriteChanges => "Записать изменения",
        UiText::InspectPlugin => "Проверить plugin",
        UiText::DryRun => "Пробный запуск",
        UiText::DebugDiagnostics => "Отладочная диагностика",
        UiText::AutoEnableGeneratedPlugins => "Автоматически включать созданные plugins",
        UiText::SaveAsDefaults => "Сохранить по умолчанию",
        UiText::ResetFromSaved => "Сбросить к сохраненному",
        UiText::RunOptionsDiffer => {
            "Параметры запуска отличаются от сохраненных значений по умолчанию."
        }
        UiText::RunOptionsMatch => {
            "Параметры запуска совпадают с сохраненными значениями по умолчанию."
        }
        UiText::SaveOrDiscardSettingsBeforeDefaults => {
            "Сохраните или отмените изменения в Настройках перед сохранением этих значений по умолчанию."
        }
        UiText::ClearOutput => "Очистить вывод",
        UiText::CopyOutput => "Копировать вывод",
        UiText::OpenOutputDir => "Открыть папку вывода",
        UiText::OpenLog => "Открыть лог",
        _ => unreachable!(),
    }
}

const fn dialog_text(key: UiText) -> &'static str {
    match key {
        UiText::ConfirmUnclipWriteTitle => "Подтвердить запись Unclip",
        UiText::ConfirmUnclipWriteMessage => {
            "Unclip изменит целевой plugin и создаст резервную копию."
        }
        UiText::Target => "Цель:",
        UiText::EnabledWriteActions => "Включенные действия записи:",
        UiText::UnsavedSettingsTitle => "Несохраненные настройки",
        UiText::UnsavedSettingsMessage => "В настройках есть несохраненные изменения.",
        UiText::SaveBeforeContinuing => "Сохранить их перед продолжением?",
        UiText::MalformedConfigTitle => "Поврежденная конфигурация",
        UiText::MalformedConfigMessage => "Не удалось загрузить greenmote.toml.",
        UiText::ReplaceWithDefaults => "Замените его значениями по умолчанию перед продолжением.",
        UiText::BackupBeforeReplacing => "Текущий файл сначала будет перемещен в файл .bak.",
        UiText::BackupRegenerateContinue => "Создать копию, пересоздать и продолжить",
        UiText::OpenMwConfigNotFoundTitle => "Конфигурация OpenMW не найдена",
        UiText::OpenMwConfigNotFoundMessage => {
            "Greenmote не смог найти или загрузить файл конфигурации OpenMW."
        }
        UiText::ChooseOpenMwConfigBeforeContinuing => {
            "Выберите допустимый путь к конфигурации OpenMW перед продолжением."
        }
        UiText::SelectOpenMwConfig => "Выбрать конфигурацию OpenMW",
        UiText::SelectUnclipTargetPlugin => "Выбрать целевой plugin Unclip",
        _ => unreachable!(),
    }
}

const fn settings_text(key: UiText) -> &'static str {
    match key {
        UiText::OpenMwConfig => "Конфигурация OpenMW",
        UiText::OpenMwPlugins => "Plugins OpenMW",
        UiText::UsingOpenMwAutodetection => "Используется автообнаружение OpenMW.",
        UiText::OpenMwConfigCannotChangeWhileRunning => {
            "Конфигурацию OpenMW нельзя менять во время выполнения."
        }
        UiText::ConvertOutputDirectory => "Папка вывода конвертации",
        UiText::OutputFromDataLocal => "Из параметра data-local выбранной конфигурации OpenMW.",
        UiText::OutputCliOverride => "Переопределено для этого запуска конвертации.",
        UiText::OutputWorkingDirectoryFallback => {
            "В OpenMW нет параметра data-local, поэтому Greenmote будет писать в текущую рабочую папку, показанную выше. OpenMW сможет загрузить созданные файлы, только если эта папка настроена как data-local или data=."
        }
        UiText::GrassIdPatterns => "Шаблоны Grass ID",
        UiText::NoGrassIdPatterns => "Шаблоны Grass ID не настроены.",
        UiText::ExcludePatterns => "Шаблоны исключения",
        UiText::NoExcludePatterns => "Шаблоны исключения не настроены.",
        UiText::IgnoredPlugins => "Игнорируемые plugins",
        UiText::NoIgnoredPlugins => "Игнорируемые plugins не настроены.",
        UiText::RunOptionsConfiguredOnConvert => {
            "Параметры запуска настраиваются на экране Конвертация."
        }
        UiText::WriteActions => "Действия записи",
        UiText::TerrainZAction => "Terrain Z (terrain-z)",
        UiText::StaticDeleteAction => "Удалять refs, полностью перекрытые statics (static-delete)",
        UiText::StaticMoveAction => "Перемещать refs, перекрытые statics (static-move)",
        UiText::OrientAction => "Ориентировать refs по terrain (orient)",
        UiText::NoWriteActionsWarning => {
            "Предупреждение: режим записи не создаст действий политики."
        }
        UiText::PolicyNumbers => "Значения политики",
        UiText::MeshContactTolerance => "Допуск контакта mesh",
        UiText::MeshContactToleranceTooltip => {
            "Максимальная дельта контакта mesh/terrain Z, считающаяся уже на terrain. Config key: contact_epsilon."
        }
        UiText::OriginHeightTolerance => "Допуск высоты origin",
        UiText::OriginHeightToleranceTooltip => {
            "Максимальная дельта origin ссылки/terrain Z, считающаяся уже на terrain. Config key: origin_epsilon."
        }
        UiText::OrientationTolerance => "Допуск ориентации",
        UiText::OrientationToleranceTooltip => {
            "Максимальный угол наклона в градусах, считающийся уже выровненным по terrain. Config key: orientation_epsilon."
        }
        UiText::RelocationStepDistance => "Дистанция шага перемещения",
        UiText::RelocationStepDistanceTooltip => {
            "Горизонтальное расстояние между пробами перемещения для static bounds. Config key: relocation_step."
        }
        UiText::RelocationProbeRings => "Кольца проб перемещения",
        UiText::RelocationProbeRingsTooltip => {
            "Количество колец проб перемещения для static moves. Config key: relocation_steps."
        }
        UiText::RegexFiltersHelp => {
            "Regex-фильтры используют нечувствительные к регистру regex полных ID. Исключение сильнее включения. Пустое включение означает все."
        }
        UiText::IncludeGrassIds => "Включать Grass ID",
        UiText::NoIncludeGrassIds => "Фильтры включения Grass ID не настроены.",
        UiText::ExcludeGrassIds => "Исключать Grass ID",
        UiText::NoExcludeGrassIds => "Фильтры исключения Grass ID не настроены.",
        UiText::IncludeOccluderIds => "Включать ID occluders",
        UiText::NoIncludeOccluderIds => "Фильтры включения ID occluders не настроены.",
        UiText::ExcludeOccluderIds => "Исключать ID occluders",
        UiText::NoExcludeOccluderIds => "Фильтры исключения ID occluders не настроены.",
        UiText::AddGrassIdPatternTitle => "Добавить шаблон Grass ID",
        UiText::AddExcludePatternTitle => "Добавить шаблон исключения",
        UiText::AddIgnoredPluginTitle => "Добавить игнорируемый plugin",
        UiText::AddIncludeGrassIdRegexTitle => "Добавить regex включения Grass ID",
        UiText::AddExcludeGrassIdRegexTitle => "Добавить regex исключения Grass ID",
        UiText::AddIncludeOccluderIdRegexTitle => "Добавить regex включения ID occluders",
        UiText::AddExcludeOccluderIdRegexTitle => "Добавить regex исключения ID occluders",
        UiText::GrassIdPatternPrompt => "Шаблон Grass ID",
        UiText::ExcludePatternPrompt => "Шаблон исключения",
        UiText::IgnoredPluginPrompt => "Игнорируемый plugin",
        UiText::IncludeGrassIdRegexPrompt => "Regex включения Grass ID",
        UiText::ExcludeGrassIdRegexPrompt => "Regex исключения Grass ID",
        UiText::IncludeOccluderIdRegexPrompt => "Regex включения ID occluders",
        UiText::ExcludeOccluderIdRegexPrompt => "Regex исключения ID occluders",
        _ => unreachable!(),
    }
}
