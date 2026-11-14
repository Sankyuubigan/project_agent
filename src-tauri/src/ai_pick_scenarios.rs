//! Сценарный прогон AI-подбора (ручной, #[ignore]): фикстура-мини-проект
//! генерируется во временной папке, задачи разных типов с эталонами,
//! реальная модель через `ai_pick::pick_files`. Итог — temp/aipick_report.md
//! и печать в консоль.
//!
//! Запуск (разрешение владельца 14.11.26):
//!   cargo test --bin vibebridge -- --ignored ai_pick_real_scenarios --nocapture
//!
//! Модели: env `AIPICK_TEST_MODELS` (разделитель `;`); по умолчанию — модель,
//! указанная владельцем при утверждении плана (gemma-4-12B).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::ai_pick::pick_files;
use agent_core::tree::scanner::{self, FileStatus};

/// selectable-пути скана (как в uplink): источник истинности для тулов
/// AI-подбора (find_files) и анти-галлюцинации оркестратора.
fn selectable_paths(root: &Path) -> Vec<String> {
    scanner::scan(root)
        .files
        .iter()
        .filter(|f| matches!(f.status, FileStatus::Ok | FileStatus::ExcludedByDefault))
        .map(|f| f.rel_path.clone())
        .collect()
}

/// Модель по умолчанию (выбор владельца 14.11.26).
const DEFAULT_TEST_MODELS: &str = "D:\\nn\\models\\llm\\uncen\\gemma-4-12B\\gemma-4-12B-it-qat-q4_0-unquantized-heretic-ja-v2.i1-IQ4_NL.gguf";

/// Отчёт — в temp/ корня проекта (codemap этот каталог игнорирует).
fn report_path() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.parent().expect("manifest has parent");
    root.join("temp").join("aipick_report.md")
}

// ---------------------------------------------------------------------------
// Фикстура: мини-проект с известной структурой.
// ---------------------------------------------------------------------------

const FIXTURE_CARGO_TOML: &str = r#"# Фикстура для сценарного прогона AI-подбора (не собирается).
[package]
name = "ledger_app"
version = "0.1.0"
edition = "2021"

[dependencies]
"#;

const FIXTURE_MAIN: &str = r#"//! Точка входа фикстурного «учётного приложения».
mod app;
mod calendar;
mod config;
mod report;
mod ui_style;
mod utils;

fn main() {
    let cfg = config::AppConfig::default();
    let entries = app::demo_entries();
    let date = calendar::parse_date("2026-02-29");
    let total = report::compute_total(&entries);
    let text = report::generate_report(total, &date);
    println!("{} ({}): {}", text, cfg.name, app::PRIMARY_BUTTON_LABEL);
}
"#;

const FIXTURE_APP: &str = r#"//! Главный модуль приложения: сборка данных и кнопки интерфейса.
use crate::calendar::Entry;
use crate::report::compute_total;
use crate::ui_style::PRIMARY_BUTTON;

pub const PRIMARY_BUTTON_LABEL: &str = "Считать месяц";

pub fn demo_entries() -> Vec<Entry> {
    vec![
        Entry::new("аренда", 40_000),
        Entry::new("продукты", 18_500),
    ]
}

pub fn total_label(entries: &[Entry]) -> String {
    format!("{}: {}", PRIMARY_BUTTON_LABEL, compute_total(entries))
}
"#;

const FIXTURE_CALENDAR: &str = r#"//! Календарь: записи и парсинг дат.
use crate::utils::is_leap_year;

pub struct Entry {
    pub title: String,
    pub amount: i64,
}

impl Entry {
    pub fn new(title: &str, amount: i64) -> Self {
        Self { title: title.to_string(), amount }
    }
}

/// Разбирает дату в формате YYYY-MM-DD.
pub fn parse_date(text: &str) -> (i32, u32, u32) {
    let parts: Vec<&str> = text.split('-').collect();
    let year: i32 = parts[0].parse().unwrap_or(1970);
    let month: u32 = parts[1].parse().unwrap_or(1);
    let day: u32 = parts[2].parse().unwrap_or(1);
    if month == 2 && day == 29 && !is_leap_year(year) {
        // БАГ: несуществующая дата молча превращается в 28 февраля.
    }
    (year, month, day.min(28))
}
"#;

const FIXTURE_UTILS: &str = r#"//! Вспомогательные функции дат.
pub fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        2 if is_leap_year(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}
"#;

const FIXTURE_REPORT: &str = r#"//! Отчёты: подсчёт итогов и текстовая сводка.
use crate::calendar::Entry;

