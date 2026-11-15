//! Черновик запроса — стейт GPUI-«Главной» (PRP 14.11.26 шаг 3): промпт +
//! вложения-копии (файлы и картинки из буфера), чекбоксы «войдёт в
//! копирование», AI-подбор файлов с отменой. Логика 1:1 из `ui/draft_view.rs`
//! (egui); рендер — `side_cards.rs`. Стейт живёт в `drafts/manifest.json`
//! (пишется при каждом изменении, атомарно) — переживает перезапуск.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;

use agent_core::draft::{DraftAttachment, DraftManifest, DraftStore};

use crate::clipboard;
use crate::worker::Command;

/// Состояние черновика: хранилище + манифест + снимок последнего сохранения.
pub struct DraftState {
    store: DraftStore,
    manifest: DraftManifest,
    saved: DraftManifest,
    /// Контент, чьё сохранение уже провалилось (не ретраить на каждый drain).
    failed_snapshot: Option<DraftManifest>,
    status: Option<String>,
    /// AI-подбор в работе: (root, cancel-флаг). Флаг живёт в UI: кнопка
    /// «Отменить» поднимает его напрямую, минуя командный канал. Смена папки
    /// проекта сбрасывает busy автоматически (старый результат uplink
    /// отбросит по root-guard).
    ai_busy: Option<(PathBuf, Arc<AtomicBool>)>,
}

impl DraftState {
    pub fn load(base: &Path) -> Self {
        let store = DraftStore::new(base);
        let manifest = store.load();
        let saved = manifest.clone();
        Self {
            store,
            manifest,
            saved,
            failed_snapshot: None,
            status: None,
            ai_busy: None,
        }
    }

    /// Сохранить манифест, если он менялся с прошлой записи. Вызывается в
    /// drain-цикле воркера; провал не ретраится до следующего изменения.
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

    /// Манифест на чтение (для рендера: prompt_checked, attachments…).
    pub fn manifest(&self) -> &DraftManifest {
        &self.manifest
    }

    /// Текст промпта из textarea (подписка InputEvent::Change).
    pub fn set_prompt(&mut self, text: String) {
        self.manifest.prompt = text;
    }

    /// Чекбокс «Промпт» (войдёт ли текст в копирование).
    pub fn set_prompt_checked(&mut self, checked: bool) {
        self.manifest.prompt_checked = checked;
    }

    /// Чекбокс вложения (войдёт ли оно в копирование).
    pub fn set_attachment_checked(&mut self, name: &str, checked: bool) {
        if let Some(att) = self.manifest.attachments.iter_mut().find(|a| a.file_name == name) {
            att.checked = checked;
        }
    }

    pub fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }

    /// Идёт ли AI-подбор для этой папки.
    pub fn ai_busy_for(&self, root: Option<&Path>) -> bool {
        root.is_some_and(|r| matches!(&self.ai_busy, Some((busy, _)) if busy == r))
    }

    /// AI-подбор для этой папки завершился (успех/ошибка/отмена).
    pub fn on_ai_finished(&mut self, root: &Path) {
        if self.ai_busy_for(Some(root)) {
            self.ai_busy = None;
        }
    }

    /// Состояние кнопки AI-подбора: (busy, enabled, подсказка при disabled).
    pub fn pick_button_state(&self, root: Option<&Path>, scan_ready: bool) -> (bool, bool, String) {
        let busy = self.ai_busy_for(root);
        if busy {
            return (true, true, String::new());
        }
        let draft_empty = self.manifest.prompt.trim().is_empty();
        let enabled = root.is_some() && scan_ready && !draft_empty;
        let hint = if root.is_none() {
            "Сначала выбери папку проекта".to_string()
        } else if !scan_ready {
            "Дождись окончания скана".to_string()
        } else if draft_empty {
            "Опиши задачу в поле промпта выше".to_string()
        } else {
            String::new()
        };
        (false, enabled, hint)
    }

    /// Старт/отмена AI-подбора (клик по кнопке). Отмена — прямой store(true)
    /// в cancel-флаг (генерация его опрашивает); busy снимет событие
    /// AiPickFailed.
    pub fn pick_or_cancel(
        &mut self,
        tx: &Sender<Command>,
        root: Option<&Path>,
        project_paths: &[String],
    ) {
        if self.ai_busy_for(root) {
            if let Some((_, flag)) = &self.ai_busy {
                flag.store(true, Ordering::Relaxed);
                log::info!("черновик: AI-подбор — отмена запрошена");
            }
            return;
        }
        let Some(root) = root else { return }; // кнопка disabled без root — перестраховка
        let cancel = Arc::new(AtomicBool::new(false));
        match tx.send(Command::PickFiles {
            root: root.to_path_buf(),
            draft: self.manifest.prompt.clone(),
            cancel: cancel.clone(),
            project_paths: project_paths.to_vec(),
        }) {
            Ok(()) => {
                log::info!("черновик: запрошен AI-подбор файлов");
                self.ai_busy = Some((root.to_path_buf(), cancel));
                self.status = Some("ИИ подбирает файлы…".to_string());
            }
            Err(e) => {
                log::error!("команда AI-подбора не отправлена: {e}");
                self.status = Some("AI-подбор не запустился (воркер мёртв?) — см. лог".to_string());
            }
        }
    }

    /// Картинка ли это вложение (по расширению).
    pub fn is_image(name: &str) -> bool {
        DraftStore::is_image(name)
    }

    /// Убрать вложение по имени (id кнопки — домен от file_name).
    pub fn remove_attachment_by_name(&mut self, name: &str) {
        let index = match self.manifest.attachments.iter().position(|a| a.file_name == name) {
            Some(i) => i,
            None => return,
        };
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

    /// Вставить картинку из буфера обмена как вложение-копию.
    pub fn add_image_from_clipboard(&mut self) {
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

    /// Добавить выбранные файлы (вложения-копии). Диалог открывает UI-слой
    /// (async prompt_for_paths): здесь только копирование и запись манифеста.
    pub fn add_files_from(&mut self, paths: Vec<PathBuf>) {
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

    /// Текстовые части черновика для общей кнопки копирования (uplink):
    /// промпт (если отмечен) + текстовые вложения FILE-блоками формата v2.
    /// Ошибка чтения вложения — Err, не глотается.
    pub fn text_parts(&self) -> Result<Vec<String>, String> {
        let mut parts: Vec<String> = Vec::new();
        if self.manifest.prompt_checked && !self.manifest.prompt.trim().is_empty() {
            parts.push(self.manifest.prompt.clone());
        }
        for att in &self.manifest.attachments {
            if !att.checked || DraftStore::is_image(&att.file_name) {
                continue;
            }
            let path = self.store.path_of(&att.file_name);
            let content = std::fs::read_to_string(&path).map_err(|e| {
                let msg = format!("вложение {}: не прочитано ({e})", att.file_name);
                log::error!("черновик: {msg}");
                msg
            })?;
            parts.push(format!(
                "<<<FILE: {}>>>\n{}\n<<<END_FILE: {}>>>",
                att.file_name, content, att.file_name
            ));
        }
        Ok(parts)
    }

    /// Картинки — CF_HDROP (список файлов как из Проводника): одно Ctrl+V
    /// в браузере прикрепит все отмеченные.
    pub fn copy_images(&mut self) {
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
