//! «Карта проекта» для AI-подбора файлов: вызов внешнего тул-бинаря
//! `agent-tools.exe` (команда `codemap`) и возврат его stdout как текста.
//!
//! Решение владельца 13.11.26: карта — готовый AST-скелет от codemap, здесь
//! тул используется как процесс (stdout наружу, в структуры не разбирается).
//!
//! Обязательные правила вызова:
//! - `--no-cache` — codemap без него пишет `.codemap/` в сканируемый проект и
//!   дописывает `.git/info/exclude`; в чужой проект писать ничего нельзя
//!   (skeleton/body по контракту на диск не пишут — флаг им не нужен).
//! - `--tokens 2048` максимум — CLI молча обрезает вывод на 12000 символов.
//! - `CREATE_NO_WINDOW` — консоль не мелькает (правило владельца).
//!
//! Tool-loop 14.11.26 (PRP tasks/PRP_ai_pick_tool_loop.md): помимо `build`
//! добавлены тонкие обёртки `run_map/run_map_scope/run_search/run_callers/
//! run_skeleton` — их вызывает модель через тула ai_pick_tools.
//! Подтверждённая мина: `codemap map --path <корень>/подкаталог` печатает пути
//! БЕЗ префикса подкаталога → `run_map_scope` восстанавливает префикс
//! (`prefix_map_paths`); search/callers/skeleton пути всегда от корня.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Бюджет карты в токенах one-shot пайплайна (лимит CLI: вывод >12000 символов
/// обрезается молча).
pub const MAP_TOKEN_BUDGET: usize = 2048;

/// Таймаут построения карты one-shot пайплайна; после него процесс убивается.
const TIMEOUT: Duration = Duration::from_secs(60);

/// Таймаут одного тул-вызова модели в tool-loop (вердикт спеца: 15 с).
pub const TOOL_TIMEOUT: Duration = Duration::from_secs(15);

/// Путь к тул-бинарю по умолчанию (машина владельца); переопределяется
/// ключом `repomap_tool` в config.json.
pub const DEFAULT_TOOL: &str = "D:\\Projects\\my-tauri-plugins\\bin\\agent-tools.exe";

// ---------------------------------------------------------------------------
// Сборка аргументов (чистые функции — тестируются без запуска процессов).
// ---------------------------------------------------------------------------

pub(crate) fn args_map(root: &str, budget_tokens: usize) -> Vec<String> {
    vec![
        "map".to_string(),
        "--path".to_string(),
        root.to_string(),
        "--no-cache".to_string(),
        "--tokens".to_string(),
        budget_tokens.to_string(),
    ]
}

pub(crate) fn args_search(root: &str, query: &str) -> Vec<String> {
    vec![
        "search".to_string(),
        query.to_string(),
        "--path".to_string(),
        root.to_string(),
        "--no-cache".to_string(),
    ]
}

pub(crate) fn args_callers(root: &str, symbol: &str) -> Vec<String> {
    vec![
        "callers".to_string(),
        symbol.to_string(),
        "--path".to_string(),
        root.to_string(),
        "--no-cache".to_string(),
    ]
}

pub(crate) fn args_skeleton(root: &str, file_rel: &str) -> Vec<String> {
    vec![
        "skeleton".to_string(),
        file_rel.to_string(),
        "--path".to_string(),
        root.to_string(),
    ]
}

