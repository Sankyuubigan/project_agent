//! Тулы tool-loop AI-подбора (Шаг 2 [PRP tasks/PRP_ai_pick_tool_loop.md]).
//! Пять инструментов исследования, которые модель вызывает сама в Фазе 1
//! (нативный OpenAI tools[] через плагин llama-engine):
//! - `find_files` — чистый Rust, 0 мс, по списку путей из скана UI
//!   (лечит модули-сироты: файл без ссылочных рёбер не попадает в codemap-карту);
//! - `codemap_map` / `codemap_search` / `codemap_callers` / `codemap_skeleton`
//!   — обёртки agent-tools codemap (см. agent_core::tree::repomap).
//!
//! Лимиты выводов — контекст-политика спеца: map ≤3500, skeleton ≤4000,
//! search/callers/find_files ≤2500 символов. Ошибки тула НЕ роняют подбор —
//! модель получает «Tool error: …» и решает сама.

use std::collections::HashSet;
use std::path::Path;

use agent_core::tree::repomap;
use serde_json::{json, Value};
use tauri_plugin_llama_engine::engine::llm_types::{FunctionDef, ToolCall, ToolDefinition};

/// Максимум путей в ответе find_files (остальное — «… ещё N»).
pub const MAX_FIND_RESULTS: usize = 40;

/// Лимиты тул-выводов ДО вставки в историю диалога (символы, не байты —
/// текст русский и пути; вердикт спеца §контекст-политика).
pub const LIMIT_MAP: usize = 3500;
pub const LIMIT_SKELETON: usize = 4000;
pub const LIMIT_SHORT: usize = 2500;

/// Результат тул-вызова: текст для модели (уже обрезанный) и встреченные
/// пути (для анти-галлюцинационной валидации оркестратора).
pub struct ToolExecOutput {
    pub content: String,
    pub found_paths: Vec<String>,
}

fn tool(name: &str, description: &str, props: Value, required: &[&str]) -> ToolDefinition {
    ToolDefinition {
        r#type: "function".to_string(),
        function: FunctionDef {
            name: name.to_string(),
            description: description.to_string(),
            parameters: json!({
                "type": "object",
                "properties": props,
                "required": required,
                "additionalProperties": false
            }),
        },
    }
}

/// Пять тулов Фазы 1 (сигнатуры — по вердикту спеца: плоские скаляры,
/// короткие description). `body` НЕ даётся — провоцирует чтение кода вместо
/// навигации.
pub fn tool_definitions() -> Vec<ToolDefinition> {
    vec![
        tool(
            "find_files",
            "Ищет файлы проекта по подстроке в имени или пути (без учёта регистра). Быстрый поиск по всем файлам проекта.",
            json!({
                "query": { "type": "string", "description": "Подстрока имени файла/каталога, например csv или session_manager" },
                "dir": { "type": "string", "description": "Подкаталог от корня проекта для сужения поиска (опционально)" }
            }),
            &["query"],
        ),
        tool(
            "codemap_map",
            "Карта каталога проекта: структура и сигнатуры файлов. Мелкий бюджет — обзор, крупный — детали.",
            json!({
                "budget": { "type": "integer", "enum": [512, 768], "description": "Бюджет карты в токенах" },
                "scope": { "type": "string", "description": "Подкаталог от корня проекта; без него — карта всего корня (опционально)" }
            }),
            &["budget"],
        ),
        tool(
            "codemap_search",
            "Ищет символ или текст по карте проекта: возвращает файлы и номера строк (определения и упоминания).",
            json!({
                "query": { "type": "string", "description": "Символ или подстрока, например имя функции или структуры" }
            }),
            &["query"],
        ),
        tool(
            "codemap_callers",
            "Возвращает файлы и строки, где символ вызывается или используется.",
            json!({
                "symbol": { "type": "string", "description": "Имя функции/структуры/константы" }
            }),
            &["symbol"],
        ),
        tool(
            "codemap_skeleton",
            "Скелет одного файла: импорты и сигнатуры без тел функций. Показывает, с чем файл связан.",
            json!({
                "file": { "type": "string", "description": "Путь файла от корня проекта" }
            }),
            &["file"],
        ),
    ]
}

/// Нормализация пути проекта: `\` → `/`, убрать ведущий `./`.
pub(crate) fn norm_path(p: &str) -> String {
    let s = p.replace('\\', "/");
    s.strip_prefix("./").unwrap_or(&s).to_string()
}

