//! Промпты и бюджет tool-loop AI-подбора (Шаг 3 [PRP tasks/PRP_ai_pick_tool_loop.md]).
//! Контекст-политика спеца: pre-seed карта в первом user-сообщении,
//! append-only история, стоп-кран по факту токенов, аварийный trim
//! старейшего tool-результата с сохранением путей.
//!
//! Поправка по смоук-тесту 14.11.26: `metrics.prompt_tokens` движка —
//! инкрементальные (не-кэшированные) токены, поэтому полный размер промпта
//! считает ХОСТ — точным токенизатором o200k_base (agent-core tokens.rs).

use std::collections::HashSet;

use agent_core::tree::tokens;
use tauri_plugin_llama_engine::engine::llm_types::LlmMessage;

/// Потолок суммы токенов Фазы 1 (n_ctx 8192 − резерв Фазы 2 1024).
/// При достижении — переход в Фазу 2.
pub const SAFE_LOOP_CEILING: u32 = 7168;

/// Порог аварийного trim перед Фазой 2 (n_ctx − 640).
pub const TRIM_TRIGGER: u32 = 7552;

/// Точные токены строки хостовым o200k-токенизатором; энкодер недоступен —
/// консервативный фолбэк «символы/2» (завышает — безопасно для бюджета).
pub fn count_tokens(text: &str) -> u32 {
    tokens::token_cache()
        .count_text(text)
        .map(|n| n as u32)
        .unwrap_or((text.chars().count() as u32).div_ceil(2))
}

/// Токены всей истории диалога (текст + ~4 токена на сообщение за роль/обвязку).
pub fn count_history_tokens(messages: &[LlmMessage]) -> u32 {
    messages.iter().map(|m| count_tokens(&m.content) + 4).sum()
}

/// Грубая оценка нового тул-вывода до его вставки (символы/2 — вердикт спеца).
pub fn est_tokens_by_chars(chars: usize) -> u32 {
    (chars as u32).div_ceil(2)
}

/// Стоп-кран Фазы 1: фактический последний ход (`last_total` — полный промпт
/// хостового счёта + сгенерированные токены) плюс грубая оценка новых
/// тул-результатов ≥ потолка → новые результаты не вставляются, идём в Фазу 2.
pub fn ceiling_reached(last_total: u32, new_results_chars: usize) -> bool {
    last_total + est_tokens_by_chars(new_results_chars) >= SAFE_LOOP_CEILING
}

/// Аварийный trim: тело САМОГО СТАРОГО tool-сообщения заменяется стабом
/// с сохранением обнаруженных путей. Пару assistant→tool не рвать —
/// меняется только content tool-роли. Возвращает true, если было что резать.
pub fn trim_oldest_tool_result(messages: &mut [LlmMessage], valid: &HashSet<String>) -> bool {
    let Some(idx) = messages.iter().position(|m| m.role == "tool") else {
        return false;
    };
    let original = messages[idx].content.clone();
    let paths = crate::ai_pick_tools::extract_found_paths(&original, valid);
    messages[idx].content = trim_stub(&paths);
    true
}

fn trim_stub(paths: &[String]) -> String {
    if paths.is_empty() {
        "[сокращено ради лимита контекста]".to_string()
    } else {
        format!(
            "[сокращено ради лимита контекста; обнаруженные пути: {}]",
            paths.join(", ")
        )
    }
}

/// Детект переполнения контекста по подстрокам ошибки (вердикт спеца §5.2).
pub fn looks_like_overflow(err: &str) -> bool {
    const MARKERS: [&str; 7] = [
        "context length",
        "exceed",
        "n_ctx",
        "too long",
        "http 400",
        "kv cache",
        "sliding window",
    ];
    let lower = err.to_lowercase();
    MARKERS.iter().any(|m| lower.contains(m))
}

// ---------------------------------------------------------------------------
// Промпты.
// ---------------------------------------------------------------------------

