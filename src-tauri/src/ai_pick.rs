//! AI-подбор файлов под черновик: агентный tool-loop (PRP
//! tasks/PRP_ai_pick_tool_loop.md, вердикт спеца 14.11.26).
//!
//! Фаза 1 — модель САМА исследует проект: до 6 ходов с нативным tool-calling
//! (find_files + codemap_* из `ai_pick_tools.rs`); ход 1 с `tool_choice=
//! "required"` (смоук: с "auto" gemma-4-12B пассивна), далее "auto".
//! Фаза 2 — строгий JSON через json_schema-грамматику (`set_tools(None)` +
//! `set_grammar`) и user-инструкция.
//!
//! Контекст/VRAM: один llama-server на весь подбор (n_ctx 8192), append-only
//! история, стоп-кран SAFE_LOOP_CEILING по хостовому счёту токенов
//! (смоук-тест: metrics.prompt_tokens движка — инкрементальные), аварийный
//! trim старейшего tool-результата. Движок спавнится под каждый запрос и
//! убивается по `Drop` — зомби-процессов не остаётся.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use agent_core::config;
use agent_core::tree::repomap;
use serde_json::json;
use tauri_plugin_llama_engine::engine::config::{load_config_early, EngineConfig, ModelParams};
use tauri_plugin_llama_engine::engine::llamacpp_installer;
use tauri_plugin_llama_engine::engine::llm::LlamaEngine;
use tauri_plugin_llama_engine::engine::llm_types::{
    GenerationResult, GrammarSpec, LlmMessage, ToolCall,
};

use crate::ai_pick_prompt as prompt;
use crate::ai_pick_tools::{execute_tool_call, norm_path, tool_definitions, ToolExecOutput};

/// Контекст задачи: луп с 5 тулами + pre-seed карта + JSON — 8k по расчёту
/// спеца (старт ~2100, потолок Фазы 1 7168, резерв Фазы 2 1024).
const AI_CTX_LIMIT: u32 = 8192;

/// Ответ (JSON в Фазе 2, текст+`<|tool_call>` в Фазе 1) — 512 с запасом.
const MAX_GEN_TOKENS: usize = 512;

/// Максимум ходов Фазы 1 (вердикт спеца: 12B зацикливается — лимит обязателен).
const MAX_TOOL_ROUNDS: usize = 6;

/// Глобальный таймаут Фазы 1 (все ходы + subprocess); после — Фаза 2.
const PHASE1_GLOBAL_TIMEOUT: Duration = Duration::from_secs(120);

/// Бюджет pre-seed карты (хост строит её сам до первого хода — экономит ход).
const PRESEED_MAP_BUDGET: usize = 768;

/// Больше файлов — pre-seed карта 768 это шум из 13 случайных файлов
/// (прогон №1 Теста B): вместо неё сводка каталогов 2 уровней (Доработка №2
/// PRP storage-link).
const PRESEED_MAP_FILE_THRESHOLD: usize = 50;

/// Проект больше этого числа файлов — спец требует обязательного вызова
/// инструментов перед финалом (phase1_system(require_tools)).
const MANY_FILES_THRESHOLD: usize = 30;

/// Ранний natural-stop на большом проекте: пинок «продолжить исследование»
/// (12B торопится в финал, промпт-минимумы игнорирует); форсирует required
/// на следующий ход; максимум 2 пинка (в лимите 6 ходов).
const NUDGE_AFTER_TOOL_ROUNDS: u32 = 3;
const NUDGE_MAX: u32 = 2;

/// Итог подбора: проверенные пути (валидация — тут, по списку скана UI) и
/// reasoning для строки статуса.
pub struct AiPickOutcome {
    pub selected: Vec<String>,
    pub reasoning: String,
}

/// Модель: явный выбор в конфиге VibeBridge или первая существующая из
/// каталога движка. Ошибки — явные (молчаливых подмен нет).
fn resolve_model(cfg: &config::AppConfig, engine_cfg: &EngineConfig) -> Result<String, String> {
    if let Some(p) = &cfg.ai_model_path {
        if Path::new(p).is_file() {
            return Ok(p.clone());
        }
        return Err(format!("ai_model_path из config.json не найден: {p}"));
    }
    engine_cfg
        .models
        .iter()
        .find(|m| Path::new(m).is_file())
        .cloned()
        .ok_or_else(|| {
            "модель не найдена: каталог движка llama-engine пуст — добавь модель или укажи ai_model_path в config.json".to_string()
        })
}