/// Запрет выхода за корень проекта (правило как в аплаере патчей: `..` и
/// абсолютные пути — ошибка). Аргументы модели не доверенные.
fn ensure_within_root(rel: &str) -> Result<(), String> {
    let norm = rel.replace('\\', "/");
    if norm.starts_with('/') || norm.split('/').any(|seg| seg == "..") {
        return Err(format!("путь вне корня проекта запрещён: {rel}"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Запуск тул-процесса.
// ---------------------------------------------------------------------------

/// Собрать команду codemap (CREATE_NO_WINDOW — правило владельца).
fn codemap_command(tool_path: &Path, args: &[String]) -> Command {
    let mut cmd = Command::new(tool_path);
    cmd.arg("codemap");
    for a in args {
        cmd.arg(a);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd
}

/// Запустить codemap с готовыми аргументами и вернуть stdout.
///
/// Вывод CLI гарантированно ≤12000 символов (обрезка на стороне тул-процесса)
/// — пайпы читаются уже после завершения, дедлока быть не может.
/// Ошибки — наружу (никаких молчаливых подмен); stderr тул-процесса — в лог.
fn run_codemap(tool_path: &Path, args: &[String], timeout: Duration) -> Result<String, String> {
    if !tool_path.is_file() {
        return Err(format!(
            "тул-бинар не найден: {} — укажи путь в config.json → repomap_tool",
            tool_path.display()
        ));
    }
    let started = Instant::now();
    let mut child = codemap_command(tool_path, args)
        .spawn()
        .map_err(|e| format!("запуск {}: {e}", tool_path.display()))?;

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if started.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    log::error!("repomap: таймаут {} с — процесс убит", timeout.as_secs());
                    return Err(format!("codemap: таймаут {} с", timeout.as_secs()));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(format!("ожидание {}: {e}", tool_path.display())),
        }
    };

    let mut stdout = String::new();
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stdout.take() {
        use std::io::Read as _;
        let _ = pipe.read_to_string(&mut stdout);
    }
    if let Some(mut pipe) = child.stderr.take() {
        use std::io::Read as _;
        let _ = pipe.read_to_string(&mut stderr);
    }
    if !stderr.trim().is_empty() {
        log::warn!("repomap: stderr тул-процесса: {}", stderr.trim());
    }

    if !status.success() {
        log::error!(
            "repomap: код выхода {:?}, stderr: {}",
            status.code(),
            stderr.trim()
        );
        return Err(format!(
            "codemap: код выхода {:?} (подробности в логе)",
            status.code()
        ));
    }
    let map = stdout.trim().to_string();
    if map.is_empty() {
        return Err("codemap: пустой вывод тул-процесса".to_string());
    }
    Ok(map)
}

// ---------------------------------------------------------------------------
// Публичные обёртки (использует ai_pick / ai_pick_tools).
// ---------------------------------------------------------------------------

/// One-shot карта корня (старый контракт ai_pick 13.11.26, бюджет 2048).
pub fn build(root: &Path, tool_path: &Path) -> Result<String, String> {
    if !root.is_dir() {
        return Err(format!("карта проекта: папка не найдена {}", root.display()));
    }
    run_codemap(
        tool_path,
        &args_map(&root.to_string_lossy(), MAP_TOKEN_BUDGET),
        TIMEOUT,
    )
}

/// Карта корня с бюджетом токенов (pre-seed и тул codemap_map в tool-loop).
pub fn run_map(root: &Path, tool_path: &Path, budget_tokens: usize) -> Result<String, String> {
    run_codemap(
        tool_path,
        &args_map(&root.to_string_lossy(), budget_tokens),
        TOOL_TIMEOUT,
    )
}

/// Карта подкаталога. codemap печатает пути БЕЗ префикса scope (мина,
/// подтверждённая замером 14.11.26) — префикс восстанавливается перед
/// возвратом: модель всегда видит пути от корня проекта.
pub fn run_map_scope(
    root: &Path,
    scope_rel: &str,
    tool_path: &Path,
    budget_tokens: usize,
) -> Result<String, String> {
    ensure_within_root(scope_rel)?;
    let scope_abs = root.join(scope_rel.replace('\\', "/"));
    if !scope_abs.is_dir() {
        return Err(format!("scope не найден: {scope_rel}"));
    }
    let out = run_codemap(
        tool_path,
        &args_map(&scope_abs.to_string_lossy(), budget_tokens),
        TOOL_TIMEOUT,
    )?;
    Ok(prefix_map_paths(&out, scope_rel))
}

/// Поиск символа/текста по карте (пути в выводе — от корня).
pub fn run_search(root: &Path, tool_path: &Path, query: &str) -> Result<String, String> {
    run_codemap(
        tool_path,
        &args_search(&root.to_string_lossy(), query),
        TOOL_TIMEOUT,
    )
}

/// Кто ссылается на символ (пути в выводе — от корня).
pub fn run_callers(root: &Path, tool_path: &Path, symbol: &str) -> Result<String, String> {
    run_codemap(
        tool_path,
        &args_callers(&root.to_string_lossy(), symbol),
        TOOL_TIMEOUT,
    )
}

/// Скелет одного файла (импорты/сигнатуры, без тел) по пути от корня.
pub fn run_skeleton(root: &Path, tool_path: &Path, file_rel: &str) -> Result<String, String> {
    ensure_within_root(file_rel)?;
    let file_abs = root.join(file_rel.replace('\\', "/"));
    if !file_abs.is_file() {
        return Err(format!("файл не найден: {file_rel}"));
    }
    run_codemap(
        tool_path,
        &args_skeleton(&root.to_string_lossy(), &file_rel.replace('\\', "/")),
        TOOL_TIMEOUT,
    )
}

// ---------------------------------------------------------------------------
// Восстановление префикса путей в карте подкаталога.
// ---------------------------------------------------------------------------

/// Строка-блок файла вида `путь/файл.ext:` (начало секции реального кода).
/// Контент-строки начинаются с отступа, заголовки — с `#`/`>`/`(`.
fn file_block_path(line: &str) -> Option<&str> {
    if !line.ends_with(':')
        || line.starts_with(' ')
        || line.starts_with('#')
        || line.starts_with('(')
    {
        return None;
    }
    let path = &line[..line.len() - 1];
    if path.is_empty() || path.contains(' ') || path.contains(" — ") {
        return None;
    }
    let name = path.rsplit('/').next()?;
    // последний сегмент обязан иметь расширение (permissions.rs, tools/media.rs)
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => Some(path),
        _ => None,
    }
}

/// Каталог из структурной строки вида `каталог — N ф., ~Xk: файлы…`.
fn structure_dir_token(line: &str) -> Option<&str> {
    if line.starts_with([' ', '#', '>', '(']) {
        return None;
    }
    let idx = line.find(" — ")?;
    let dir = &line[..idx];
    if dir.is_empty() || dir.contains(' ') {
        return None;
    }
    // после « — » обязано идти «N ф.» — отсекает случайные совпадения в контенте
    let after = line[idx + " — ".len()..].trim_start();
    let (num, tail) = after.split_once(' ')?;
    if num.is_empty() || !num.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if !tail.trim_start().starts_with("ф.") {
        return None;
    }
    Some(dir)
}

/// Дописывает префикс `scope/` путям в выводе `codemap map --path <root>/<scope>`.
/// Обрабатываются строки-блоки файлов (`путь:`) и строки структуры
/// (`каталог — N ф., …`); каталог `.` структуры заменяется на сам scope.
pub fn prefix_map_paths(output: &str, scope: &str) -> String {
    let scope = scope.replace('\\', "/");
    let scope = scope.trim_matches('/').to_string();
    if scope.is_empty() || scope == "." {
        return output.to_string();
    }
    let prefixed = output
        .lines()
        .map(|line| {
            if file_block_path(line).is_some() {
                format!("{scope}/{line}")
            } else if let Some(dir) = structure_dir_token(line) {
                let rest = &line[dir.len()..];
                if dir == "." {
                    format!("{scope}{rest}")
                } else {
                    format!("{scope}/{dir}{rest}")
                }
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    prefixed
}

/// Сводка каталогов до 2-го уровня с числом файлов (Доработка №2 PRP
/// tasks/PRP_ai_pick_storage_link.md): на проектах >50 файлов pre-seed карта
/// 768 — шум из случайных файлов; вместо неё — компактная навигация по слоям
/// (~150–250 токенов) из списка путей скана UI, БЕЗ диска и БЕЗ codemap.
pub fn dir_summary_2l(project_paths: &[String]) -> String {
    use std::collections::BTreeMap;
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for p in project_paths {
        let norm = p.replace('\\', "/");
        let parts: Vec<&str> = norm.split('/').collect();
        // До 2 компонентов пути; файлы в корне — группа «(корень)».
        let key = if parts.len() > 2 {
            format!("{}/{}", parts[0], parts[1])
        } else if parts.len() == 2 {
            parts[0].to_string()
        } else {
            "(корень)".to_string()
        };
        *counts.entry(key).or_insert(0) += 1;
    }
    let mut rows: Vec<(String, usize)> = counts.into_iter().collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut out = format!(
        "В проекте {} файлов в {} каталогах (полный список файлов — тул find_files, карта каталога — codemap_map со scope):",
        project_paths.len(),
        rows.len()
    );
    for (dir, n) in rows {
        out.push_str(&format!("\n{dir}/ ({n})"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_map_is_complete() {
        assert_eq!(
            args_map("D:/proj", 768),
            vec!["map", "--path", "D:/proj", "--no-cache", "--tokens", "768"]
        );
    }

    #[test]
    fn args_search_callers_skeleton_are_complete() {
        assert_eq!(
            args_search("D:/proj", "ChatSession"),
            vec!["search", "ChatSession", "--path", "D:/proj", "--no-cache"]
        );
        assert_eq!(
            args_callers("D:/proj", "rename_session"),
            vec!["callers", "rename_session", "--path", "D:/proj", "--no-cache"]
        );
        assert_eq!(
            args_skeleton("D:/proj", "src/app.ts"),
            vec!["skeleton", "src/app.ts", "--path", "D:/proj"]
        );
    }

    #[test]
    fn traversal_out_of_root_is_rejected() {
        assert!(ensure_within_root("src/app.ts").is_ok());
        assert!(ensure_within_root("src\\app.ts").is_ok());
        assert!(ensure_within_root("../outside.rs").is_err());
        assert!(ensure_within_root("src/../../outside.rs").is_err());
        assert!(ensure_within_root("/abs/path.rs").is_err());
    }

    /// Формат подтверждён реальным выводом codemap (king_orch_3, 14.11.26).
    const SCOPE_MAP_SAMPLE: &str = "\
# Карта проекта: D:/Projects/king_orch_3/src-tauri/src/infra

> 2026-11-14 11:25:01 | файлов: 32 | разобрано: 32 | символов: 870 | токенов: ~139777

## 📁 Структура проекта
tools — 15 ф., ~76k: media.rs, ffmpeg.rs, video.rs …+10
. — 15 ф., ~43k: permissions.rs, config.rs, event_bus.rs …+10
… ещё 1 каталогов
(игнорируется: tools/.codemap)
permissions.rs:
   70 |
   71 | impl PermissionApprover {
tools/media.rs:
   39 |
   40 | impl ImageArtifactRegistry {
";

    #[test]
    fn scope_prefix_restores_full_paths() {
        let out = prefix_map_paths(SCOPE_MAP_SAMPLE, "src-tauri/src/infra");
        assert!(out.contains("src-tauri/src/infra/permissions.rs:"), "{out}");
        assert!(out.contains("src-tauri/src/infra/tools/media.rs:"), "{out}");
        // структурные строки получили префикс
        assert!(out.contains("src-tauri/src/infra/tools — 15 ф.,"), "{out}");
        assert!(out.contains("src-tauri/src/infra — 15 ф., ~43k: permissions.rs"), "{out}");
        // контент и заголовки не тронуты
        assert!(out.contains("   71 | impl PermissionApprover {"), "{out}");
        assert!(out.contains("# Карта проекта: D:/Projects/king_orch_3"), "{out}");
        assert!(out.contains("(игнорируется: tools/.codemap)"), "{out}");
    }

    #[test]
    fn scope_prefix_is_noop_for_root() {
        assert_eq!(prefix_map_paths("a.rs:\n", ""), "a.rs:\n");
        assert_eq!(prefix_map_paths("a.rs:\n", "."), "a.rs:\n");
    }

    #[test]
    fn missing_tool_is_a_clear_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = build(dir.path(), &dir.path().join("no-such-tool.exe")).unwrap_err();
        assert!(err.contains("тул-бинар не найден"), "{err}");
        assert!(err.contains("repomap_tool"), "{err}");
    }

    #[test]
    fn missing_root_is_a_clear_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = build(&dir.path().join("no-such-dir"), Path::new("t.exe")).unwrap_err();
        assert!(err.contains("папка не найдена"), "{err}");
    }

    #[test]
    fn missing_scope_is_a_clear_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = run_map_scope(
            dir.path(),
            "no/such/dir",
            Path::new("D:/Projects/my-tauri-plugins/bin/agent-tools.exe"),
            512,
        )
        .unwrap_err();
        assert!(err.contains("scope не найден"), "{err}");
    }

    #[test]
    fn dir_summary_counts_and_sorts() {
        // виндовые разделители (последний элемент) нормализуются
        let paths: Vec<String> = [
            "src/controllers/sessions.ts", "src/controllers/tabs.ts", "src/services/SessionService.ts",
            "src-tauri/src/api/sessions.rs", "src-tauri/src/infra/session_manager.rs",
            "src-tauri/src/infra/messages.rs", "src-tauri/src/infra/permissions.rs",
            "Cargo.toml", "src\\backslash\\norm.rs",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let s = dir_summary_2l(&paths);
        assert!(s.contains("9 файлов"), "{s}");
        assert!(s.contains("src-tauri/src/ (4)"), "{s}");
        assert!(s.contains("src/controllers/ (2)"), "{s}");
        assert!(s.contains("src/services/ (1)"), "{s}");
        // сортировка по убыванию: самый крупный каталог раньше
        assert!(
            s.find("src-tauri/src/").unwrap() < s.find("src/controllers/").unwrap(),
            "{s}"
        );
    }
}
