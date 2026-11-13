//! Черновик запроса (новая фича Шага 4): промпт + вложения-копии (файлы и
//! картинки из буфера), чекбоксы «войдёт в копирование», крестики.
//!
//! Стейт живёт в `drafts/manifest.json` (пишется при каждом изменении,
//! атомарно) — переживает перезапуск. Копирование: текст — промпт + текстовые
//! вложения FILE-блоками; картинки — CF_HDROP отдельной кнопкой.

use std::path::PathBuf;

use tauri_plugin_egui::egui;

use agent_core::draft::{DraftAttachment, DraftManifest, DraftStore};

use super::style;
use crate::clipboard;

/// Состояние черновика: хранилище + манифест + снимок последнего сохранения.
pub struct DraftState {
    store: DraftStore,
    manifest: DraftManifest,
    saved: DraftManifest,
    /// Контент, чьё сохранение уже провалилось (не ретраить на каждый кадр).
    failed_snapshot: Option<DraftManifest>,
    status: Option<String>,
}

impl DraftState {
    pub fn load(base: &std::path::Path) -> Self {
        let store = DraftStore::new(base);
        let manifest = store.load();
        let saved = manifest.clone();
        Self { store, manifest, saved, failed_snapshot: None, status: None }
    }

    /// Сохранить манифест, если он менялся с прошлой записи. Вызывается каждый
    /// кадр; провал не ретраится до следующего изменения (без спама в лог).
    pub fn flush_if_dirty(&mut self) {
        if self.manifest == self.saved
            || self.failed_snapshot.as_ref() == Some(&self.manifest)
        {
            return;
        }
        match self.store.save(&self.manifest) {
            Ok(()) => {
                self.saved = self.manifest.clone();
                self.failed_snapshot = None;
            }
            Err(e) => {
                log::error!("черновик не сохранён: {e}");
                self.failed_snapshot = Some(self.manifest.clone());
                self.status = Some(format!("черновик не сохранён: {e}"));
            }
        }
    }

    /// Доступ только на чтение для лога запуска (ui/mod.rs).
    pub fn manifest(&self) -> &DraftManifest {
        &self.manifest
    }

    pub fn draw(&mut self, ui: &mut egui::Ui) {
        ui.heading("Черновик запроса");
        ui.add_space(2.0);
        self.draw_toolbar(ui);
        // Поле всегда редактируемо: чекбокс «Промпт» влияет только на то,
        // войдёт ли текст в копирование (как в старой версии).
        ui.add(
            egui::TextEdit::multiline(&mut self.manifest.prompt)
                .hint_text("Текст запроса к LLM…")
                .desired_width(ui.available_width())
                .desired_rows(6)
                .margin(egui::Margin::same(8)),
        );
        self.draw_attachments(ui);
        if let Some(status) = &self.status {
            ui.label(status);
        }
    }