fn truncate(content: &str, limit: usize) -> String {
    if content.chars().count() <= limit {
        return content.to_string();
    }
    let cut: String = content.chars().take(limit).collect();
    format!("{cut}\n… [вывод обрезан по лимиту {limit} символов]")
}

/// find_files: substring-матч по списку путей скана UI. `dir` — опциональный
/// префикс-подкаталог. Возвращает (текст для модели, найденные пути).
fn find_files(
    root: &Path,
    project_paths: &[String],
    query: &str,
    dir: Option<&str>,
) -> Result<(String, Vec<String>), String> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Err("пустой query".to_string());
    }
    let dir_prefix = dir
        .map(norm_path)
        .map(|d| d.trim_matches('/').to_lowercase())
        .filter(|d| !d.is_empty());
    let mut hits: Vec<String> = Vec::new();
    for p in project_paths {
        let np = norm_path(p);
        let np_lower = np.to_lowercase();
        let in_dir = dir_prefix
            .as_deref()
            .map(|d| np_lower.starts_with(&format!("{d}/")) || np_lower == d)
            .unwrap_or(true);
        if in_dir && np_lower.contains(&q) {
            hits.push(np);
        }
    }
    hits.sort();
    let total = hits.len();
    let shown: Vec<String> = hits.into_iter().take(MAX_FIND_RESULTS).collect();
    let found = shown.clone();
    let content = if shown.is_empty() {
        format!("Файлы по запросу «{query}» не найдены. Попробуй другую подстроку.")
    } else {
        // Энрич (Доработка №1 PRP storage-link): подписи объявлений для ≤25
        // хитов — прямая зацепка «путь → структуры» для модели. found
        // остаётся чистым списком путей (валидация/trim его не парсят).
        let mut display = shown.clone();
        if display.len() <= crate::ai_pick_snippets::ENRICH_MAX_HITS {
            let base = display.join("\n").chars().count();
            let reserve = if total > MAX_FIND_RESULTS { 60 } else { 20 };
            let budget = LIMIT_SHORT.saturating_sub(base + reserve);
            display = crate::ai_pick_snippets::enrich_find_files(root, &display, budget);
        }
        let mut s = display.join("\n");
        if total > MAX_FIND_RESULTS {
            s.push_str(&format!(
                "\n… ещё {} файлов — уточни query или укажи dir",
                total - MAX_FIND_RESULTS
            ));
        }
        s
    };
    Ok((content, found))
}

