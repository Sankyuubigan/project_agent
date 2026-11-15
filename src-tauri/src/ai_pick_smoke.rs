//! Смоук-тест нативного tool-calling (Шаг 0 [PRP tasks/PRP_ai_pick_tool_loop.md]).
//! Живая модель (gemma-4-12B) + llama-server: проверяем два риска, названных
//! архитектором:
//!   1) `<|tool_call>` Gemma 4 парсится движком в `GenerationResult.tool_calls`,
//!      а не сваливается в `text`;
//!   2) plain-text `as_tool_result` на ходе 2 не роняет llama-server
//!      (HTTP 400/500); при падении — ретрай с JSON-обёрткой `{"output": …}`.
//!
//! Запуск (разрешение владельца 14.11.26):
//!   cargo test --bin vibebridge -- --ignored ai_pick_smoke_tools --nocapture
//! Модель: env `AIPICK_TEST_MODELS` (первая до `;`) или дефолт (gemma-4-12B).

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use serde_json::json;
use plugin_llama_engine::engine::config::{load_config_early, ModelParams};
use plugin_llama_engine::engine::llamacpp_installer;
use plugin_llama_engine::engine::llm::LlamaEngine;
use plugin_llama_engine::engine::llm_types::{FunctionDef, LlmMessage, ToolDefinition};

/// Модель по умолчанию (выбор владельца 14.11.26).
const DEFAULT_TEST_MODEL: &str = "D:\\nn\\models\\llm\\uncen\\gemma-4-12B\\gemma-4-12B-it-qat-q4_0-unquantized-heretic-ja-v2.i1-IQ4_NL.gguf";

const AI_CTX_LIMIT: u32 = 8192;
const MAX_GEN_TOKENS: usize = 512;

/// Тул-заглушка find_files: один ход с тулом достаточен для смока (форма
/// параметров — та же, что пойдёт в ai_pick_tools.rs на Шаге 2).
fn find_files_tool() -> ToolDefinition {
    ToolDefinition {
        r#type: "function".to_string(),
        function: FunctionDef {
            name: "find_files".to_string(),
            description: "Ищет файлы проекта по подстроке в имени или пути. Возвращает список относительных путей (до 40 шт.).".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Подстрока имени файла или каталога (без учёта регистра)." }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
        },
    }
}

/// assistant-сообщение с вызовом тула в OpenAI-форме (форма из теста плагина
/// `does_not_merge_assistant_with_tool_calls`; роль/контент пусты — движок
/// рендерит `<|tool_call>` по чат-шаблону).
fn assistant_tool_msg(call: &plugin_llama_engine::engine::llm_types::ToolCall) -> LlmMessage {
    LlmMessage::default()
        .with_tool_calls(vec![json!({
            "id": call.id,
            "type": "function",
            "function": { "name": call.name, "arguments": call.arguments }
        })])
}

