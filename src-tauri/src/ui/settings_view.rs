//! Вкладка «⚙ Настройки»: плашки плагина llama-engine (read-only — чужой
//! конфиг плагина не редактируем), выбор модели для AI-подбора и тул-бинаря
//! карты проекта. Сохранение — config.json VibeBridge рядом с exe, атомарно,
//! read-modify-write (не затираем last_project_dir другой части UI).

use std::path::Path;

use tauri_plugin_egui::egui;
use plugin_llama_engine::engine::config::{load_config_early, EngineConfig};

use agent_core::config::{self, UiBackend};
use agent_core::tree::repomap::DEFAULT_TOOL;

use super::style;
use crate::restart;

/// Стейт вкладки настроек.
pub struct SettingsState {
    /// Снимок конфига VibeBridge (отображение; сохранение — read-modify-write).
    cfg: config::AppConfig,
    /// Выбранный движок интерфейса (рабочее состояние радио; применяется
    /// кнопкой «Сохранить и перезапустить», PRP 14.11.26 шаг 2).
    ui_backend: UiBackend,
    /// Конфиг плагина (перечитывается кнопкой «↻ Перечитать»).
    engine: Option<EngineConfig>,
    status: Option<String>,
}

impl SettingsState {
    pub fn load(base: &Path) -> Self {
        let cfg = config::load(base);
        let mut st = Self { ui_backend: cfg.ui_backend, cfg, engine: None, status: None };
        st.refresh_engine();
        st
    }

    /// Перечитать конфиг плагина (модели могли поменяться извне).
    fn refresh_engine(&mut self) {
        log::info!("настройки: перечитываю конфиг плагина llama-engine");
        self.engine = Some(load_config_early());
    }

    /// Сохранить поле конфига: свежий read-modify-write с диска.
    fn persist(&mut self, base: &Path, apply: impl FnOnce(&mut config::AppConfig)) {
        let mut cfg = config::load(base);
        apply(&mut cfg);
        match config::save(base, &cfg) {
            Ok(()) => {
                self.cfg = cfg;
                log::info!("настройки: сохранены");
                self.status = Some("настройки сохранены".to_string());
            }
            Err(e) => {
                log::error!("настройки: не сохранены: {e}");
                self.status = Some(format!("не сохранено: {e}"));
            }
        }
    }

