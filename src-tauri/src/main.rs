//! VibeBridge — точка входа.
//!
//! Окно Tauri (`Window`, НЕ `WebviewWindow`) + egui через вендоренный
//! `tauri-plugin-egui`: WebView2 не используется, процесса `msedgewebview2.exe`
//! от приложения быть не должно (проверка шага 1 PRP).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ai_pick;
mod ai_pick_prompt;
mod ai_pick_snippets;
mod ai_pick_tools;
#[cfg(test)]
mod ai_pick_scenarios;
#[cfg(test)]
mod ai_pick_smoke;
mod clipboard;
mod ui;
mod worker;

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use tauri_plugin_egui::{egui, AppHandleExt, Builder as EguiPluginBuilder};

fn main() {
    // Краш-лог с первой миллисекунды (до инициализации Tauri).
    tauri_plugin_logs::early_init("vibebridge.log");
    tauri_plugin_logs::early_log("INFO", "VibeBridge: запуск (early, до Tauri)");

    if let Err(e) = run() {
        let msg = format!("критическая ошибка запуска: {e:#}");
        log::error!("{msg}");
        tauri_plugin_logs::early_log("ERROR", &msg);
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    tauri::Builder::default()
        .plugin(tauri_plugin_logs::init())
        .setup(|app| {
            // Облачная отправка ошибок отключена на уровне хоста (правило).
            tauri_plugin_logs::set_reporting_enabled(false);
            log::info!("VibeBridge: инициализация окна на egui (без WebView)");

            // Рендер-плагин egui: подписывается на цикл событий wry.
            app.wry_plugin(EguiPluginBuilder::new(app.handle().to_owned()));

            // Все пути приложения — от папки exe (правило владельца §6).
            let base = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(Path::to_path_buf))
                .ok_or("не удалось определить папку exe (current_exe)")?;

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
