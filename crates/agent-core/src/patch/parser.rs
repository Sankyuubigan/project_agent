//! Парсер «Умного Markdown»: `<<<FILE: путь>>>…<<<END_FILE: путь>>>`,
//! SEARCH/REPLACE внутри блока, `<<<DELETE_FILE: путь>>>`.
//!
//! Правила (вердикты спеца №2/№3 + решение владельца):
//! - путь в закрывающем теге обязателен, не совпал — `PatchError`;
//! - голый `<<<END_FILE>>>` внутри блока НЕ закрывает его (self-hosting
//!   защита) — остаётся содержимым с WARN;
//! - незакрытый блок — ошибка, содержимое не теряется молча;
//! - пустой блок = создать пустой файл + WARN (удаления по пустоте НЕТ);
//! - путь: относительный, `/`, без `..`, без абсолютных.

use super::models::{ChangeKind, Edit, FileChange, PatchError, ParsedResponse};

/// Строка-открывающий тег: `<<<FILE: путь>>>`.
const FILE_PREFIX: &str = "<<<FILE:";
/// Строка-закрывающий тег с путём: `<<<END_FILE: путь>>>`.
const END_FILE_PREFIX: &str = "<<<END_FILE:";
/// Однострочный тег удаления: `<<<DELETE_FILE: путь>>>`.
const DELETE_PREFIX: &str = "<<<DELETE_FILE:";
/// Общий суффикс тегов.
const TAG_SUFFIX: &str = ">>>";
/// Маркеры SEARCH/REPLACE.
const SEARCH_MARKER: &str = "<<<<<<< SEARCH";
const SPLIT_MARKER: &str = "=======";
const REPLACE_MARKER: &str = ">>>>>>> REPLACE";

/// Разобрать ответ LLM на список изменений.
pub fn parse(response: &str) -> ParsedResponse {
    let mut out = ParsedResponse::default();
    let mut open: Option<OpenBlock> = None;

    for (idx, raw_line) in response.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw_line.trim();

        if let Some(block) = open.as_mut() {
            if let Some(end_path) = tag_path(line, END_FILE_PREFIX) {
                close_block(block, end_path, line_no, &mut out);
                open = None;
            } else if line == END_FILE_PREFIX || line == "<<<END_FILE>>>" {
                // Голый END_FILE: self-hosting защита — это содержимое, не конец.
                out.warnings.push(format!(
                    "строка {line_no}: голый <<<END_FILE>>> внутри блока `{}` — считается содержимым",
                    block.path
                ));
                block.body.push(raw_line.to_string());
            } else {
                block.body.push(raw_line.to_string());
            }
            continue;
        }

        if let Some(path) = tag_path(line, FILE_PREFIX) {
            match validate_path(&path) {
                Ok(norm) => open = Some(OpenBlock::new(norm, line_no)),
                Err(reason) => out.errors.push(PatchError::new(Some(&path), reason)),
            }
        } else if let Some(path) = tag_path(line, DELETE_PREFIX) {
            handle_delete(&path, &mut out);
        } else if line == END_FILE_PREFIX || line == "<<<END_FILE>>>" {
            out.warnings
                .push(format!("строка {line_no}: голый <<<END_FILE>>> вне блока — игнорируется"));
        }
        // Остальной текст вне блоков игнорируется.
    }

    if let Some(block) = open {
        out.errors.push(PatchError::new(
            Some(&block.path),
            format!(
                "строка {}: незакрытый блок <<<FILE: {}>>> — нет <<<END_FILE: {}>>>",
                block.start_line, block.path, block.path
            ),
        ));
    }

    out
}

/// Открытый блок FILE, накапливающий строки содержимого.
struct OpenBlock {
    path: String,
    start_line: usize,
    body: Vec<String>,
}

impl OpenBlock {
    fn new(path: String, start_line: usize) -> Self {
        Self {
            path,
            start_line,
            body: Vec::new(),
        }
    }
}

/// Вытащить путь из тега `PREFIX путь>>>`; None, если строка не тег.
fn tag_path(line: &str, prefix: &str) -> Option<String> {
    let rest = line.strip_prefix(prefix)?.trim_start();
    let path = rest.strip_suffix(TAG_SUFFIX)?.trim();
    Some(path.to_string())
}