/// Извлечь из вывода тула пути, которые реально есть в проекте (для
/// observed_paths). Встречаются в формах `путь:строка`, `путь:` и `путь,`.
pub(crate) fn extract_found_paths(content: &str, valid: &HashSet<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for token in content.split(|c: char| {
        c.is_whitespace() || matches!(c, ',' | ';' | '(' | ')' | '[' | ']' | '|' | '"' | '\'' | '`')
    }) {
        let t = token.split(':').next().unwrap_or("");
        let t = norm_path(t);
        if valid.contains(&t) && !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

/// Диспетчер тул-вызовов Фазы 1. Никогда не паникует: любая ошибка —
/// «Tool error: …» в content (модель видит и корректируется) + лог.
pub fn execute_tool_call(
    root: &Path,
    project_paths: &[String],
    codemap_exe: &Path,
    call: &ToolCall,
) -> ToolExecOutput {
    let result = dispatch(root, project_paths, codemap_exe, call);
    match result {
        Ok((content, found)) => ToolExecOutput {
            content,
            found_paths: found,
        },
        Err(e) => {
            log::warn!("ai-pick tools: тул {} ошибся: {e}", call.name);
            ToolExecOutput {
                content: format!("Tool error: {e}"),
                found_paths: Vec::new(),
            }
        }
    }
}

fn dispatch(
    root: &Path,
    project_paths: &[String],
    codemap_exe: &Path,
    call: &ToolCall,
) -> Result<(String, Vec<String>), String> {
    let args: Value = serde_json::from_str(&call.arguments)
        .map_err(|e| format!("аргументы не разобраны ({e}): {}", call.arguments))?;
    if !args.is_object() {
        return Err(format!("аргументы не объект: {}", call.arguments));
    }
    match call.name.as_str() {
        "find_files" => {
            let (content, found) = find_files(
                root,
                project_paths,
                args.get("query").and_then(Value::as_str).unwrap_or(""),
                args.get("dir").and_then(Value::as_str),
            )?;
            Ok((content, found))
        }
        "codemap_map" => {
            let budget = match args.get("budget").and_then(Value::as_u64) {
                Some(512) => 512,
                _ => 768,
            };
            let scope = args
                .get("scope")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty() && *s != ".");
            let out = match scope {
                Some(s) => repomap::run_map_scope(root, s, codemap_exe, budget)?,
                None => repomap::run_map(root, codemap_exe, budget)?,
            };
            Ok((truncate(&out, LIMIT_MAP), extract_found_paths(&out, &valid_set(project_paths))))
        }
        "codemap_search" => {
            let out = repomap::run_search(
                root,
                codemap_exe,
                args.get("query").and_then(Value::as_str).unwrap_or(""),
            )?;
            Ok((truncate(&out, LIMIT_SHORT), extract_found_paths(&out, &valid_set(project_paths))))
        }
        "codemap_callers" => {
            let out = repomap::run_callers(
                root,
                codemap_exe,
                args.get("symbol").and_then(Value::as_str).unwrap_or(""),
            )?;
            Ok((truncate(&out, LIMIT_SHORT), extract_found_paths(&out, &valid_set(project_paths))))
        }
        "codemap_skeleton" => {
            let out = repomap::run_skeleton(
                root,
                codemap_exe,
                args.get("file").and_then(Value::as_str).unwrap_or(""),
            )?;
            Ok((truncate(&out, LIMIT_SKELETON), extract_found_paths(&out, &valid_set(project_paths))))
        }
        other => Err(format!(
            "неизвестный тул «{other}»; доступны: find_files, codemap_map, codemap_search, codemap_callers, codemap_skeleton"
        )),
    }
}

fn valid_set(project_paths: &[String]) -> HashSet<String> {
    project_paths.iter().map(|p| norm_path(p)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> Vec<String> {
        vec![
            "src/report.rs".to_string(),
            "src/csv_writer.rs".to_string(),
            "src-tauri/src/infra/session_manager.rs".to_string(),
            "src/controllers/sessions.ts".to_string(),
        ]
    }

    #[test]
    fn five_tools_with_required_args() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 5);
        let names: Vec<&str> = defs.iter().map(|d| d.function.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["find_files", "codemap_map", "codemap_search", "codemap_callers", "codemap_skeleton"]
        );
        for d in &defs {
            assert_eq!(d.r#type, "function");
            assert_eq!(d.function.parameters["additionalProperties"], false);
            let required = d.function.parameters["required"].as_array().unwrap();
            for r in required {
                assert!(
                    d.function.parameters["properties"].get(r.as_str().unwrap()).is_some(),
                    "{}: required {} отсутствует в properties",
                    d.function.name,
                    r
                );
            }
        }
    }

    /// Несуществующий root: энрич молча не сработает — пути остаются голыми
    /// (файлы не читаются, паника исключена).
    const NO_ROOT: &str = "Q:/vb-test-nonexistent-root";

    #[test]
    fn find_files_matches_case_insensitive_and_truncates() {
        let (content, found) =
            find_files(Path::new(NO_ROOT), &paths(), "CSV", None).expect("find_files ок");
        assert!(content.contains("src/csv_writer.rs"), "{content}");
        assert_eq!(found, vec!["src/csv_writer.rs"]);
    }

    #[test]
    fn find_files_respects_dir_filter() {
        let (content, found) =
            find_files(Path::new(NO_ROOT), &paths(), "session", Some("src-tauri"))
                .expect("find_files ок");
        assert!(content.contains("src-tauri/src/infra/session_manager.rs"), "{content}");
        assert_eq!(found.len(), 1);
        // другой каталог — другой хит по тому же query
        let (other, found2) = find_files(
            Path::new(NO_ROOT),
            &paths(),
            "session",
            Some("src/controllers"),
        )
        .expect("ок");
        assert!(other.contains("src/controllers/sessions.ts"), "{other}");
        assert_eq!(found2.len(), 1);
        let (empty, _) =
            find_files(Path::new(NO_ROOT), &paths(), "csv", Some("src-tauri")).expect("ок");
        assert!(empty.contains("не найдены"), "{empty}");
    }

    #[test]
    fn find_files_enriches_signatures_and_keeps_found_clean() {
        let dir = tempfile::tempdir().expect("tmpdir");
        std::fs::create_dir_all(dir.path().join("src-tauri/src/infra")).expect("mkdir");
        std::fs::write(
            dir.path().join("src-tauri/src/infra/session_manager.rs"),
            "pub struct ChatSession {\n    id: String,\n}\n\nimpl ChatSession {\n    fn save(&self) {}\n}\n",
        )
        .expect("write");
        let (content, found) = find_files(
            dir.path(),
            &paths(),
            "session",
            Some("src-tauri"),
        )
        .expect("ок");
        assert!(
            content.contains(
                "session_manager.rs  [pub struct ChatSession, impl ChatSession, fn save(&self)]"
            ),
            "{content}"
        );
        // found — чистые пути, без суффиксов (валидация/trim их парсят)
        assert_eq!(found, vec!["src-tauri/src/infra/session_manager.rs"]);
    }

    #[test]
    fn extract_found_paths_parses_enriched_lines() {
        let valid: HashSet<String> =
            ["src-tauri/src/infra/session_manager.rs"].iter().map(|s| s.to_string()).collect();
        let content =
            "src-tauri/src/infra/session_manager.rs  [pub struct ChatSession, impl ChatSession]";
        assert_eq!(
            extract_found_paths(content, &valid),
            vec!["src-tauri/src/infra/session_manager.rs"]
        );
    }

    #[test]
    fn find_files_empty_query_is_error() {
        let err = find_files(Path::new(NO_ROOT), &paths(), "  ", None).unwrap_err();
        assert!(err.contains("пустой query"), "{err}");
    }

    #[test]
    fn find_files_over_limit_appends_hint() {
        let many: Vec<String> = (0..50).map(|i| format!("src/f{i}.rs")).collect();
        let (content, found) =
            find_files(Path::new(NO_ROOT), &many, "f", None).expect("ок");
        assert_eq!(found.len(), MAX_FIND_RESULTS);
        assert!(content.contains("… ещё 10 файлов"), "{content}");
    }

    #[test]
    fn truncate_limits_by_chars_with_marker() {
        let long = "а".repeat(100);
        let t = truncate(&long, 30);
        assert_eq!(t.chars().count(), 30 + "\n… [вывод обрезан по лимиту 30 символов]".chars().count());
        assert!(t.contains("вывод обрезан"));
        assert_eq!(truncate("коротко", 30), "коротко");
    }

    #[test]
    fn extract_paths_only_known_ones() {
        let valid = valid_set(&paths());
        let out = extract_found_paths(
            "  src-tauri/src/infra/session_manager.rs:21  [def]  ChatSession\nsrc/ghost.rs:1",
            &valid,
        );
        assert_eq!(out, vec!["src-tauri/src/infra/session_manager.rs"]);
    }

    #[test]
    fn extract_paths_from_map_blocks() {
        let valid = valid_set(&paths());
        let out = extract_found_paths(
            "src/report.rs:\n   12 | fn a()\nsrc-tauri/src/infra/session_manager.rs:",
            &valid,
        );
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn unknown_tool_is_tool_error_not_panic() {
        let call = ToolCall {
            id: "t".into(),
            name: "read_body".into(),
            arguments: "{}".into(),
        };
        let out = execute_tool_call(Path::new("."), &paths(), Path::new("t.exe"), &call);
        assert!(out.content.starts_with("Tool error:"), "{}", out.content);
        assert!(out.found_paths.is_empty());
    }

    #[test]
    fn broken_arguments_are_tool_error() {
        let call = ToolCall {
            id: "t".into(),
            name: "find_files".into(),
            arguments: "не json".into(),
        };
        let out = execute_tool_call(Path::new("."), &paths(), Path::new("t.exe"), &call);
        assert!(out.content.starts_with("Tool error:"), "{}", out.content);
    }

    #[test]
    fn find_files_dispatch_works_without_subprocess() {
        let call = ToolCall {
            id: "t".into(),
            name: "find_files".into(),
            arguments: r#"{"query":"csv_writer"}"#.into(),
        };
        let out = execute_tool_call(Path::new("."), &paths(), Path::new("t.exe"), &call);
        assert!(out.content.contains("src/csv_writer.rs"), "{}", out.content);
        assert_eq!(out.found_paths, vec!["src/csv_writer.rs"]);
    }

    #[test]
    fn norm_path_unifies_separators() {
        assert_eq!(norm_path("src\\ui\\mod.rs"), "src/ui/mod.rs");
        assert_eq!(norm_path("./src/ui/mod.rs"), "src/ui/mod.rs");
    }
}