pub fn compute_total(entries: &[Entry]) -> i64 {
    entries.iter().map(|e| e.amount).sum()
}

pub fn generate_report(total: i64, date: &(i32, u32, u32)) -> String {
    format!("Итог {}: {}", date.0, total)
}
"#;

const FIXTURE_CONFIG: &str = r#"//! Настройки приложения.
pub struct AppConfig {
    pub name: String,
    pub theme_dark: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self { name: "ledger".into(), theme_dark: true }
    }
}
"#;

const FIXTURE_UI_STYLE: &str = r##"//! Цвета и стили интерфейса.
pub const PRIMARY_BUTTON: &str = "#2F6FED";
pub const DANGER_BUTTON: &str = "#ED4F2F";

pub fn accent_color() -> &'static str {
    PRIMARY_BUTTON
}
"##;

const FIXTURE_CSV_WRITER: &str = r#"//! Экспорт табличных данных в CSV.
pub fn write_csv(rows: &[Vec<String>]) -> String {
    rows.iter().map(|r| r.join(",")).collect::<Vec<_>>().join("\n")
}
"#;

/// Создать фикстуру в каталоге `dir` (создаёт src/, пишет 9 файлов).
fn build_fixture(dir: &Path) -> PathBuf {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("fixture src dir");
    let files: &[(&str, &str)] = &[
        ("Cargo.toml", FIXTURE_CARGO_TOML),
        ("src/main.rs", FIXTURE_MAIN),
        ("src/app.rs", FIXTURE_APP),
        ("src/calendar.rs", FIXTURE_CALENDAR),
        ("src/utils.rs", FIXTURE_UTILS),
        ("src/report.rs", FIXTURE_REPORT),
        ("src/config.rs", FIXTURE_CONFIG),
        ("src/ui_style.rs", FIXTURE_UI_STYLE),
        ("src/csv_writer.rs", FIXTURE_CSV_WRITER),
    ];
    for (rel, content) in files {
        fs::write(dir.join(rel), content).expect("fixture file");
    }
    dir.to_path_buf()
}

// ---------------------------------------------------------------------------
// Сценарии с эталонами.
// ---------------------------------------------------------------------------

/// Сценарий-задача с эталоном выбора.
struct Scenario {
    name: &'static str,
    task: &'static str,
    /// Обязательные файлы: без каждого из них вердикт — FAIL.
    core: &'static [&'static str],
    /// Допустимые: выбор их мусором не считается.
    allowed: &'static [&'static str],
    /// Явно лишние: попадание любого — FAIL.
    forbidden: &'static [&'static str],
}

const SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "БАГ: 29 февраля",
        task: "Функция parse_date принимает дату 2026-02-29 (невисокосный год) и молча превращает её в 28 февраля. Исправь: parse_date должна возвращать ошибку для несуществующих дат, а не подменять день.",
        core: &["src/calendar.rs"],
        allowed: &["src/utils.rs", "src/app.rs", "src/main.rs"],
        forbidden: &["src/ui_style.rs", "src/csv_writer.rs"],
    },
    Scenario {
        name: "ФИЧА: экспорт CSV",
        task: "Добавь в отчёты экспорт в CSV: generate_report должна уметь дополнительно возвращать строки отчёта в CSV-формате (готовый csv_writer уже есть в проекте — используй его).",
        core: &["src/report.rs"],
        allowed: &["src/csv_writer.rs", "src/app.rs", "src/main.rs"],
        forbidden: &["src/calendar.rs"],
        // Ключевая ловушка: модель должна НАЙТИ csv_writer по карте и взять его.
    },
    Scenario {
        name: "КОНФИГ: serde",
        task: "Сериализуй AppConfig в JSON: добавь derive Serialize/Deserialize для AppConfig и зависимость serde в Cargo.toml.",
        core: &["src/config.rs", "Cargo.toml"],
        allowed: &["src/main.rs"],
        forbidden: &["src/ui_style.rs", "src/csv_writer.rs"],
    },
    Scenario {
        name: "РЕФАКТОР: rename",
        task: "Переименуй функцию compute_total в calculate_total во всём проекте (объявление и все вызовы).",
        core: &["src/report.rs"],
        allowed: &["src/app.rs", "src/main.rs"],
        forbidden: &["src/ui_style.rs", "src/csv_writer.rs"],
    },
    Scenario {
        name: "UI: цвет кнопки",
        task: "Поменяй цвет главной кнопки приложения на зелёный (#2ECC40).",
        core: &["src/ui_style.rs"],
        allowed: &["src/app.rs"],
        forbidden: &["src/calendar.rs", "src/report.rs", "src/utils.rs"],
    },
];