fn user_msg(content: String) -> LlmMessage {
    LlmMessage { role: "user".to_string(), content, ..Default::default() }
}

fn system_msg(content: String) -> LlmMessage {
    LlmMessage { role: "system".to_string(), content, ..Default::default() }
}

fn assistant_msg(content: String) -> LlmMessage {
    LlmMessage { role: "assistant".to_string(), content, ..Default::default() }
}

/// Полный пайплайн подбора. Каждая ошибка — Err с текстом для юзера
/// (логируется вызывающей стороной); статусы прогресса — через `status`.
/// `project_paths` — относительные пути selectable-файлов из скана UI
/// (источник истинности для find_files и анти-галлюцинации).
/// Блокирующая функция — вызывается только из воркера.
pub fn pick_files(
    root: &Path,
    base: &Path,
    project_paths: &[String],
    draft: &str,
    cancel: Arc<AtomicBool>,
    status: &dyn Fn(String),
) -> Result<AiPickOutcome, String> {
    // 1. Конфиг VibeBridge: тул карты + переопределение модели.
    let cfg = config::load(base);
    let tool = cfg
        .repomap_tool
        .clone()
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            log::warn!("repomap_tool не задан — дефолт {}", repomap::DEFAULT_TOOL);
            PathBuf::from(repomap::DEFAULT_TOOL)
        });

    // 2. Pre-seed: ≤50 файлов — карта 768 (покрывает весь проект); >50 —
    //    сводка каталогов 2 уровней из скана UI (карта была бы шумом).
    status("строю карту проекта…".to_string());
    let map = if project_paths.len() > PRESEED_MAP_FILE_THRESHOLD {
        let s = repomap::dir_summary_2l(project_paths);
        log::info!(
            "ai-pick: сводка каталогов {} символов ({} файлов — карта 768 была бы шумом)",
            s.len(),
            project_paths.len()
        );
        s
    } else {
        let m = repomap::run_map(root, &tool, PRESEED_MAP_BUDGET)?;
        log::info!("ai-pick: pre-seed карта {} символов", m.len());
        m
    };

    // 3. Модель: ai_model_path → первая из каталога движка.
    status("ищу модель…".to_string());
    let engine_cfg = load_config_early();
    let model = resolve_model(&cfg, &engine_cfg)?;
    log::info!("ai-pick: модель {model}");

    // 4. Движок: спавн llama-server + ожидание /health (первая загрузка
    //    модели в VRAM — до ~30 с; health-таймаут движка — 120 с).
    status("загружаю модель… (первый запуск может занять полминуты)".to_string());
    let engine_dir = match &engine_cfg.llamacpp_dir {
        Some(dir) => PathBuf::from(dir),
        None => {
            let exe_dir = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(Path::to_path_buf))
                .ok_or_else(|| "не удалось определить папку exe (current_exe)".to_string())?;
            llamacpp_installer::default_dir(&exe_dir)
        }
    };
    let engine = LlamaEngine::new(
        &engine_dir,
        &model,
        AI_CTX_LIMIT,
        false,
        false,
        0, // reasoning_budget: подбору думать не нужно — без --reasoning-флагов
        |s: String| log::info!("[llama] {s}"),
        |_s: String| {},
    )?;
    log::info!("ai-pick: движок запущен (ctx {AI_CTX_LIMIT})");

    // 5. Луп: Фаза 1 (исследование тулами) → Фаза 2 (строгий JSON).
    let outcome = run_tool_loop(&engine, root, &tool, project_paths, draft, &map, cancel, status);

    // Сервер убит при любом исходе (Drop → kill_process_tree + Job Object).
    drop(engine);
    outcome
}

