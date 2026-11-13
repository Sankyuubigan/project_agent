//! Сканер дерева файлов (перенос правил `fs_scanner_utils.py`).
//!
//! Обход — крейт `ignore` (автор ripgrep): каскадные .gitignore из коробки.
//! Поверх — глобальные исключения старой версии и статусы файлов.

use std::path::Path;

use ignore::WalkBuilder;

/// Папки, игнорируемые всегда (как в старой версии).
pub const GLOBAL_IGNORED_DIRS: &[&str] = &[
    ".git", "__pycache__", ".vscode", ".idea", "node_modules", "venv", ".env", "build", "dist",
    "out", "target", ".pytest_cache", ".mypy_cache", ".tox", ".conda", ".condaenv", "env",
    ".zcode", "tasks",
];

/// Файлы, игнорируемые всегда (fnmatch-паттерны, без учёта регистра).
pub const GLOBAL_IGNORED_FILES: &[&str] = &[
    ".DS_Store", "Thumbs.db", "desktop.ini", "*.pyc", "*.pyo", "*.pyd", ".so", "*.dll", "*.log",
    "*.tmp", "*.bak", "*.swp", "*.swo", ".coverage", "todo.md",
];

/// Исключены по умолчанию (видны, но не отмечаются автоматически).
pub const EXCLUDED_BY_DEFAULT_PATTERNS: &[&str] = &[
    "poetry.lock", "pnpm-lock.yaml", "yarn.lock", "package-lock.json", "Pipfile.lock",
    ".eslintcache", "*.min.js", "*.min.css", "LICENSE", "LICENSE.*", "COPYING", "NOTICE",
    "*.ipynb_checkpoints*", "go.sum",
];

/// Расширения-бинарники (перенос BINARY_EXTENSIONS из file_processing.py).
pub const BINARY_EXTENSIONS: &[&str] = &[
    ".jpg", ".jpeg", ".png", ".gif", ".bmp", ".tiff", ".webp", ".svg", ".ico", ".mp3", ".wav",
    ".aac", ".ogg", ".flac", ".m4a", ".mp4", ".mkv", ".avi", ".mov", ".wmv", ".flv", ".webm",
    ".pdf", ".doc", ".docx", ".xls", ".xlsx", ".ppt", ".pptx", ".odt", ".ods", ".odp", ".zip",
    ".rar", ".tar", ".gz", ".bz2", ".7z", ".jar", ".war", ".exe", ".dll", ".so", ".dylib",
    ".app", ".msi", ".sqlite", ".db", ".mdb", ".ttf", ".otf", ".woff", ".woff2", ".pyc", ".pyo",
    ".pyd", ".class", ".bundle", ".swf", ".dat", ".bin", ".obj", ".lib", ".a", ".pak", ".assets",
    ".resource", ".resS",
];

/// Файл больше этого размера не читается и не токенизируется (1 MB).
pub const MAX_FILE_SIZE_BYTES: u64 = 1 * 1024 * 1024;

/// Статус файла в дереве.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileStatus {
    /// Обычный текстовый файл.
    Ok,
    /// Бинарный (по расширению) — в буфер не попадает.
    Binary,
    /// Больше `MAX_FILE_SIZE_BYTES` — в буфер не попадает.
    TooLarge,
    /// Лок/минифицированные — можно отметить вручную.
    ExcludedByDefault,
    /// Ошибка stat/чтения.
    ReadError(String),
}

/// Запись о файле в дереве.
#[derive(Debug, Clone)]
pub struct FileEntry {
    /// Относительный путь с `/` (для тегов и отображения).
    pub rel_path: String,
    pub size: u64,
    pub status: FileStatus,
}

/// Запись о папке (для дерева UI).
#[derive(Debug, Clone)]
pub struct DirEntry {
    pub rel_path: String,
}

/// Результат сканирования проекта.
#[derive(Debug, Clone, Default)]
pub struct ScanOutput {
    pub dirs: Vec<DirEntry>,
    pub files: Vec<FileEntry>,
}