/// Системный промпт Фазы 1 — исследователь. `require_tools` (проект >30
/// файлов) — спец требует явного вызова инструментов перед финалом.
pub fn phase1_system(require_tools: bool) -> String {
    let mut s = String::from(
        "Ты — исследователь кодовой базы в IDE. Цель: выяснить, какие файлы \
проекта затронуты задачей пользователя, используя инструменты исследования. \
Финальный JSON ты вернёшь по отдельной инструкции.

ПРАВИЛА:
1. Пути файлов брать ТОЛЬКО из карты проекта и результатов инструментов. Выдумывать пути запрещено.
2. Исследуй все затронутые слои: UI/контроллеры → сервисы → API → хранилище/инфраструктура. Фича обычно тянется цепочкой через границы модулей: codemap_search находит символ, codemap_callers показывает, кто его использует.
3. Если в задаче назван модуль или файл по имени — найди его через find_files (файлы без ссылок в карте всё равно найдутся).
4. Обычно достаточно 2–6 вызовов инструментов. Конфиги (Cargo.toml, package.json) — только если задача про зависимости или сборку.
5. Когда исследования достаточно, ответь обычным текстом: краткое резюме, какие файлы задействованы (без JSON).",
    );
    if require_tools {
        s.push_str(
            "\n\nВ проекте больше 30 файлов — исследование идёт ПО СЛОЯМ, минимум 4 вызова инструментов: \
(1) найди UI/контроллер задачи; (2) по именам символов из его кода и результатов поиска проследи \
сервисы и API-слой (в Tauri-приложениях — src-tauri/src/api/*.rs, команды invoke); \
(3) найди, где структуры данных задачи хранятся/объявлены — хранилище и инфраструктура \
(codemap_search по имени структуры: infra, domain, storage); \
(4) уточни содержимое ключевых файлов (codemap_skeleton). \
Не завершай исследование, пока цепочка UI→сервисы→API→хранилище не проверена — финал после \
одного-двух вызовов почти всегда неполный.",
        );
    }
    s
}

/// Стартовое user-сообщение: pre-seed карта (бюджет ~768, построена хостом)
/// + черновик пользователя.
pub fn user_seed(map: &str, draft: &str) -> String {
    format!(
        "=== СТРУКТУРА ПРОЕКТА (построена заранее; полный список файлов доступен через тул find_files, а точечную карту каталога можно построить через codemap_map со scope=путь/к/каталогу) ===\n\
{map}\n\n\
=== ЗАДАЧА ПОЛЬЗОВАТЕЛЯ (ЧЕРНОВИК) ===\n{}\n\n\
Исследуй проект инструментами и определи, какие файлы нужно затронуть.",
        draft.trim()
    )
}

/// Инструкция Фазы 2 после ЕСТЕСТВЕННОЙ остановки (модель сама закончила
/// текстом). Самодостаточная: system Фазы 1 в истории остаётся, отдельного
/// system Фазы 2 нет — формат JSON должен лежать в самой инструкции.
pub fn phase2_user_natural() -> String {
    "Ты завершил исследование сам. Теперь верни ИТОГОВЫЙ ответ СТРОГО в формате JSON без какого-либо текста до или после:\n\
{\"reasoning\": \"1-2 предложения: какие модули затронуты задачей и почему\", \"selected_files\": [\"относительный/путь/к/файлу\"]}\n\n\
Правила: в selected_files — только пути, реально существующие в проекте (из результатов инструментов выше); файлы, которые нужно изменить, и файлы с объявлениями структур, которые правка затрагивает; НЕ добавляй модули-источники данных, которые правка в рантайме лишь читает, НО ОБЯЗАТЕЛЬНО добавляй модули хранения/моделей, схему данных или логику записи которых нужно изменить под задачу; обычно 2–8 файлов.".to_string()
}

/// Инструкция Фазы 2 после ПРИНУДИТЕЛЬНОЙ остановки (лимит хода/бюджета) —
/// неполный текст модели отбрасывается.
pub fn phase2_user_forced() -> String {
    "Исследование завершено (достигнут лимит хода). На основе полученных выше результатов инструментов верни ИТОГОВЫЙ ответ СТРОГО в формате JSON без какого-либо текста до или после:\n\
{\"reasoning\": \"1-2 предложения: какие модули затронуты задачей и почему\", \"selected_files\": [\"относительный/путь/к/файлу\"]}\n\n\
Правила: в selected_files — только пути, реально существующие в проекте (из результатов инструментов выше); файлы, которые нужно изменить, и файлы с объявлениями структур, которые правка затрагивает; НЕ добавляй модули-источники данных, которые правка в рантайме лишь читает, НО ОБЯЗАТЕЛЬНО добавляй модули хранения/моделей, схему данных или логику записи которых нужно изменить под задачу; обычно 2–8 файлов.".to_string()
}