/// Результат одного прогона.
struct ScenarioResult {
    verdict: &'static str,
    selected: Vec<String>,
    missing_core: Vec<String>,
    forbidden_hits: Vec<String>,
    noise: Vec<String>,
    reasoning: String,
    error: Option<String>,
}

fn run_scenario(scen: &Scenario, fixture_root: &Path, base: &Path) -> ScenarioResult {
    let cancel = Arc::new(AtomicBool::new(false));
    let status = |msg: String| println!("    [{msg}]");
    match pick_files(fixture_root, base, &selectable_paths(fixture_root), scen.task, cancel, &status) {
        Ok(out) => {
            let selected: Vec<String> =
                out.selected.iter().map(|s| norm(s)).filter(|s| !s.is_empty()).collect();
            let missing_core: Vec<String> = scen
                .core
                .iter()
                .filter(|c| !selected.iter().any(|s| s.as_str() == **c))
                .map(|c| c.to_string())
                .collect();
            let forbidden_hits: Vec<String> = scen
                .forbidden
                .iter()
                .filter(|c| selected.iter().any(|s| s.as_str() == **c))
                .map(|c| c.to_string())
                .collect();
            let noise: Vec<String> = selected
                .iter()
                .filter(|s| !scen.core.contains(&s.as_str()) && !scen.allowed.contains(&s.as_str()))
                .cloned()
                .collect();
            let verdict: &'static str = if !missing_core.is_empty() || !forbidden_hits.is_empty() {
                "FAIL"
            } else {
                "PASS"
            };
            ScenarioResult {
                verdict,
                selected,
                missing_core,
                forbidden_hits,
                noise,
                reasoning: out.reasoning,
                error: None,
            }
        }
        Err(e) => ScenarioResult {
            verdict: "ERROR",
            selected: Vec::new(),
            missing_core: Vec::new(),
            forbidden_hits: Vec::new(),
            noise: Vec::new(),
            reasoning: String::new(),
            error: Some(e),
        },
    }
}

/// Нормализовать предложенный путь (\\→/, без ./) — как в uplink.rs.
fn norm(p: &str) -> String {
    let p = p.trim().replace('\\', "/");
    p.strip_prefix("./").unwrap_or(&p).to_string()
}

fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

/// Ручной сценарий: полный прогон всех сценариев на выбранных моделях,
/// отчёт в temp/aipick_report.md. Обычный cargo_test его не запускает.
#[test]
#[ignore = "ручной прогон с реальной LLM (5-10 мин); запуск: cargo test -- --ignored ai_pick_real_scenarios --nocapture"]
fn ai_pick_real_scenarios() {
    let models: Vec<String> = match std::env::var("AIPICK_TEST_MODELS") {
        Ok(v) if !v.trim().is_empty() => v
            .split(';')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        _ => vec![DEFAULT_TEST_MODELS.to_string()],
    };

    let mut report = String::from("# Отчёт: сценарный прогон AI-подбора файлов\n\n");
    report.push_str(&format!("Дата: 14.11.26. Моделей: {}. Сценариев: {}.\n\n", models.len(), SCENARIOS.len()));
    let (mut pass, mut total) = (0usize, 0usize);

    for model in &models {
        if !Path::new(model).is_file() {
            println!("!! модель не найдена, прогон пропущен: {model}");
            report.push_str(&format!("## `{model}`\n\n⚠ файл модели не найден — прогон пропущен.\n\n"));
            continue;
        }
        println!("=== МОДЕЛЬ: {}", file_name(model));
        report.push_str(&format!("## Модель: `{}`\n\n", file_name(model)));
        report.push_str("| Сценарий | Вердикт | Пропущено (core) | Мусор | Запретные |\n|---|---|---|---|---|\n");
        for scen in SCENARIOS {
            total += 1;
            println!("--- сценарий: {}", scen.name);
            // Свежая фикстура на каждый прогон: изоляция + чистый codemap.
            let tmp = tempfile::tempdir().expect("tempdir");
            let fixture_root = build_fixture(tmp.path());
            // Конфиг VibeBridge с выбранной моделью: проверяется и resolve_model.
            let config_json = format!("{{\n  \"ai_model_path\": \"{}\"\n}}", model.replace('\\', "\\\\"));
            fs::write(tmp.path().join("config.json"), config_json).expect("config.json");
            let res = run_scenario(scen, &fixture_root, tmp.path());
            if res.verdict == "PASS" {
                pass += 1;
            }
            println!(
                "    вердикт: {}; выбрано: {:?}; пропущено: {:?}; мусор: {:?}; запретные: {:?}",
                res.verdict, res.selected, res.missing_core, res.noise, res.forbidden_hits
            );
            let name = scen.name;
            report.push_str(&format!(
                "| {name} | **{}** | {} | {} | {} |\n",
                res.verdict,
                res.missing_core.join(", "),
                res.noise.join(", "),
                res.forbidden_hits.join(", ")
            ));
            report.push_str(&res.details());
        }
        report.push('\n');
    }

    report.push_str(&format!("\n## Итог\n\nPASS: {pass}/{total}.\n"));
    let path = report_path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).expect("temp dir");
    }
    fs::write(&path, &report).expect("report write");
    println!("\n=== ИТОГ: PASS {pass}/{total}; отчёт: {}", path.display());
}

