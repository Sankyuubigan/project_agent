//! 1-строчные сигнатуры объявлений для энрича `find_files` (Доработка №1
//! [PRP tasks/PRP_ai_pick_storage_link.md], Гипотеза A спеца).
//!
//! Проблема: выдача find_files — голые пути без подсказок, и модель 12B не
//! связывает «persist на бэкенде» с `infra/session_manager.rs`, хотя файл
//! перед глазами. Решение: для хитов (≤25) прочитать первые ~250 строк с
//! диска (2–5 мс, чистый Rust, БЕЗ codemap-индекса) и собрать навигационную
//! карту `путь  [ChatSession, SessionManager, save_session]`.
//!
//! Приоритет — типы данных (struct/enum/class/interface/trait/impl) перед
//! fn: модель ищет «где объявлена структура», а не методы.

use std::io::BufRead;
use std::path::Path;

/// Сколько первых строк файла сканируем (вердикт спеца: ~250).
const SCAN_LINES: usize = 250;
/// Максимум подписей на файл; типы имеют приоритет перед fn.
const MAX_SIGS_PER_FILE: usize = 6;
/// Лимит длины подписи (символы, вердикт спеца: ~60–80).
const SIG_MAX_CHARS: usize = 72;
/// Энричим выдачу find_files только при ≤25 хитов (вердикт спеца: чтение
/// заголовков 25 файлов занимает 2–5 мс; 40 хитов — уже лишний ввод-вывод).
pub(crate) const ENRICH_MAX_HITS: usize = 25;

/// Шаблоны объявлений-ТИПОВ (приоритетные) и объявлений-FN.
/// Совпадение — по началу строки после трима (пробел в шаблоне защищает от
/// попадания идентификаторов вроде `classname`).
const TYPE_PATTERNS: [&str; 14] = [
    "pub struct ",
    "struct ",
    "pub enum ",
    "enum ",
    "pub trait ",
    "trait ",
    "impl ",
    "export interface ",
    "interface ",
    "export class ",
    "class ",
    "export type ",
    "type ",
    "export enum ",
];
const FN_PATTERNS: [&str; 7] = [
    "pub fn ",
    "pub async fn ",
    "async fn ",
    "fn ",
    "export function ",
    "function ",
    "async ", // TS-методы класса: `async fetchSessions() {}` (без `function`)
];

/// Является ли (после трима) строка объявлением. `true` — тип (приоритет),
/// `false` — fn; None — не объявление. Комментарии/атрибуты отсечены выше.
fn classify(line: &str) -> Option<bool> {
    for p in TYPE_PATTERNS {
        if line.starts_with(p) {
            return Some(true);
        }
    }
    for p in FN_PATTERNS {
        if line.starts_with(p) {
            return Some(false);
        }
    }
    None
}

/// Подпись в одну компактную строку: тело однострочных объявлений
/// (` { … }`) и хвост ` {` многострочных — шум, отрезаем по первому `{`;
/// затем сжатие пробелов, обрезка по лимиту, снятие хвостового `=`
/// (TS `export type Foo = …`).
fn sig_line(line: &str) -> String {
    let head = line.split('{').next().unwrap_or("");
    let mut out = String::with_capacity(SIG_MAX_CHARS * 2);
    let mut prev_space = true;
    for c in head.chars() {
        if out.chars().count() >= SIG_MAX_CHARS {
            break;
        }
        if c.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(c);
            prev_space = false;
        }
    }
    let s = out.trim_end();
    s.strip_suffix('=')
        .map(str::trim_end)
        .unwrap_or(s)
        .to_string()
}

/// Сигнатуры одного файла: первые SCAN_LINES строк, типы первыми, fn после,
/// суммарно ≤ MAX_SIGS_PER_FILE. Ошибка чтения (отсутствует/бинарь) —
/// пустой Vec, БЕЗ паники (энрич — необязательное улучшение).
pub(crate) fn file_signatures(abs_path: &Path) -> Vec<String> {
    let Ok(file) = std::fs::File::open(abs_path) else {
        log::debug!("сигнатуры: файл не открыт {}", abs_path.display());
        return Vec::new();
    };
    let reader = std::io::BufReader::new(file);
    let mut types: Vec<String> = Vec::new();
    let mut fns: Vec<String> = Vec::new();
    for (i, line) in reader.lines().enumerate() {
        if i >= SCAN_LINES {
            break;
        }
        let Ok(raw) = line else { break };
        let trimmed = raw.trim();
        if trimmed.is_empty()
            || trimmed.starts_with("//")
            || trimmed.starts_with("/*")
            || trimmed.starts_with('*')
            || trimmed.starts_with("#[")
        {
            continue;
        }
        match classify(trimmed) {
            Some(true) if types.len() < MAX_SIGS_PER_FILE => types.push(sig_line(trimmed)),
            Some(false) if fns.len() < MAX_SIGS_PER_FILE => fns.push(sig_line(trimmed)),
            _ => {}
        }
    }
    types.into_iter().chain(fns).take(MAX_SIGS_PER_FILE).collect()
}

