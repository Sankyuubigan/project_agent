//! Весь UI VibeBridge: вкладка «Главная» (Downlink + Черновик слева,
//! дерево проекта/Uplink справа) и вкладка «Логи».
//!
//! Правила (вердикт спеца №1): вся отрисовка — в `AppUi::update`;
//! `AppHandle` сюда не протягивается; тяжёлое — в воркере через mpsc-каналы.

mod downlink;
mod draft_view;
mod logs_view;
mod settings_view;
mod style;
mod tree_widget;
mod uplink;
mod uplink_text;

use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::Duration;

use tauri_plugin_egui::egui;

use crate::worker::{self, Command, Event};
use downlink::DownlinkState;
use draft_view::DraftState;
use settings_view::SettingsState;
use uplink::UplinkState;

/// Период фоновой перерисовки в простое (реактивный рендер: без 60 FPS).
const IDLE_REPAINT: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Main,
    Logs,
    Settings,
}

/// Пути приложения (правило владельца §6: всё от папки exe).
pub struct AppPaths {
    /// Папка exe: config.json, drafts/, backup/.
    pub base: PathBuf,
}

/// Весь UI приложения. Один стейт, одна точка отрисовки.
pub struct AppUi {
    tab: Tab,
    cmd_tx: Sender<Command>,
    event_rx: std::sync::mpsc::Receiver<Event>,
    paths: AppPaths,
    uplink: UplinkState,
    downlink: DownlinkState,
    draft: DraftState,
    settings: SettingsState,
    logs: logs_view::LogView,
    /// Шрифты применены (однократно перед первым кадром).
    styled: bool,
    /// Тёмная тема (переключатель ☀/🌙 в шапке; тёмная по умолчанию).
    dark: bool,
    /// Какая тема применена последней (None — ещё ни одной).
    theme_applied: Option<bool>,
}

impl AppUi {
    pub fn new(paths: AppPaths) -> Self {
        let (cmd_tx, event_rx) = worker::spawn(paths.base.clone());

        // Конфиг: последняя папка проекта → сразу сканируем.
        let cfg = agent_core::config::load(&paths.base);
        let mut uplink = UplinkState::new();
        if let Some(dir) = cfg.last_project_dir.clone() {
            let root = PathBuf::from(&dir);
            if root.is_dir() {
                uplink.open_root(&root, &paths.base, &cmd_tx);
            } else {
                log::warn!("сохранённая папка проекта не найдена: {dir}");
            }
        }

        // Черновик переживает перезапуск (manifest.json).
        let draft = DraftState::load(&paths.base);
        log::info!(
            "черновик: промпт {} символов, вложений {}",
            draft.manifest().prompt.len(),
            draft.manifest().attachments.len()
        );

        // Настройки: читают конфиг ДО перемещения paths в стейт.
        let settings = SettingsState::load(&paths.base);

        Self {
            tab: Tab::Main,
            cmd_tx,
            event_rx,
            paths,
            uplink,
            downlink: DownlinkState::new(),
            draft,
            settings,
            logs: logs_view::LogView::new(),
            styled: false,
            dark: true,
            theme_applied: None,
        }
    }

    /// Единственный вход отрисовки (вызывается из замыкания кадра в main.rs).
    pub fn update(&mut self, ctx: &egui::Context) {
        // Шрифты — один раз до первой отрисовки панелей; тема — при старте
        // и при переключении ☀/🌙.
        if !self.styled {
            style::apply_fonts(ctx);
            self.styled = true;
        }
        if self.theme_applied != Some(self.dark) {
            style::apply_theme(ctx, self.dark);
            self.theme_applied = Some(self.dark);
        }
        ctx.request_repaint_after(IDLE_REPAINT);

        self.poll_events();
        self.draft.flush_if_dirty();

        egui::TopBottomPanel::top("vibebridge_tabs")
            // Fill обязателен: Frame::new() прозрачный, сквозь него виден
            // белый clear-color wgpu (названия вкладок сливались с фоном).
            .frame(
                egui::Frame::new()
                    .fill(ctx.style().visuals.panel_fill)
                    .inner_margin(egui::Margin::symmetric(12, 8)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("VibeBridge").strong());
                    ui.separator();
                    ui.selectable_value(&mut self.tab, Tab::Main, "Главная");
                    ui.selectable_value(&mut self.tab, Tab::Logs, "Логи");
                    ui.selectable_value(&mut self.tab, Tab::Settings, "⚙ Настройки");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let glyph = if self.dark { "☀" } else { "🌙" };
                        if ui.button(glyph).clicked() {
                            self.dark = !self.dark;
                            style::apply_theme(ctx, self.dark);
                            self.theme_applied = Some(self.dark);
                        }
                    });
                });
            });

        match self.tab {
            Tab::Main => self.draw_main(ctx),
            Tab::Logs => self.logs.draw(ctx),
            Tab::Settings => self.settings.draw(ctx, &self.paths.base),
        }
    }

    /// Неблокирующее чтение событий воркера (скан, токены, AI-подбор).
    fn poll_events(&mut self) {
        while let Ok(ev) = self.event_rx.try_recv() {
            match ev {
                Event::Scanned { root, out } => self.uplink.on_scanned(&root, out),
                Event::Tokens { root, rel, count } => self.uplink.on_tokens(&root, &rel, count),
                Event::AiStatus { root, msg } => self.uplink.on_ai_status(&root, &msg),
                Event::AiPicked { root, selected, reasoning } => {
                    self.uplink.on_ai_picked(&root, &selected, &reasoning);
                    self.draft.on_ai_finished(&root);
                }
                Event::AiPickFailed { root, msg } => {
                    self.uplink.on_ai_failed(&root, &msg);
                    self.draft.on_ai_finished(&root);
                }
            }
        }
    }

    fn draw_main(&mut self, ctx: &egui::Context) {
        // Справа — проект (Uplink): дерево с чекбоксами и сборка промпта.
        // Содержимое панелей — в карточках (Frame с рамкой и полями).
        egui::SidePanel::right("tree_panel")
            .min_width(360.0)
            .resizable(true)
            .show(ctx, |ui| {
                style::card_frame(ui.ctx()).show(ui, |ui| {
                    self.uplink.draw(ui, &self.paths.base, &self.cmd_tx, &self.draft);
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            // Сверху слева — ответ LLM (Downlink).
            egui::TopBottomPanel::top("downlink_area")
                .resizable(true)
                .default_height(340.0)
                .min_height(180.0)
                .show_inside(ui, |ui| {
                    style::card_frame(ui.ctx()).show(ui, |ui| {
                        self.downlink.draw(ui, self.uplink.root(), &self.paths.base);
                    });
                });
            // Ниже — черновик запроса. Панель bottom (не голый Frame): только
            // панели egui честно обрезаются по границе центральной области —
            // Frame на ui центральной панели вылезал под правую панель
            // (рамки правого края не было видно, кнопки обрезались).
            egui::TopBottomPanel::bottom("draft_panel").show_inside(ui, |ui| {
                style::card_frame(ui.ctx()).show(ui, |ui| {
                    // selectable-пути скана — для тулов AI-подбора (find_files,
                    // анти-галлюцинация); заимствование uplink кончается до
                    // мутаций внутри draw.
                    let project_paths = self.uplink.selectable_relative_paths();
                    self.draft.draw(
                        ui,
                        &self.cmd_tx,
                        self.uplink.root(),
                        self.uplink.scan_ready(),
                        &project_paths,
                    );
                });
            });
        });
    }
}