/// Просканировать проект: файлы по `ignore`-каскаду + глобальные исключения.
pub fn scan(root: &Path) -> ScanOutput {
    let mut out = ScanOutput::default();
    let walker = WalkBuilder::new(root)
        .hidden(false)
        // Каскадные .gitignore только ВНУТРИ root (как в старой версии:
        // корневой + локальные в подпапках), без машинного глобального игнора.
        .git_ignore(true)
        .git_global(false)
        .git_exclude(true)
        .parents(false)
        // Вне git-репозитория .gitignore всё равно применяется — старая
        // версия не требовала наличия .git.
        .require_git(false)
        // Служебные папки вырезаем с поддеревом (как os.walk с pruning).
        .filter_entry(|e| e.depth() == 0 || !is_global_ignored_dir(e))
        .build();

    for entry in walker.flatten() {
        let path = entry.path();
        let Some(rel) = path.strip_prefix(root).ok().map(p_to_rel) else {
            continue;
        };
        if rel.is_empty() {
            continue;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        if entry.file_type().is_some_and(|t| t.is_dir()) {
            if GLOBAL_IGNORED_DIRS.contains(&name.as_str()) {
                continue;
            }
            out.dirs.push(DirEntry { rel_path: rel });
        } else {
            if is_ignored_file(&name) {
                continue;
            }
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            out.files.push(FileEntry {
                rel_path: rel,
                size,
                status: file_status(&name, size),
            });
        }
    }
    sort_output(&mut out);
    out
}

fn p_to_rel(p: &std::path::Path) -> String {
    p.components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

/// Служебная папка (GLOBAL_IGNORED_DIRS)? Для `filter_entry`: false обрезает
/// папку вместе с поддеревом.
fn is_global_ignored_dir(e: &ignore::DirEntry) -> bool {
    e.file_type().is_some_and(|t| t.is_dir())
        && e.file_name().to_str().is_some_and(|n| GLOBAL_IGNORED_DIRS.contains(&n))
}

/// Глобальные игноры файлов: fnmatch-паттерны без учёта регистра.
fn is_ignored_file(name: &str) -> bool {
    GLOBAL_IGNORED_FILES.iter().any(|pat| fnmatch(pat, name))
}

/// Статус по имени/размеру (перенос get_item_status_info, без токенов).
pub fn file_status(name: &str, size: u64) -> FileStatus {
    let lower = name.to_lowercase();
    let ext = lower.rsplit_once('.').map(|(_, e)| format!(".{e}"));

    if ext.as_deref().is_some_and(|e| BINARY_EXTENSIONS.contains(&e)) {
        return FileStatus::Binary;
    }
    if size > MAX_FILE_SIZE_BYTES {
        return FileStatus::TooLarge;
    }
    if EXCLUDED_BY_DEFAULT_PATTERNS
        .iter()
        .any(|pat| fnmatch(pat, &lower))
    {
        return FileStatus::ExcludedByDefault;
    }
    FileStatus::Ok
}

/// Мини-fnmatch: только `*` (хватает для всех паттернов старой версии),
/// без учёта регистра (fnmatch на Windows нормализует регистр обоих строк).
pub fn fnmatch(pattern: &str, name: &str) -> bool {
    let pat = pattern.to_lowercase();
    let name = name.to_lowercase();
    let parts: Vec<&str> = pat.split('*').collect();
    let mut rest: &str = name.as_str();
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        let found = if i == 0 {
            if rest.starts_with(part) { Some(0) } else { None }
        } else {
            rest.find(part)
        };
        let Some(pos) = found else { return false };
        rest = &rest[pos + part.len()..];
    }
    // Хвостовой литерал должен заканчивать имя.
    if let Some(last) = parts.last() {
        if !last.is_empty() && parts.len() > 1 {
            return rest.is_empty() || name.ends_with(last);
        }
    }
    true
}

fn sort_output(out: &mut ScanOutput) {
    // Без учёта регистра — как сортировка старого дерева (name.lower()).
    out.dirs.sort_unstable_by_key(|d| d.rel_path.to_lowercase());
    out.files.sort_unstable_by_key(|f| f.rel_path.to_lowercase());
}