/// Энрич выдачи find_files: к каждому пути с найденными подписями добавить
/// `  [A, B, C]`. Бюджет `budget_chars` — запас символов на суффиксы: когда
/// исчерпан, оставшиеся пути идут БЕЗ подписей (пути не теряются никогда —
/// обрезка вывода по общему лимиту не отрезает хиты).
pub(crate) fn enrich_find_files(
    root: &Path,
    hits: &[String],
    budget_chars: usize,
) -> Vec<String> {
    let mut out = Vec::with_capacity(hits.len());
    let mut used = 0usize;
    for h in hits {
        let sigs = file_signatures(&root.join(h));
        if sigs.is_empty() || used >= budget_chars {
            out.push(h.clone());
            continue;
        }
        let suffix = format!("  [{}]", sigs.join(", "));
        used += suffix.chars().count();
        out.push(format!("{h}{suffix}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// tmp-файл с содержимым; TempDir возвращается парой и живёт до конца
    /// теста (удаление — в его drop).
    fn write_tmp(name: &str, content: &str) -> (PathBuf, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("tmpdir");
        let p = dir.path().join(name);
        std::fs::write(&p, content).expect("write");
        (p, dir)
    }

    #[test]
    fn rust_types_first_then_fns() {
        let (p, _dir) = write_tmp(
            "lib.rs",
            "//! док\npub struct ChatSession {\n    id: String,\n}\n\n\
             #[tauri::command]\npub fn get_sessions() -> Vec<ChatSession> { vec![] }\n\n\
             pub enum Kind { A, B }\n\nimpl ChatSession {\n    fn save(&self) {}\n}\n",
        );
        let sigs = file_signatures(&p);
        assert_eq!(sigs[0], "pub struct ChatSession", "{sigs:?}");
        assert_eq!(sigs[1], "pub enum Kind", "{sigs:?}");
        assert_eq!(sigs[2], "impl ChatSession", "{sigs:?}");
        assert!(sigs[3].starts_with("pub fn get_sessions"), "{sigs:?}");
        assert!(sigs[4].starts_with("fn save"), "{sigs:?}");
    }

    #[test]
    fn ts_interfaces_and_classes() {
        let (p, _dir) = write_tmp(
            "svc.ts",
            "import { x } from \"./x\";\nexport interface SessionMeta {\n  id?: string;\n}\n\n\
             export class SessionService {\n  async fetchSessions() {}\n}\n",
        );
        let sigs = file_signatures(&p);
        assert_eq!(sigs[0], "export interface SessionMeta", "{sigs:?}");
        assert_eq!(sigs[1], "export class SessionService", "{sigs:?}");
        assert!(sigs[2].starts_with("async fetchSessions"), "{sigs:?}");
    }

    #[test]
    fn long_line_truncated_and_comment_skipped() {
        let (p, _dir) = write_tmp(
            "long.rs",
            "// struct Commented — не подпись\n/* struct Block */\n\
             pub struct ThisIsAVeryLongStructNameThatMustBeTruncatedSomeWhereAroundSeventyTwoChars { x: u8 }\n",
        );
        let sigs = file_signatures(&p);
        assert_eq!(sigs.len(), 1, "{sigs:?}");
        assert!(sigs[0].chars().count() <= SIG_MAX_CHARS, "{sigs:?}");
        assert!(!sigs[0].contains("Commented"), "{sigs:?}");
    }

    #[test]
    fn unreadable_file_is_skipped() {
        let sigs = file_signatures(Path::new("Q:/definitely/not/here.rs"));
        assert!(sigs.is_empty());
    }

    #[test]
    fn enrich_appends_suffix_and_respects_budget() {
        let dir = tempfile::tempdir().expect("tmpdir");
        std::fs::write(dir.path().join("a.rs"), "pub struct Alpha {\n}\n")
            .expect("write");
        std::fs::write(dir.path().join("b.rs"), "pub struct Beta {\n}\n").expect("write");
        let rel = |n: &str| {
            let s = n.to_string();
            s
        };
        let hits = vec![rel("a.rs"), rel("b.rs")];
        let enriched = enrich_find_files(dir.path(), &hits, 500);
        assert!(enriched[0].starts_with("a.rs  [pub struct Alpha"), "{enriched:?}");
        assert!(enriched[1].starts_with("b.rs  [pub struct Beta"), "{enriched:?}");
        // Бюджет 0: пути остаются голыми
        let bare = enrich_find_files(dir.path(), &hits, 0);
        assert_eq!(bare, vec!["a.rs", "b.rs"], "{bare:?}");
    }
}