/// Закрыть открытый блок: путь в END обязан совпасть; содержимое — Whole или Edits.
fn close_block(block: &mut OpenBlock, end_path: String, line_no: usize, out: &mut ParsedResponse) {
    // Путь закрывающего тега валидируем/нормализуем так же, как открывающий
    // (`\`→`/` и правила сегментов), иначе сравнение с block.path ложно.
    let Ok(end_norm) = validate_path(&end_path) else {
        out.errors.push(PatchError::new(
            Some(&block.path),
            format!("строка {line_no}: некорректный путь закрывающего тега `{end_path}`"),
        ));
        return;
    };
    if end_norm != block.path {
        out.errors.push(PatchError::new(
            Some(&block.path),
            format!(
                "строка {line_no}: путь закрывающего тега `{end_path}` не совпадает с открывающим `{}`",
                block.path
            ),
        ));
        return;
    }

    let content = strip_one_fence(&block.body).join("\n");
    if content.trim().is_empty() && block.body.iter().all(|l| l.trim().is_empty()) {
        // Пустой блок = создать пустой файл + WARN (НЕ удалять — решение владельца).
        out.warnings.push(format!(
            "блок `{}` пуст: будет создан пустой файл (удаление — только <<<DELETE_FILE>>>)",
            block.path
        ));
        out.changes.push(FileChange {
            path: block.path.clone(),
            kind: ChangeKind::Whole { content: String::new() },
        });
        return;
    }

    match parse_search_replace(&content) {
        Ok(Some(edits)) => out.changes.push(FileChange {
            path: block.path.clone(),
            kind: ChangeKind::Edits { edits },
        }),
        Ok(None) => out.changes.push(FileChange {
            path: block.path.clone(),
            kind: ChangeKind::Whole { content },
        }),
        Err(reason) => out.errors.push(PatchError::new(Some(&block.path), reason)),
    }
}

/// Тег удаления: путь валиден → изменение Delete; иначе ошибка.
fn handle_delete(path: &str, out: &mut ParsedResponse) {
    match validate_path(path) {
        Ok(norm) => out.changes.push(FileChange {
            path: norm,
            kind: ChangeKind::Delete,
        }),
        Err(reason) => out.errors.push(PatchError::new(Some(path), reason)),
    }
}

/// Валидация относительного пути файла: без `..`, абсолютных и пустых сегментов.
fn validate_path(path: &str) -> Result<String, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("пустой путь в теге".to_string());
    }
    let unified = trimmed.replace('\\', "/");
    if unified.starts_with('/') {
        return Err(format!("абсолютный путь `{trimmed}` запрещён"));
    }
    if unified.len() >= 2 && unified.as_bytes()[1] == b':' {
        return Err(format!("абсолютный путь с диском `{trimmed}` запрещён"));
    }
    for seg in unified.split('/') {
        if seg.is_empty() || seg == "." {
            return Err(format!("пустой или `.` сегмент в пути `{trimmed}`"));
        }
        if seg == ".." {
            return Err(format!("выход за корень проекта (`..`) в пути `{trimmed}`"));
        }
    }
    Ok(unified)
}

/// Снять ОДНУ оборачивающую ```-пару (как в старом парсере patching.py).
fn strip_one_fence(body: &[String]) -> Vec<String> {
    let first_nonempty = body.iter().position(|l| !l.trim().is_empty());
    let last_nonempty = body.iter().rposition(|l| !l.trim().is_empty());
    let (Some(first), Some(last)) = (first_nonempty, last_nonempty) else {
        return body.to_vec();
    };

    let head = body[first].trim();
    let opens = head.starts_with("```") && head[3..].trim().len() <= 32;
    let tail = body[last].trim();
    let closes = tail == "```";

    if opens && closes && last > first {
        let out: Vec<String> = body[first + 1..last].to_vec();
        if out.iter().all(|l| l.trim().is_empty()) {
            return Vec::new();
        }
        return out;
    }
    body.to_vec()
}

/// Разобрать содержимое блока: маркер `SEARCH` есть → строгий разбор Edits
/// (любой текст вне ханков — ошибка); маркера нет вообще → `Ok(None)` —
/// это целая замена файла (Whole), а не испорченный Edits.
fn parse_search_replace(content: &str) -> Result<Option<Vec<Edit>>, String> {
    if !content.lines().any(|l| l.trim() == SEARCH_MARKER) {
        return Ok(None);
    }
    let mut lines = content.lines();
    let mut edits: Vec<Edit> = Vec::new();
    let mut state = State::Outside;
    let mut search_buf: Vec<&str> = Vec::new();
    let mut replace_buf: Vec<&str> = Vec::new();

    for line in lines.by_ref() {
        match state {
            State::Outside => {
                if line.trim() == SEARCH_MARKER {
                    state = State::Search;
                } else if !line.trim().is_empty() {
                    return Err(format!(
                        "текст вне SEARCH/REPLACE внутри блока с маркерами: `{}`",
                        trim_chars(line)
                    ));
                }
            }
            State::Search => {
                if line.trim() == SPLIT_MARKER {
                    state = State::Replace;
                } else {
                    search_buf.push(line);
                }
            }
            State::Replace => {
                if line.trim() == REPLACE_MARKER {
                    edits.push(Edit {
                        search: search_buf.join("\n"),
                        replace: replace_buf.join("\n"),
                    });
                    search_buf.clear();
                    replace_buf.clear();
                    state = State::Outside;
                } else {
                    replace_buf.push(line);
                }
            }
        }
    }

    match state {
        State::Outside if edits.is_empty() => Ok(None),
        State::Outside => Ok(Some(edits)),
        State::Search => Err("SEARCH без разделителя =======".to_string()),
        State::Replace => Err("SEARCH/======= без закрывающего >>>>>>> REPLACE".to_string()),
    }
}

enum State {
    Outside,
    Search,
    Replace,
}

/// Первые 40 символов строки для сообщений об ошибках.
fn trim_chars(line: &str) -> String {
    line.chars().take(40).collect()
}