    fn draw_toolbar(&mut self, ui: &mut egui::Ui) {
        // Две строки: с padding (12,6) и шрифтом 14 все шесть элементов
        // в одну строку не влезают (кнопка «Скопировать картинки…»
        // обрезалась краем карточки).
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.manifest.prompt_checked, "Промпт");
            ui.separator();
            if ui.button("Вставить картинку из буфера").clicked() {
                self.add_image_from_clipboard();
            }
            if ui.button("Добавить файлы…").clicked() {
                self.add_files();
            }
        });
        ui.horizontal(|ui| {
            if style::accent_button(ui, "Скопировать текст", true, style::BLUE, style::BLUE_HOVER) {
                self.copy_text();
            }
            if ui.button("Скопировать картинки (файлы)").clicked() {
                self.copy_images();
            }
        });
    }

    fn draw_attachments(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.weak("Вложения (галочка — войдёт в копирование, ✖ — убрать):");
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .max_height(160.0)
            .show(ui, |ui| {
                let mut to_remove: Option<usize> = None;
                for (i, att) in self.manifest.attachments.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut att.checked, "");
                        let glyph =
                            if DraftStore::is_image(&att.file_name) { "🖼" } else { "📄" };
                        let full_path = self.store.path_of(&att.file_name);
                        ui.label(format!("{glyph} {}", att.file_name))
                            .on_hover_text(full_path.display().to_string());
                        if ui.small_button("✖").clicked() {
                            to_remove = Some(i);
                        }
                    });
                }
                if let Some(i) = to_remove {
                    self.remove_attachment(i);
                }
                if self.manifest.attachments.is_empty() {
                    ui.weak("— вложений нет —");
                }
            });
    }

    fn remove_attachment(&mut self, index: usize) {
        if let Some(att) = self.manifest.attachments.get(index) {
            if let Err(e) = self.store.remove(&att.file_name) {
                log::error!("вложение не удалено: {e}");
                self.status = Some(format!("вложение не удалено: {e}"));
                return;
            }
            log::info!("вложение убрано: {}", att.file_name);
        }
        if index < self.manifest.attachments.len() {
            self.manifest.attachments.remove(index);
        }
    }

    fn add_image_from_clipboard(&mut self) {
        match clipboard::png_from_clipboard() {
            Ok(png) => match self.store.add_png_bytes("clipboard", &png) {
                Ok(name) => {
                    log::info!("картинка из буфера добавлена: {name}");
                    self.manifest.attachments.push(DraftAttachment {
                        file_name: name.clone(),
                        checked: true,
                    });
                    self.status = Some(format!("картинка добавлена: {name}"));
                }
                Err(e) => {
                    log::error!("картинка из буфера: {e}");
                    self.status = Some(e);
                }
            },
            Err(e) => {
                log::warn!("картинка из буфера: {e}");
                self.status = Some(e);
            }
        }
    }

    fn add_files(&mut self) {
        let Some(paths) = rfd::FileDialog::new().pick_files() else { return };
        let mut added = 0usize;
        let mut last_err: Option<String> = None;
        for path in paths {
            match self.store.add_copied_file(&path) {
                Ok(name) => {
                    self.manifest.attachments.push(DraftAttachment {
                        file_name: name,
                        checked: true,
                    });
                    added += 1;
                }
                Err(e) => {
                    log::error!("вложение не добавлено: {e}");
                    last_err = Some(e);
                }
            }
        }
        self.status = match (added, last_err) {
            (0, Some(e)) => Some(e),
            (n, Some(e)) => Some(format!("добавлено {n}, с ошибками: {e}")),
            (n, None) => Some(format!("добавлено файлов: {n}")),
        };
    }

    /// Текст копирования: промпт (если отмечен) + текстовые вложения
    /// FILE-блоками формата v2 (путь в обоих тегах).
    fn copy_text(&mut self) {
        let mut parts: Vec<String> = Vec::new();
        if self.manifest.prompt_checked && !self.manifest.prompt.trim().is_empty() {
            parts.push(self.manifest.prompt.clone());
        }
        for att in &self.manifest.attachments {
            if !att.checked || DraftStore::is_image(&att.file_name) {
                continue;
            }
            let path = self.store.path_of(&att.file_name);
            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    parts.push(format!(
                        "<<<FILE: {}>>>\n{}\n<<<END_FILE: {}>>>",
                        att.file_name, content, att.file_name
                    ));
                }
                Err(e) => {
                    let msg = format!("вложение {}: не прочитано ({e})", att.file_name);
                    log::error!("черновик: {msg}");
                    self.status = Some(msg);
                    return;
                }
            }
        }
        if parts.is_empty() {
            self.status = Some("нечего копировать: отметь промпт или текстовые вложения".to_string());
            return;
        }
        match clipboard::copy_text(&parts.join("\n\n")) {
            Ok(()) => {
                let total: usize = parts.iter().map(|p| p.len()).sum();
                log::info!("черновик: текст скопирован ({total} символов)");
                self.status = Some("текст черновика скопирован".to_string());
            }
            Err(e) => {
                log::error!("черновик: {e}");
                self.status = Some(e);
            }
        }
    }

    /// Картинки — CF_HDROP (список файлов как из Проводника): одно Ctrl+V
    /// в браузере прикрепит все отмеченные.
    fn copy_images(&mut self) {
        let paths: Vec<PathBuf> = self
            .manifest
            .attachments
            .iter()
            .filter(|a| a.checked && DraftStore::is_image(&a.file_name))
            .map(|a| self.store.path_of(&a.file_name))
            .collect();
        match clipboard::copy_file_list(&paths) {
            Ok(()) => {
                log::info!("черновик: CF_HDROP скопировано {} картинок", paths.len());
                self.status = Some(format!("скопировано картинок (файлами): {}", paths.len()));
            }
            Err(e) => {
                log::warn!("черновик: {e}");
                self.status = Some(e);
            }
        }
    }
}
