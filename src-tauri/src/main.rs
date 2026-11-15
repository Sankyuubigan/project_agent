//! VibeBridge — точка входа.
//!
//! Движок интерфейса выбирается конфигом (`ui_backend` в `config.json`, PRP
//! 14.11.26): egui — окно Tauri (`Window`, НЕ `WebviewWindow`) + egui через
//! вендоренный `tauri-plugin-egui`; gpui — нативное окно gpui-kit, Tauri не
//! инициализируется. WebView2 не используется, процесса `msedgewebview2.exe`
//! от приложения быть не должно (проверка шага 1 PRP).

#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod ai_pick;
mod ai_pick_prompt;
mod ai_pick_snippets;
mod ai_pick_tools;
#[cfg(test)]
mod ai_pick_scenarios;
#[cfg(test)]
mod ai_pick_smoke;
mod clipboard;
mod gpui_ui;
mod restart;
mod ui;
mod worker;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use tauri_plugin_egui::{egui, AppHandleExt, Builder as EguiPluginBuilder};

use agent_core::config::{self, UiBackend};

fn main() {
    // Краш-лог с первой миллисекунды (до инициализации Tauri).
    plugin_log_collector::early_init("vibebridge.log");
    plugin_log_collector::early_log("INFO", "VibeBridge: запуск (early, до Tauri)");

    if let Err(e) = run() {
        let msg = format!("критическая ошибка запуска: {e:#}");
        log::error!("{msg}");
        plugin_log_collector::early_log("ERROR", &msg);
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Все пути приложения — от папки exe (правило владельца §6). Конфиг читаем
    // ДО выбора ветки UI: он и выбирает движок интерфейса.
    let base = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .ok_or("не удалось определить папку exe (current_exe)")?;
    let cfg = config::load(&base);
    // Глобальный log:: ещё не установлен (это делает выбранная ветка) — ранний канал.
    plugin_log_collector::early_log(
        "INFO",
        &format!("VibeBridge: движок интерфейса — {}", cfg.ui_backend),
    );
    match cfg.ui_backend {
        UiBackend::Gpui => gpui_ui::run(base),
        UiBackend::Egui => run_egui(base),
    }
}

/// Текущий фронтенд: окно Tauri + egui через вендоренный tauri-plugin-egui.
fn run_egui(base: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    tauri::Builder::default()
        .plugin(plugin_log_collector::init())
        .setup(move |app| {
            // Облачная отправка ошибок отключена на уровне хоста (правило).
            plugin_log_collector::set_reporting_enabled(false);
            log::info!("VibeBridge: инициализация окна на egui (без WebView)");

            // Рендер-плагин egui: подписывается на цикл событий wry.
            app.wry_plugin(EguiPluginBuilder::new(app.handle().to_owned()));

            // UI-стейт общий между Tauri-setup и egui-замыканием кадра.
            let app_ui = Arc::new(Mutex::new(ui::AppUi::new(ui::AppPaths { base })));

            // Обычное окно (не WebviewWindow) — движок webview не создаётся.
            tauri::window::Window::builder(app, "main")
                .title("VibeBridge")
                .inner_size(1200.0, 850.0)
                .build()
                .map_err(|e| format!("создание окна main: {e}"))?;

            // Замыкание кадра: egui дергает его на каждый рендер.
            let frame_ui = app_ui.clone();
            app.handle()
                .start_egui_for_window(
                    "main",
                    Box::new(move |ctx: &egui::Context| {
                        let mut guard = frame_ui.lock().unwrap_or_else(PoisonError::into_inner);
                        guard.update(ctx);
                    }),
                )
                .map_err(|e| format!("запуск egui для окна main: {e:#}"))?;

            log::info!("VibeBridge: окно main создано, egui запущен");
            Ok(())
        })
        .run(tauri::generate_context!())
        .map_err(|e| format!("цикл Tauri завершился с ошибкой: {e}"))?;
    Ok(())
}
