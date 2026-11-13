//! Сборщик промпта (перенос clipboard_logic.py): [инструкция] → структура →
//! содержимое отмеченных файлов. Буфер — инфраструктура UI; ядро строит текст.
//!
//! Отличие от старой версии: блоки файлов — `<<<END_FILE: путь>>>` (формат v2),
//! путь в закрывающем теге (вердикт спеца №3).

use std::fs;
use std::path::{Path, PathBuf};

use super::scanner::FileEntry;

/// Порядок и разделители — как в старой версии (clipboard_logic.py).
/// Содержимое файлов читается от `root` по относительным путям записей.
pub fn build_prompt(
    root: &Path,
    selected: &[FileEntry],
    structure_text: Option<String>,
    instruction_text: Option<String>,
) -> PromptResult {
    let mut parts: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut file_count = 0usize;

    // 1) Инструкция метода (внешний md-файл рядом с exe).
    if let Some(instr) = instruction_text {
        parts.push(instr);
        parts.push("\n\n---\n\n".to_string());
    }

    // 2) Структура проекта (если включена).
    if let Some(structure) = structure_text {
        if !structure.trim().is_empty() {
            parts.push(structure);
            parts.push("\n\n---\nСодержимое выбранных файлов:\n---\n\n".to_string());
        } else {
            warnings.push("структура проекта пуста — не включена".to_string());
        }
    }

    // 3) Блоки файлов (формат v2: путь и в открывающем, и в закрывающем теге).
    let mut blocks: Vec<String> = Vec::new();
    for file in selected {
        match fs::read_to_string(root.join(&file.rel_path)) {
            Ok(content) => {
                blocks.push(format!(
                    "<<<FILE: {}>>>\n{}\n<<<END_FILE: {}>>>",
                    file.rel_path, content, file.rel_path
                ));
                file_count += 1;
            }
            Err(e) => warnings.push(format!("{}: не прочитан ({e})", file.rel_path)),
        }
    }
    if !blocks.is_empty() {
        parts.push(blocks.join("\n\n"));
    }

    PromptResult {
        text: parts.join("").trim().to_string(),
        file_count,
        warnings,
    }
}

/// Результат сборки промпта.
pub struct PromptResult {
    pub text: String,
    pub file_count: usize,
    pub warnings: Vec<String>,
}

/// Загрузить текст инструкции метода из внешнего md-файла.
/// Файл не найден/пуст → `Err` с объяснением (никакого молчания).
pub fn load_instruction(path: &Path) -> Result<String, String> {
    if !path.is_file() {
        return Err(format!(
            "файл инструкции не найден: {} (должен лежать рядом с exe)",
            path.display()
        ));
    }
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if text.trim().is_empty() {
        return Err(format!("файл инструкции пуст: {}", path.display()));
    }
    Ok(text)
}

/// Путь инструкции от папки exe: `<exe_dir>/doc/<method_file>`.
pub fn instruction_path(exe_dir: &Path, method_file: &str) -> PathBuf {
    exe_dir.join("doc").join(method_file)
}

/// Найти `doc/<method_file>` рядом с exe; в dev-запуске exe лежит глубже
/// (`target/debug`) — поднимаемся максимум на 4 уровня вверх. Release-установка
/// находит файл на первом шаге.
pub fn find_instruction_path(exe_dir: &Path, method_file: &str) -> Option<PathBuf> {
    let mut dir = exe_dir.to_path_buf();
    for _ in 0..5 {
        let cand = dir.join("doc").join(method_file);
        if cand.is_file() {
            return Some(cand);
        }
        if !dir.pop() {
            break;
        }
    }
    None
}