/// Инструкция ретрая после отбраковки фантомных путей (ровно 1 ретрай).
pub fn retry_instruction(phantom: &[String]) -> String {
    format!(
        "Пути [{}] не существуют в проекте. Выбери только существующие пути из результатов инструментов выше.",
        phantom.join(", ")
    )
}

/// Пинок после раннего natural-stop на большом проекте (оркестратор даёт
/// РОВНО один): модель 12B торопится в финал после 1–2 тул-вызовов и
/// пропускает слои сервисов/хранилища.
pub fn research_nudge() -> String {
    "Проверь, какие файлы отвечают за хранение данных/состояние и модели, которые нужно \
изменить под задачу: изучи через codemap_skeleton или codemap_search подходящие файлы из \
infra/store/models/services, найденные выше. Когда цепочка слоёв проверена, ответь обычным \
текстом: краткое резюме задействованных файлов."
        .to_string()
}

/// Ответ модели. Форму гарантирует json_schema-грамматика; `default` —
/// перестраховка (движки с неполной поддержкой схем).
#[derive(serde::Deserialize, Debug)]
pub struct PickAnswer {
    #[serde(default)]
    pub reasoning: String,
    #[serde(default)]
    pub selected_files: Vec<String>,
}

/// Грамматика финального ответа (Фаза 2): жёсткий JSON-контракт.
pub fn answer_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "reasoning": { "type": "string" },
            "selected_files": { "type": "array", "items": { "type": "string" } }
        },
        "required": ["reasoning", "selected_files"],
        "additionalProperties": false
    })
}