/// Ручной тест на живом проекте: полный пайплайн AI-подбора без фикстуры.
/// Env: AIPICK_REAL_ROOT — корень проекта (для него codemap идёт с --no-cache,
/// сам проект не изменяется), AIPICK_REAL_TASK — черновик-задача. Модель —
/// DEFAULT_TEST_MODELS через временный config.json. Результат — stdout и
/// temp/aipick_real_project_report.md; сверку выбора делает человек.
#[test]
#[ignore = "ручной прогон на реальном проекте; запуск: AIPICK_REAL_ROOT=... AIPICK_REAL_TASK=... cargo test -- --ignored ai_pick_real_project --nocapture"]
fn ai_pick_real_project() {
    let root = match std::env::var("AIPICK_REAL_ROOT") {
        Ok(v) if !v.trim().is_empty() => PathBuf::from(v.trim()),
        _ => panic!("задай AIPICK_REAL_ROOT — корень реального проекта"),
    };
    let task = match std::env::var("AIPICK_REAL_TASK") {
        Ok(v) if !v.trim().is_empty() => v,
        _ => panic!("задай AIPICK_REAL_TASK — текст задачи"),
    };
    assert!(root.is_dir(), "AIPICK_REAL_ROOT не найден: {}", root.display());

    // base — временная папка: конфиг только с моделью владельца (repomap_tool
    // возьмёт DEFAULT_TOOL), реальный config.json VibeBridge не читается.
    let tmp = tempfile::tempdir().expect("tempdir");
    let config_json = format!(
        "{{\n  \"ai_model_path\": \"{}\"\n}}",
        DEFAULT_TEST_MODELS.replace('\\', "\\\\")
    );
    fs::write(tmp.path().join("config.json"), config_json).expect("config.json");

    let cancel = Arc::new(AtomicBool::new(false));
    let status = |msg: String| println!("    [{msg}]");
    let started = std::time::Instant::now();
    let mut report = format!(
        "# Прогон на живом проекте: {}\n\nЗадача: {task}\n\n",
        root.display()
    );
    match pick_files(&root, tmp.path(), &selectable_paths(&root), &task, cancel, &status) {
        Ok(out) => {
            println!("=== выбрано ({}):", out.selected.len());
            for s in &out.selected {
                println!("  {s}");
            }
            println!("=== reasoning: {}", out.reasoning.trim());
            report.push_str(&format!("## Выбрано ({}):\n", out.selected.len()));
            for s in &out.selected {
                report.push_str(&format!("- {s}\n"));
            }
            report.push_str(&format!(
                "\nReasoning: {}\n\nВремя: {:.1} с\n",
                out.reasoning.trim(),
                started.elapsed().as_secs_f32()
            ));
        }
        Err(e) => {
            println!("=== ОШИБКА: {e}");
            report.push_str(&format!("ОШИБКА: {e}\n"));
        }
    }
    let path = report_path().with_file_name("aipick_real_project_report.md");
    fs::write(&path, &report).expect("report write");
    println!("=== отчёт: {}", path.display());
}

impl ScenarioResult {
    /// Детали прогона под таблицей (выборка + reasoning + ошибка).
    fn details(&self) -> String {
        let mut s = String::new();
        if let Some(e) = &self.error {
            s.push_str(&format!("\n> Ошибка: {e}\n"));
        }
        s.push_str(&format!("\nВыбрано ({}): {}\n", self.selected.len(), self.selected.join(", ")));
        if !self.reasoning.trim().is_empty() {
            s.push_str(&format!("\nReasoning: {}\n", self.reasoning.trim()));
        }
        s.push('\n');
        s
    }
}
