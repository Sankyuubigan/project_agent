//! GPUI-фронтенд VibeBridge (экспериментальный, PRP 14.11.26).
//!
//! Запускается из `main.rs` при `ui_backend = "gpui"`: нативное окно gpui-kit,
//! Tauri НЕ инициализируется (WebView2 не создаётся). Бизнес-логика общая с
//! egui-веткой: `agent-core` + `worker.rs`; логи — `tauri-plugin-logs`
//! (standalone-установка, без AppHandle).

mod downlink_state;
mod draft_state;
mod logs_view;
mod main_state;
mod main_view;
mod root_view;
mod settings_view;
mod side_cards;
mod tree_items;
mod uplink_state;

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};

// AppContext — трейт, дающий cx.new() для &mut App (создание корневой Entity).
use my_gpui_kit::{AppContext as _, application, init, open_window};

use crate::worker::{Command, Event};

/// Точка входа GPUI-ветки: блокирует поток до закрытия окна.
pub fn run(base: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    // Облачная отправка отключена на уровне хоста (паритет с egui-веткой).
    tauri_plugin_logs::set_reporting_enabled(false);
    // Без Tauri глобальный log:: не ставится — standalone-установка единого логгера.
    tauri_plugin_logs::install_standalone(true, None);
    log::info!("VibeBridge: запуск GPUI-фронтенда (Tauri не инициализируется)");

    // Воркер (скан/токены/AI-подбор) — общий с egui-веткой (crate::worker).
    let (cmd_tx, event_rx): (Sender<Command>, Receiver<Event>) =
        crate::worker::spawn(base.clone());

    // Точка входа — рецепт скилла gpui-kit (quick reference): application →
    // init ПЕРВЫМ → open_window (сам оборачивает view в base::Root, без чего
    // не работают Dialog/Notification/Popover). Шапка — кастомный TitleBar:
    // системная белая полоса отключена (appears_transparent), drag — на нашей
    // верхней панели (план спеца №4, одна полоса).
    application()
        .with_assets(my_gpui_kit::assets::Assets)
        .run(move |cx| {
            init(cx);
            open_window(my_gpui_kit::window_options(), cx, |window, cx| {
                cx.new(|cx| root_view::RootView::new(base, cmd_tx, event_rx, window, cx))
            })
            .expect("failed to open window");
        });
    log::info!("VibeBridge: GPUI-окно закрыто");
    Ok(())
}