/// Фазы 1+2. Единственная мутация истории — append; исключение — аварийный
/// trim по переполнению (в `generate_step`).
fn run_tool_loop(
    engine: &LlamaEngine,
    root: &Path,
    tool: &Path,
    project_paths: &[String],
    draft: &str,
    map: &str,
    cancel: Arc<AtomicBool>,
    status: &dyn Fn(String),
) -> Result<AiPickOutcome, String> {
    let valid: HashSet<String> = project_paths.iter().map(|p| norm_path(p)).collect();
    let require_tools = project_paths.len() > MANY_FILES_THRESHOLD;
    let mut messages = vec![
        system_msg(prompt::phase1_system(require_tools)),
        user_msg(prompt::user_seed(map, draft)),
    ];
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut dup_hits = 0u32;
    let mut natural = false;
    let mut tool_rounds = 0u32;
    let mut nudged = 0u32;
    let mut force_next = false; // после пинка следующий ход — "required"
    let started = Instant::now();

    // ── Фаза 1: исследование ────────────────────────────────────────────────
    for round in 0..MAX_TOOL_ROUNDS {
        if cancel.load(Ordering::Relaxed) {
            return Err("выбор файлов отменён".to_string());
        }
        if started.elapsed() >= PHASE1_GLOBAL_TIMEOUT {
            log::warn!("ai-pick: глобальный таймаут Фазы 1 ({} с) — финальный JSON", PHASE1_GLOBAL_TIMEOUT.as_secs());
            break;
        }
        // Смоук 14.11.26: с "auto" на первом ходу модель пассивна — "required".
        // Пинок тоже форсирует вызов: текстовое «продолжение» без тулов
        // моделью уже показано (прогон 4 Теста B).
        let choice = if round == 0 || force_next { "required" } else { "auto" };
        force_next = false;
        let prepare = || engine.set_tools(Some(tool_definitions()));
        let generated = generate_step(engine, &mut messages, Some(choice), &cancel, status, &valid, &prepare)?;
        if generated.stop_reason == "CANCELLED" {
            return Err("выбор файлов отменён".to_string());
        }
        if generated.tool_calls.is_empty() {
            // Естественная остановка. На большом проекте текст до минимального
            // числа тул-ходов — торопливость: один пинок продолжить (ровно
            // один — второй ранний финал принимаем как есть).
            if require_tools
                && tool_rounds < NUDGE_AFTER_TOOL_ROUNDS + nudged
                && nudged < NUDGE_MAX
                && round + 1 < MAX_TOOL_ROUNDS
            {
                nudged += 1;
                log::info!(
                    "ai-pick: ранний финал после {tool_rounds} тул-ходов — пинок {nudged} (required) продолжить исследование"
                );
                messages.push(assistant_msg(generated.text));
                messages.push(user_msg(prompt::research_nudge()));
                force_next = true;
                continue;
            }
            // Естественная остановка: текст модели сохраняем в истории.
            messages.push(assistant_msg(generated.text));
            natural = true;
            break;
        }
        messages.push(
            assistant_msg(generated.text).with_tool_calls(generated.tool_calls.iter().map(|c| {
                json!({ "id": c.id, "type": "function",
                        "function": { "name": c.name, "arguments": c.arguments } })
            }).collect()),
        );
        // Выполняем ВСЕ вызовы хода (каждому нужен свой tool-результат).
        let mut new_chars = 0usize;
        for call in &generated.tool_calls {
            if cancel.load(Ordering::Relaxed) {
                return Err("выбор файлов отменён".to_string());
            }
            let key = canonical_call_key(call);
            let is_dup = seen.contains(&key);
            seen.insert(key);
            let out = if is_dup {
                dup_hits += 1;
                ToolExecOutput {
                    content: "Этот вызов уже выполнялся выше. Выбери другой инструмент или аргументы, либо переходи к финальному ответу.".to_string(),
                    found_paths: Vec::new(),
                }
            } else {
                status(format!(
                    "шаг {}/{}: {} {}",
                    round + 1,
                    MAX_TOOL_ROUNDS,
                    call.name,
                    short_args(call)
                ));
                execute_tool_call(root, project_paths, tool, call)
            };
            log::info!(
                "ai-pick: ход {} тул {} → {} симв., путей {}",
                round + 1,
                call.name,
                out.content.chars().count(),
                out.found_paths.len()
            );
            new_chars += out.content.chars().count();
            messages.push(
                LlmMessage::default().as_tool_result(call.id.clone(), out.content),
            );
        }
        tool_rounds += 1;

        // Стоп-кран: полный промпт считает хост (смоук: metrics.prompt_tokens
        // движка — инкрементальные, не годятся для бюджета).
        let projected = prompt::count_history_tokens(&messages);
        let _ = new_chars; // оценка уже внутри projected; chars — для лога
        log::info!("ai-pick: ход {}: контекст ~{} токенов (потолок {})", round + 1, projected, prompt::SAFE_LOOP_CEILING);
        if dup_hits >= 2 || projected >= prompt::SAFE_LOOP_CEILING {
            break;
        }
        if projected >= prompt::TRIM_TRIGGER {
            prompt::trim_oldest_tool_result(&mut messages, &valid);
        }
        if round + 1 >= MAX_TOOL_ROUNDS {
            break;
        }
    }
    if !natural {
        // Принудительная остановка: неполное не докидываем.
        drop_dangling_tool_calls(&mut messages);
        messages.push(user_msg(prompt::phase2_user_forced()));
    } else {
        messages.push(user_msg(prompt::phase2_user_natural()));
    }

    // ── Фаза 2: строгий JSON ────────────────────────────────────────────────
    status("формирую итоговый JSON…".to_string());
    let mut answer = generate_final_answer(engine, &mut messages, &cancel, status, &valid)?;
    let (mut ok_paths, mut phantom) = validate_selection(&answer.selected_files, &valid);
    if !phantom.is_empty() && (ok_paths.is_empty() || phantom.len() > ok_paths.len()) {
        // Ретрай РОВНО 1 раз (вердикт спеца): фантомы и валидных почти нет.
        log::warn!("ai-pick: фантомные пути {phantom:?} — ретрай 1");
        status("часть путей не существует — переспрашиваю модель…".to_string());
        messages.push(user_msg(prompt::retry_instruction(&phantom)));
        answer = generate_final_answer(engine, &mut messages, &cancel, status, &valid)?;
        (ok_paths, phantom) = validate_selection(&answer.selected_files, &valid);
    }
    let (selected, phantom_count) = (ok_paths, phantom.len());
    log::info!(
        "ai-pick: выбрано {} файлов (фантомы отброшены: {}); reasoning: {}",
        selected.len(),
        phantom_count,
        answer.reasoning
    );
    Ok(AiPickOutcome { selected, reasoning: answer.reasoning })
}