    pub fn draw(&mut self, ctx: &egui::Context, base: &Path) {
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                self.draw_backend_card(ui, base);
                ui.add_space(10.0);
                self.draw_engine_card(ui);
                ui.add_space(10.0);
                self.draw_model_card(ui, base);
                ui.add_space(10.0);
                self.draw_repomap_card(ui, base);
                if let Some(status) = &self.status {
                    ui.add_space(6.0);
                    ui.label(status);
                }
            });
        });
    }

    /// Карточка «Движок интерфейса» (PRP 14.11.26, шаг 2): переключатель
    /// egui/GPUI + сохранение и перезапуск. Конфиг — через persist
    /// (read-modify-write), рестарт — `crate::restart`.
    fn draw_backend_card(&mut self, ui: &mut egui::Ui, base: &Path) {
        style::card_frame(ui.ctx()).show(ui, |ui| {
            ui.heading("Движок интерфейса");
            ui.radio_value(&mut self.ui_backend, UiBackend::Egui, "egui (Текущий)");
            ui.radio_value(&mut self.ui_backend, UiBackend::Gpui, "GPUI (Новый)");
            let backend = self.ui_backend;
            ui.add_enabled_ui(backend != self.cfg.ui_backend, |ui| {
                if ui.button("Сохранить и перезапустить").clicked() {
                    self.persist(base, |cfg| cfg.ui_backend = backend);
                    if restart::relaunch() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    } else {
                        self.status =
                            Some("не удалось запустить новый процесс — см. лог".to_string());
                    }
                }
            });
        });
    }

    /// Плашка «Плагин llama-engine» (read-only).
    fn draw_engine_card(&mut self, ui: &mut egui::Ui) {
        style::card_frame(ui.ctx()).show(ui, |ui| {
            ui.heading("Плагин llama-engine (движок локальных моделей)");
            match &self.engine {
                None => {
                    ui.weak("— конфиг плагина не прочитан —");
                }
                Some(engine) => {
                    kv_row(ui, "Движок (llamacpp):", engine.llamacpp_dir.as_deref().unwrap_or("— не задан —"));
                    kv_row(ui, "Каталог моделей:", engine.models_dir.as_deref().unwrap_or("— не задан —"));
                    kv_row(ui, "Источник:", engine.engine_source.as_deref().unwrap_or("ggml-org (дефолт)"));
                    kv_row(ui, "Вариант:", engine.engine_variant.as_deref().unwrap_or("авто"));
                    let existing = engine.models.iter().filter(|m| Path::new(m).is_file()).count();
                    ui.weak(format!("Моделей в списке: {} (файлов на диске: {})", engine.models.len(), existing));
                    if engine.models.is_empty() {
                        ui.label(egui::RichText::new("Список моделей пуст — добавь модели в плагин (через его основное приложение).").color(egui::Color32::LIGHT_RED));
                    }
                    if ui.button("↻ Перечитать").clicked() {
                        self.refresh_engine();
                    }
                }
            }
        });
    }

    /// Плашка «Модель для AI-подбора»: ComboBox из списка плагина + ручной GGUF.
    fn draw_model_card(&mut self, ui: &mut egui::Ui, base: &Path) {
        style::card_frame(ui.ctx()).show(ui, |ui| {
            ui.heading("Модель для AI-подбора файлов");
            let Some(engine) = &self.engine else {
                ui.weak("— конфиг плагина не прочитан —");
                return;
            };
            let current = match &self.cfg.ai_model_path {
                Some(p) => file_label(p),
                None => "Первая доступная из списка плагина".to_string(),
            };
            let mut choice: Option<Option<String>> = None;
            egui::ComboBox::from_label("Модель")
                .selected_text(&current)
                .show_ui(ui, |ui| {
                    let none_sel = self.cfg.ai_model_path.is_none();
                    if ui.selectable_label(none_sel, "Первая доступная").clicked() {
                        choice = Some(None);
                    }
                    for m in &engine.models {
                        let exists = Path::new(m).is_file();
                        let is_sel = self.cfg.ai_model_path.as_deref() == Some(m.as_str());
                        let resp = ui.add_enabled(exists, egui::Button::selectable(is_sel, file_label(m)));
                        if resp.clicked() {
                            choice = Some(Some(m.clone()));
                        }
                        if !exists {
                            resp.on_disabled_hover_text("файл не найден на диске");
                        }
                    }
                });
            if let Some(model) = choice {
                self.persist(base, |cfg| cfg.ai_model_path = model);
            }
            ui.horizontal(|ui| {
                if ui.button("Другой GGUF…").clicked() {
                    if let Some(p) = rfd::FileDialog::new().add_filter("GGUF-модель", &["gguf"]).pick_file() {
                        let p = p.display().to_string();
                        log::info!("настройки: модель указана вручную: {p}");
                        self.persist(base, |cfg| cfg.ai_model_path = Some(p));
                    }
                }
                if self.cfg.ai_model_path.is_some() {
                    if ui.button("Сбросить («первая доступная»)").clicked() {
                        self.persist(base, |cfg| cfg.ai_model_path = None);
                    }
                    kv_row(ui, "Текущий ai_model_path:", self.cfg.ai_model_path.as_deref().unwrap_or(""));
                }
            });
            ui.weak("Используется кнопкой «✨ Подобрать файлы» в черновике; остальное (params) — из конфига плагина.");
        });
    }

    /// Плашка «Карта проекта»: путь тул-бинаря agent-tools.exe.
    fn draw_repomap_card(&mut self, ui: &mut egui::Ui, base: &Path) {
        style::card_frame(ui.ctx()).show(ui, |ui| {
            ui.heading("Карта проекта (тул codemap)");
            let tool = self
                .cfg
                .repomap_tool
                .clone()
                .unwrap_or_else(|| DEFAULT_TOOL.to_string());
            kv_row(ui, "Тул-бинарь:", &tool);
            if !Path::new(&tool).is_file() {
                ui.label(egui::RichText::new("⚠ файл не найден — AI-подбор не сможет строить карту проекта").color(egui::Color32::LIGHT_RED));
            }
            ui.horizontal(|ui| {
                if ui.button("Выбрать exe…").clicked() {
                    if let Some(p) = rfd::FileDialog::new().add_filter("exe", &["exe"]).pick_file() {
                        let p = p.display().to_string();
                        self.persist(base, |cfg| cfg.repomap_tool = Some(p));
                    }
                }
                if self.cfg.repomap_tool.is_some() && ui.button("Сбросить на дефолт").clicked() {
                    self.persist(base, |cfg| cfg.repomap_tool = None);
                }
            });
            ui.weak("agent-tools.exe codemap map --no-cache --tokens 2048 — скелет проекта для модели.");
        });
    }
}

/// Строка «ключ: значение» с моноширинным обрезаемым значением.
fn kv_row(ui: &mut egui::Ui, key: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.weak(key);
        ui.add(egui::Label::new(egui::RichText::new(value).monospace()).truncate());
    });
}

/// Имя файла из пути (для списка моделей).
fn file_label(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}