#[test]
#[ignore = "живая модель; запуск: cargo test --bin vibebridge -- --ignored ai_pick_smoke_tools --nocapture"]
fn ai_pick_smoke_tools() {
    // Модель: env или дефолт. Пустая переменная = дефолт.
    let model = std::env::var("AIPICK_TEST_MODELS")
        .ok()
        .and_then(|v| v.split(';').next().map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_TEST_MODEL.to_string());
    println!("СМОК: модель {model}");

    // Каталог llama.cpp — как в ai_pick::pick_files (глобальный конфиг плагина).
    let engine_cfg = load_config_early();
    let engine_dir = match &engine_cfg.llamacpp_dir {
        Some(dir) => PathBuf::from(dir),
        None => {
            let exe_dir = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(std::path::Path::to_path_buf))
                .expect("current_exe доступен в тесте");
            llamacpp_installer::default_dir(&exe_dir)
        }
    };

    let engine = LlamaEngine::new(
        &engine_dir,
        &model,
        AI_CTX_LIMIT,
        false,
        false,
        0,
        |s: String| println!("[llama] {s}"),
        |_s: String| {},
    )
    .expect("движок должен спавниться (проверь каталог llama.cpp в конфиге плагина)");

    let params = ModelParams { temperature: 0.1, ..Default::default() };
    let cancel = Arc::new(AtomicBool::new(false));
    let messages = vec![
        LlmMessage {
            role: "system".to_string(),
            content: "Ты — ассистент-исследователь кодовой базы. Для поиска файлов используй инструменты, не выдумывай пути.".to_string(),
            ..Default::default()
        },
        LlmMessage {
            role: "user".to_string(),
            content: "В проекте есть модуль экспорта CSV (csv_writer). Найди его: вызови тул find_files с query \"csv\". Не отвечай по памяти — сначала вызов.".to_string(),
            ..Default::default()
        },
    ];

    // ── Ход 1: модель должна вернуть tool_calls ─────────────────────────────
    // Сначала "auto" (основной режим по вердикту спеца); если модель пассивна —
    // ретрай с "required" (форс вызова). set_tools — consume-and-clear:
    // перед КАЖДЫМ generate_chat заново.
    engine.set_tools(Some(vec![find_files_tool()]));
    let t0 = std::time::Instant::now();
    let mut r1 = engine
        .generate_chat(
            &messages, MAX_GEN_TOKENS, &params, "auto", true, cancel.clone(),
            "smoke", Some("auto"), |_f: f32, _s: &str| {}, |s: String| println!("[llama] {s}"),
        )
        .expect("ход 1 должен пройти");
    let mut choice_used = "auto";
    if r1.tool_calls.is_empty() {
        println!("ХОД 1 auto: tool_calls=0 — ретрай с tool_choice=\"required\"");
        engine.set_tools(Some(vec![find_files_tool()]));
        r1 = engine
            .generate_chat(
                &messages, MAX_GEN_TOKENS, &params, "auto", true, cancel.clone(),
                "smoke", Some("required"), |_f: f32, _s: &str| {}, |s: String| println!("[llama] {s}"),
            )
            .expect("ход 1 (required) должен пройти");
        choice_used = "required";
    }
    println!(
        "ХОД 1 ({}.{:02}с, tool_choice={choice_used}): stop={}, text={:?}, tool_calls={}",
        t0.elapsed().as_secs(),
        t0.elapsed().subsec_millis() / 10,
        r1.stop_reason,
        r1.text.chars().take(200).collect::<String>(),
        r1.tool_calls.len()
    );
    println!(
        "ХОД 1 метрики: prompt_tokens={} generated_tokens={} ttft={:.1}с",
        r1.metrics.prompt_tokens, r1.metrics.generated_tokens, r1.metrics.ttft_sec
    );
    assert!(
        !r1.tool_calls.is_empty(),
        "ни \"auto\", ни \"required\" не дали tool_calls (модель/сборка не умеют нативный tool-calling): {:?}",
        r1.text
    );
    println!("ВЫВОД: ход 1 требует tool_choice=\"{choice_used}\" — учесть в оркестраторе (PRP §tool_choice).");

    let call = &r1.tool_calls[0];
    let args: serde_json::Value = serde_json::from_str(&call.arguments)
        .unwrap_or_else(|e| panic!("arguments не JSON ({e}): {:?}", call.arguments));
    println!("ХОД 1: тул `{}` аргументы {}", call.name, args);
    assert!(
        args.get("query").is_some(),
        "в arguments нет query: {args}"
    );

    // ── Ход 2: plain-text tool_result — не должен ронять сервер ────────────
    let mut history = messages.clone();
    history.push(assistant_tool_msg(call));
    history.push(
        LlmMessage::default().as_tool_result(
            call.id.clone(),
            "src/report.rs\nsrc/csv_writer.rs\nsrc/tests/csv_writer_test.rs",
        ),
    );
    let t1 = std::time::Instant::now();
    let plain = engine.generate_chat(
        &history, MAX_GEN_TOKENS, &params, "auto", true, cancel.clone(),
        "smoke", None, |_f: f32, _s: &str| {}, |s: String| println!("[llama] {s}"),
    );
    match plain {
        Ok(r) => {
            println!(
                "ХОД 2 plain ({}.{:02}с): OK stop={}, prompt_tokens={} — JSON-обёртка НЕ нужна",
                t1.elapsed().as_secs(),
                t1.elapsed().subsec_millis() / 10,
                r.stop_reason,
                r.metrics.prompt_tokens
            );
            println!("ХОД 2 ответ: {:?}", r.text.chars().take(300).collect::<String>());
        }
        Err(e) => {
            println!("ХОД 2 plain: Err: {e} — пробуем JSON-обёртку {{\"output\": …}}");
            history.pop(); // убрать plain-результат, поставить обёрнутый
            history.push(
                LlmMessage::default().as_tool_result(
                    call.id.clone(),
                    json!({ "output": "src/report.rs\nsrc/csv_writer.rs\nsrc/tests/csv_writer_test.rs" }).to_string(),
                ),
            );
            let wrapped = engine.generate_chat(
                &history, MAX_GEN_TOKENS, &params, "auto", true, cancel.clone(),
                "smoke", None, |_f: f32, _s: &str| {}, |s: String| println!("[llama] {s}"),
            )
            .expect("ход 2 с JSON-обёрткой должен пройти");
            println!(
                "ХОД 2 обёртка: OK stop={} — В ПРОДЕ ОБЯЗАТЕЛЬНА обёртка json!({{\"output\": …}})",
                wrapped.stop_reason
            );
            println!("ХОД 2 ответ: {:?}", wrapped.text.chars().take(300).collect::<String>());
        }
    }

    println!("СМОК PASSED: tool-calling gemma-4-12B работает end-to-end");
}