/// Разобрать ответ модели (pub(crate) — для оркестратора и тестов).
pub fn parse_answer(text: &str) -> Result<PickAnswer, String> {
    serde_json::from_str::<PickAnswer>(text.trim()).map_err(|e| {
        log::error!("ai-pick: ответ не разобран ({e}); текст: {text}");
        format!("ответ модели не разобран ({e}) — текст в логе")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri_plugin_llama_engine::engine::llm_types::ToolCall;

    fn tool_msg(content: &str) -> LlmMessage {
        LlmMessage::default().as_tool_result("t1".to_string(), content)
    }

    fn assistant_with_call() -> LlmMessage {
        LlmMessage {
            role: "assistant".to_string(),
            content: String::new(),
            ..Default::default()
        }
        .with_tool_calls(vec![serde_json::json!({
            "id": "t1", "type": "function",
            "function": { "name": "codemap_search", "arguments": "{\"query\":\"x\"}" }
        })])
    }

    #[test]
    fn phase1_prompt_names_all_tools_and_respects_flag() {
        let p = phase1_system(false);
        for tool in ["find_files", "codemap_search", "codemap_callers"] {
            assert!(p.contains(tool), "{tool} не упомянут");
        }
        assert!(!p.contains("больше 30 файлов"));
        assert!(phase1_system(true).contains("больше 30 файлов"));
    }

    #[test]
    fn phase2_instructions_are_self_contained() {
        for p in [phase2_user_natural(), phase2_user_forced()] {
            assert!(p.contains("selected_files"), "{p}");
            assert!(p.contains("reasoning"), "{p}");
            assert!(p.contains("СТРОГО"), "{p}");
            // формат JSON лежит в самой инструкции (system Фазы 2 нет)
            assert!(p.contains("относительный/путь/к/файлу"), "{p}");
        }
    }

    #[test]
    fn seed_contains_map_and_draft() {
        let s = user_seed("File: a.rs", "  почини парсер  ");
        assert!(s.contains("File: a.rs"));
        assert!(s.contains("почини парсер"));
        assert!(s.contains("find_files"));
    }

    #[test]
    fn count_tokens_positive_and_history_adds_overhead() {
        assert!(count_tokens("hello world, привет мир") > 0);
        let msgs = vec![LlmMessage {
            role: "user".to_string(),
            content: "abc".to_string(),
            ..Default::default()
        }];
        assert_eq!(count_history_tokens(&msgs), count_tokens("abc") + 4);
    }

    #[test]
    fn ceiling_boundary_is_exact() {
        assert!(!ceiling_reached(7167, 0));
        assert!(ceiling_reached(7167, 2));
        assert!(ceiling_reached(7000, 400));
        assert!(!ceiling_reached(1000, 100));
    }

    #[test]
    fn trim_keeps_pairs_and_paths() {
        let valid: HashSet<String> = ["src/report.rs", "src/csv_writer.rs"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mut msgs = vec![
            LlmMessage { role: "system".into(), content: "с".into(), ..Default::default() },
            LlmMessage { role: "user".into(), content: "у".into(), ..Default::default() },
            assistant_with_call(),
            tool_msg("src/report.rs:\n   12 | fn a()\nsrc/csv_writer.rs:"),
            LlmMessage { role: "assistant".into(), content: "резюме".into(), ..Default::default() },
        ];
        assert!(trim_oldest_tool_result(&mut msgs, &valid));
        assert_eq!(msgs[2].tool_calls.as_ref().map(|t| t.len()), Some(1), "пару не рвать");
        assert!(msgs[3].content.contains("src/report.rs"), "{}", msgs[3].content);
        assert!(msgs[3].content.contains("сокращено"));
        assert_eq!(msgs[4].content, "резюме", "хвост не тронут");
        assert_eq!(msgs[3].role, "tool");
    }

    #[test]
    fn trim_without_tool_messages_is_noop() {
        let valid: HashSet<String> = HashSet::new();
        let mut msgs = vec![LlmMessage {
            role: "user".into(),
            content: "у".into(),
            ..Default::default()
        }];
        assert!(!trim_oldest_tool_result(&mut msgs, &valid));
        assert_eq!(msgs[0].content, "у");
    }

    #[test]
    fn trim_stub_without_known_paths_is_generic() {
        let valid: HashSet<String> = HashSet::new();
        let mut msgs = vec![tool_msg("что-то без путей")];
        assert!(trim_oldest_tool_result(&mut msgs, &valid));
        assert_eq!(msgs[0].content, "[сокращено ради лимита контекста]");
    }

    #[test]
    fn overflow_markers_match_error_text() {
        assert!(looks_like_overflow("error: context length is 8192 but prompt is 9000"));
        assert!(looks_like_overflow("HTTP 400 Bad Request"));
        assert!(looks_like_overflow("prompt too long: n_ctx exceeded"));
        assert!(!looks_like_overflow("connection refused"));
        assert!(!looks_like_overflow("модель отказалась"));
    }

    #[test]
    fn retry_instruction_lists_phantoms() {
        let r = retry_instruction(&["a/ghost.rs".to_string(), "b/fake.ts".to_string()]);
        assert!(r.contains("a/ghost.rs"));
        assert!(r.contains("b/fake.ts"));
        assert!(r.contains("не существуют"));
    }

    #[test]
    fn tool_call_construction_smoke() {
        // гарантия: ToolCall в тестах собирается в OpenAI-форму без паники
        let c = ToolCall { id: "t1".into(), name: "find_files".into(), arguments: "{}".into() };
        assert_eq!(c.name, "find_files");
    }

    #[test]
    fn parse_valid_answer() {
        let a = parse_answer(r#"{"reasoning":"затронут парсер","selected_files":["a.rs","b.rs"]}"#)
            .unwrap();
        assert_eq!(a.selected_files, vec!["a.rs", "b.rs"]);
        assert_eq!(a.reasoning, "затронут парсер");
    }

    #[test]
    fn parse_tolerates_extra_fields() {
        let a = parse_answer(r#"{"reasoning":"","selected_files":[],"unknown":1}"#).unwrap();
        assert!(a.selected_files.is_empty());
    }

    #[test]
    fn broken_json_is_a_clear_error() {
        let err = parse_answer("не json вообще").unwrap_err();
        assert!(err.contains("не разобран"), "{err}");
    }

    #[test]
    fn schema_has_required_keys() {
        let schema = answer_schema();
        assert_eq!(schema["required"][0], "reasoning");
        assert_eq!(schema["required"][1], "selected_files");
        assert_eq!(schema["additionalProperties"], false);
    }
}
