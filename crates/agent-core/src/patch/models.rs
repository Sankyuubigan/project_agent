//! Модели данных патчера «Умный Markdown».

/// Одна точечная замена внутри блока файла (SEARCH/REPLACE).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    /// Точный фрагмент, который ищется в текущем файле.
    pub search: String,
    /// Фрагмент на его место.
    pub replace: String,
}

/// Тип изменения одного файла.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeKind {
    /// Полная замена содержимого (или создание файла, если его нет).
    Whole { content: String },
    /// Точечные замены SEARCH/REPLACE (только для существующих файлов).
    Edits { edits: Vec<Edit> },
    /// Удаление файла — только явным тегом `<<<DELETE_FILE: путь>>>`.
    Delete,
}

/// Изменение одного файла: путь относительный, нормализованный (`/`, без `..`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub path: String,
    pub kind: ChangeKind,
}

/// Структурированная ошибка патча. Молчаливых потерь нет (вердикт спеца №4):
/// каждая проблема попадает в список с путём и причиной.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchError {
    /// Путь из тега, если ошибка привязана к блоку файла.
    pub path: Option<String>,
    /// Человекочитаемая причина (для UI и лога).
    pub reason: String,
}

impl PatchError {
    pub fn new(path: Option<&str>, reason: impl Into<String>) -> Self {
        Self {
            path: path.map(str::to_string),
            reason: reason.into(),
        }
    }
}

/// Результат разбора ответа LLM.
#[derive(Debug, Clone, Default)]
pub struct ParsedResponse {
    /// Успешно распознанные изменения (порядок как в ответе).
    pub changes: Vec<FileChange>,
    /// Ошибки разбора (блоки, которые нельзя применить).
    pub errors: Vec<PatchError>,
    /// Предупреждения (голый END_FILE, пустой блок и т.п.).
    pub warnings: Vec<String>,
}

/// Результат dry-run подготовки одного файла (в памяти, без записи).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedChange {
    pub path: String,
    pub kind: PreparedKind,
    /// Предупреждения подготовки (например «файл будет пустым»).
    pub warnings: Vec<String>,
}

/// Действие над файлом на диске после успешного dry-run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreparedKind {
    /// Записать содержимое (атомарно, UTF-8, LF).
    Write { content: String },
    /// Удалить файл.
    Delete,
}

/// Ошибка подготовки/применения по конкретному файлу.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePatchError {
    pub path: String,
    pub reason: String,
}

/// Отчёт применения (фаза 2, запись на диск).
#[derive(Debug, Clone, Default)]
pub struct ApplyReport {
    pub applied: Vec<String>,
    pub failed: Vec<FilePatchError>,
    /// Относительные пути файлов, попавших в бэкап (для Undo).
    pub backed_up: Vec<String>,
}