/// generate_chat с одним аварийным trim при переполнении контекста.
/// `prepare` вызывается перед КАЖДОЙ попыткой: set_tools/set_grammar —
/// consume-and-clear, их нужно ставить заново на каждый вызов.
#[allow(clippy::too_many_arguments)]
fn generate_step(
    engine: &LlamaEngine,
    messages: &mut Vec<LlmMessage>,
    tool_choice: Option<&str>,
    cancel: &Arc<AtomicBool>,
    status: &dyn Fn(String),
    valid: &HashSet<String>,
    prepare: &dyn Fn(),
) -> Result<GenerationResult, String> {
    let params = ModelParams { temperature: 0.1, ..Default::default() };
    let mut overflow_err = String::new();
    for attempt in 0..2 {
        prepare();
        let res = engine.generate_chat(
            messages,
            MAX_GEN_TOKENS,
            &params,
            "auto",
            true, // disable_reasoning: мыслительный блок подавляется движком
            cancel.clone(),
            "ai-pick",
            tool_choice,
            |_f: f32, _s: &str| {},
            |s: String| log::info!("[llama] {s}"),
        );
        match res {
            Ok(r) => return Ok(r),
            Err(e) => {
                if attempt == 0 && prompt::looks_like_overflow(&e) {
                    log::warn!("ai-pick: переполнение контекста ({e}) — trim и ретрай");
                    status("контекст у потолка — сокращаю старые результаты тула…".to_string());
                    prompt::trim_oldest_tool_result(messages, valid);
                    overflow_err = e;
                } else {
                    return Err(e);
                }
            }
        }
    }
    Err(overflow_err)
}

