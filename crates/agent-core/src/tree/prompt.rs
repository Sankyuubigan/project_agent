//! Сборщик промпта (перенос clipboard_logic.py): [инструкции] → структура →
//! содержимое отмеченных файлов. Буфер — инфраструктура UI; ядро строит текст.
//!
//! Отличие от старой версии: блоки файлов — `<<<END_FILE: путь>>>` (формат v2),
//! путь в закрывающем теге (вердикт спеца №3). Инструкций несколько
//! (выдача изменений + режим плана), каждая — внешний md рядом с exe.

use std::fs;
use std::path::{Path, PathBuf};

use super::scanner::FileEntry;

/// Файл инструкции выдачи изменений (внешний md рядом с exe, требование владельца).
pub const CHANGES_FILE: &str = "markdown_changes.md";

/// Файл инструкции режима плана (внешний md рядом с exe, требование владельца).
pub const PLAN_FILE: &str = "markdown_plan.md";

/// Разделитель секций промпта (как в старой версии).
const SECTION_SEP: &str = "\n\n---\n\n";

/// Одна инструкция промпта: состояние чекбокса + прочитанный текст.
/// Логика чтения md рядом с exe общая для всех инструкций — SSOT для
/// egui- и GPUI-фронтов (прежде дублировалась в обоих).
pub struct InstructionSlot {
    /// Инструкция включена чекбоксом в UI.
    pub checked: bool,
    text: Option<String>,
    error: Option<String>,
}

impl InstructionSlot {
    pub fn new(checked: bool) -> Self {
        Self {
            checked,
            text: None,
            error: None,
        }
    }

    /// Включили чекбокс — (пере)читать md с диска. Ошибка видима, не молчит.
    pub fn refresh(&mut self, base: &Path, file: &str) {
        let result = find_instruction_path(base, file)
            .ok_or_else(|| format!("doc/{file} не найден рядом с exe (и выше по папкам)"))
            .and_then(|p| load_instruction(&p).map(|text| (text, p)));
        match result {
            Ok((text, path)) => {
                log::info!("инструкция {file}: {}", path.display());
                self.text = Some(text);
                self.error = None;
            }
            Err(e) => {
                log::warn!("инструкция {file}: {e}");
                self.text = None;
                self.error = Some(e);
            }
        }
    }

    /// Прочитанный текст (для сборки промпта); `None` — надо лениво дочитать.
    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    /// Строка-подсказка под чекбоксом: `Some((текст, это_ошибка))`.
    pub fn hint(&self, label: &str) -> Option<(String, bool)> {
        match &self.error {
            Some(err) => Some((err.clone(), true)),
            None => self
                .text
                .as_ref()
                .map(|t| (format!("{label}: {} символов", t.len()), false)),
        }
    }
}

/// Порядок и разделители — как в старой версии (clipboard_logic.py).
/// Содержимое файлов читается от `root` по относительным путям записей.
pub fn build_prompt(
    root: &Path,
    selected: &[FileEntry],
    structure_text: Option<String>,
    instructions: &[String],
) -> PromptResult {
    let mut parts: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut file_count = 0usize;

    // 1) Инструкции (внешние md-файлы рядом с exe), в порядке UI.
    for instr in instructions {
        parts.push(instr.clone());
        parts.push(SECTION_SEP.to_string());
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

/// Загрузить текст инструкции из внешнего md-файла.
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