/// Фаза 2: грамматика answer_schema → ответ → разбор PickAnswer.
fn generate_final_answer(
    engine: &LlamaEngine,
    messages: &mut Vec<LlmMessage>,
    cancel: &Arc<AtomicBool>,
    status: &dyn Fn(String),
    valid: &HashSet<String>,
) -> Result<prompt::PickAnswer, String> {
    let prepare = || {
        engine.set_grammar(Some(GrammarSpec {
            gbnf: None,
            json_schema: Some(prompt::answer_schema()),
        }));
    };
    let generated = generate_step(engine, messages, None, &cancel, status, valid, &prepare)?;
    if generated.stop_reason == "CANCELLED" {
        return Err("выбор файлов отменён".to_string());
    }
    log::info!("ai-pick: JSON {} символов (stop: {})", generated.text.len(), generated.stop_reason);
    prompt::parse_answer(&generated.text)
}

/// Ключ анти-лупа: имя тула + канонический JSON аргументов
/// (serde_json Value сериализуется с отсортированными ключами).
fn canonical_call_key(call: &ToolCall) -> (String, String) {
    let canonical = serde_json::from_str::<serde_json::Value>(&call.arguments)
        .map(|v| v.to_string())
        .unwrap_or_else(|_| call.arguments.trim().to_string());
    (call.name.clone(), canonical)
}

/// Валидация выбора: нормализация, отброс фантомов, дедуп с сохранением
/// порядка. Возвращает (валидные, фантомы-как-их-дала-модель).
fn validate_selection(selected: &[String], valid: &HashSet<String>) -> (Vec<String>, Vec<String>) {
    let mut kept: Vec<String> = Vec::new();
    let mut phantom: Vec<String> = Vec::new();
    for raw in selected {
        let rel = norm_path(raw);
        if rel.is_empty() {
            continue;
        }
        if valid.contains(&rel) {
            if !kept.contains(&rel) {
                kept.push(rel);
            }
        } else {
            phantom.push(raw.clone());
        }
    }
    (kept, phantom)
}

/// Хвост assistant с tool_calls без результатов ломает формат (OpenAI:
/// tool_calls обязан сопровождаться tool-результатом) — отбросить.
fn drop_dangling_tool_calls(messages: &mut Vec<LlmMessage>) {
    let dangling = messages
        .last()
        .is_some_and(|m| m.role == "assistant" && m.tool_calls.as_ref().is_some_and(|t| !t.is_empty()));
    if dangling {
        log::warn!("ai-pick: assistant с tool_calls без результатов отброшен (принудительная остановка)");
        messages.pop();
    }
}

fn short_args(call: &ToolCall) -> String {
    let a = call.arguments.trim();
    if a.chars().count() <= 48 {
        a.to_string()
    } else {
        format!("{}…", a.chars().take(48).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_key_normalizes_argument_order() {
        let c1 = ToolCall { id: "1".into(), name: "find_files".into(), arguments: r#"{"query":"csv","dir":"src"}"#.into() };
        let c2 = ToolCall { id: "2".into(), name: "find_files".into(), arguments: r#"{ "dir" : "src", "query" : "csv" }"#.into() };
        assert_eq!(canonical_call_key(&c1), canonical_call_key(&c2));
        let c3 = ToolCall { id: "3".into(), name: "find_files".into(), arguments: r#"{"query":"csv"}"#.into() };
        assert_ne!(canonical_call_key(&c1), canonical_call_key(&c3));
    }

    #[test]
    fn validation_normalizes_dedups_and_separates_phantoms() {
        let valid: HashSet<String> = ["src/a.rs", "src/b.rs"].iter().map(|s| s.to_string()).collect();
        let sel = vec!["src\\a.rs".to_string(), "./src/a.rs".to_string(), "src/ghost.rs".to_string()];
        let (kept, phantom) = validate_selection(&sel, &valid);
        assert_eq!(kept, vec!["src/a.rs"]);
        assert_eq!(phantom, vec!["src/ghost.rs"]);
    }

    #[test]
    fn dangling_tool_calls_are_dropped() {
        let mut msgs = vec![
            system_msg("с".into()),
            user_msg("у".into()),
            assistant_msg(String::new()).with_tool_calls(vec![json!({"id": "t"})]),
        ];
        drop_dangling_tool_calls(&mut msgs);
        assert_eq!(msgs.len(), 2);
        // хвост с результатом не трогается
        let mut ok = vec![user_msg("у".into()), LlmMessage::default().as_tool_result("t".to_string(), "р")];
        drop_dangling_tool_calls(&mut ok);
        assert_eq!(ok.len(), 2);
    }
}
