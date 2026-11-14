//! Downlink (чат → проект): поле ответа LLM, двухфазное применение правок
//! (dry-run → запись с бэкапом), «применить только валидные», Undo.

use std::path::{Path, PathBuf};

use tauri_plugin_egui::egui;

use super::style;
use agent_core::patch::{apply, parse, prepare, undo_last, ApplyReport, PreparedChange};

/// Папка бэкапа последнего патча (относительно base — папки exe).
const BACKUP_DIR: &str = r"backup\last_patch";

/// Состояние Downlink-части главной вкладки.
pub struct DownlinkState {
    response: String,
    /// Отчёт последнего разбора/применения (показ + кнопки).
    report: Option<Report>,
}

/// Результат разбора/применения для показа в UI.
struct Report {
    parse_errors: Vec<(Option<String>, String)>,
    parse_warnings: Vec<String>,
    applied: Vec<String>,
    failed: Vec<(String, String)>,
    backed_up: usize,
    /// Валидные подготовленные изменения (кнопка «применить только валидные»).
    valid_pending: Option<Vec<PreparedChange>>,
    can_undo: bool,
    status: Option<String>,
}

impl Report {
    /// Есть ли что показывать (зеркало условия раннего выхода `draw_report`).
    fn has_content(&self) -> bool {
        !self.parse_errors.is_empty()
            || !self.parse_warnings.is_empty()
            || !self.applied.is_empty()
            || !self.failed.is_empty()
            || self.valid_pending.is_some()
            || self.status.is_some()
    }
}

impl DownlinkState {
    pub fn new() -> Self {
        Self { response: String::new(), report: None }
    }

    pub fn draw(&mut self, ui: &mut egui::Ui, root: Option<&Path>, base: &Path) {
        ui.heading("Чат → проект");
        // Поле занимает остаток панели (минус ряд кнопок и место под отчёт):
        // фиксированный desired_rows задавал минимум ВЫСОТЫ панели, и
        // разделитель «отпрыгивал» к минимальной высоте контента.
        let report_space = if self.report.as_ref().is_some_and(Report::has_content) {
            170.0
        } else {
            0.0
        };
        let edit_height = (ui.available_height() - 44.0 - report_space).max(80.0);
        // Поле в скролле с потолком edit_height: multiline TextEdit в egui растёт
        // с контентом без ограничения (высота add_sized — только минимум), большой
        // ответ раздувал панель и уводил кнопку «Применить изменения» за экран.
        let response = egui::ScrollArea::vertical()
            .id_salt("downlink_response_scroll")
            .auto_shrink(false)
            .max_height(edit_height)
            .show(ui, |ui| {
                ui.add_sized(
                    [ui.available_width(), edit_height],
                    egui::TextEdit::multiline(&mut self.response)
                        .hint_text("Вставь ответ LLM с блоками <<<FILE: путь>>>…<<<END_FILE: путь>>>")
                        .font(egui::TextStyle::Monospace)
                        .margin(egui::Margin::same(8))
                        .id_salt("downlink_response"),
                )
            })
            .inner;
        super::text_menu::attach(&response, &self.response);
        ui.add_space(4.0);

        ui.horizontal(|ui| {
            let root_ready = root.is_some();
            // Пустое поле ответа (или из одних пробелов/переводов строк) —
            // кнопка неактивна: клик по пустому даёт бессмысленный отчёт разбора.
            let has_text = !self.response.trim().is_empty();
            if style::accent_button(
                ui,
                "Применить изменения",
                root_ready && has_text,
                style::GREEN,
                style::GREEN_HOVER,
            ) {
                if let Some(root) = root {
                    self.apply(root, base);
                }
            }
            let can_undo = self.report.as_ref().is_some_and(|r| r.can_undo);
            if style::outline_button(ui, "Откатить последний патч", can_undo, style::AMBER) {
                if let Some(root) = root {
                    self.undo(root, base);
                }
            }
            if !root_ready {
                ui.weak("сначала выбери папку проекта справа");
            }
        });
        self.draw_report(ui, root, base);

        // Добивка до конца панели: egui хранит высоту панели ПО КОНТЕНТУ, и
        // если контент короче выделенной высоты хоть на пиксель, панель
        // «сжимается» кадр за кадром (наблюдалось: 340 → 176 по ~13px/кадр,
        // драг съедался самосжатием — «разделитель отпрыгивает»).
        let rest = ui.available_height();
        if rest > 0.0 {
            ui.add_space(rest);
        }
    }

    fn draw_report(&mut self, ui: &mut egui::Ui, root: Option<&Path>, base: &Path) {
        let Some(rep) = &self.report else { return };
        if !rep.has_content() {
            return;
        }
        ui.separator();
        // Отчёт в скролле с потолком высоты: длинный отчёт не должен задавать
        // минимальную высоту панели (иначе разделитель «отпрыгивает»).
        let mut apply_valid = false;
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .max_height(150.0)
            .show(ui, |ui| {
                if let Some(status) = &rep.status {
                    ui.label(egui::RichText::new(status).strong());
                }
                if !rep.parse_errors.is_empty() {
                    ui.label(
                        egui::RichText::new(format!(
                            "Ошибки разбора ({}):",
                            rep.parse_errors.len()
                        ))
                        .color(egui::Color32::LIGHT_RED)
                        .strong(),
                    );
                    for (path, reason) in &rep.parse_errors {
                        let who = path.as_deref().unwrap_or("(без пути)");
                        ui.label(
                            egui::RichText::new(format!("{who}: {reason}"))
                                .color(egui::Color32::LIGHT_RED),
                        );
                    }
                }
                for w in &rep.parse_warnings {
                    ui.label(egui::RichText::new(format!("⚠ {w}")).color(egui::Color32::YELLOW));
                }
                if !rep.applied.is_empty() {
                    ui.label(
                        egui::RichText::new(format!("Применено ({}):", rep.applied.len()))
                            .color(egui::Color32::LIGHT_GREEN)
                            .strong(),
                    );
                    for rel in &rep.applied {
                        ui.label(format!("✓ {rel}"));
                    }
                }
                if !rep.failed.is_empty() {
                    ui.label(
                        egui::RichText::new(format!("Ошибки ({}):", rep.failed.len()))
                            .color(egui::Color32::LIGHT_RED)
                            .strong(),
                    );
                    for (path, reason) in &rep.failed {
                        ui.label(
                            egui::RichText::new(format!("✗ {path}: {reason}"))
                                .color(egui::Color32::LIGHT_RED),
                        );
                    }
                }
                if rep.backed_up > 0 {
                    ui.weak(format!("в бэкапе файлов: {} (Undo вернёт всё)", rep.backed_up));
                }

                // «Применить только валидные»: готовые prepared-изменения ждут кнопки.
                if rep.valid_pending.is_some() {
                    let count = rep.valid_pending.as_ref().map_or(0, |p| p.len());
                    let total = count + rep.failed.len();
                    ui.label(format!("Валидных изменений: {count} из {total}"));
                    ui.add_space(4.0);
                    if ui.button("Применить только валидные").clicked() {
                        apply_valid = true;
                    }
                }
            });
        if apply_valid {
            if let Some(root) = root {
                self.apply_valid_only(root, base);
            }
        }
    }

    /// Полный цикл: parse → dry-run всех изменений. При ошибках на диск не
    /// пишется ничего; валидные ждут решения владельца.
    fn apply(&mut self, root: &Path, base: &Path) {
        let parsed = parse(&self.response);
        for w in &parsed.warnings {
            log::warn!("патч: {w}");
        }
        if !parsed.errors.is_empty() {
            for e in &parsed.errors {
                log::error!("патч: {}: {}", e.path.as_deref().unwrap_or("(без пути)"), e.reason);
            }
            self.report = Some(Report {
                parse_errors: parsed
                    .errors
                    .iter()
                    .map(|e| (e.path.clone(), e.reason.clone()))
                    .collect(),
                parse_warnings: parsed.warnings,
                applied: vec![],
                failed: vec![],
                backed_up: 0,
                valid_pending: None,
                can_undo: false,
                status: Some("на диск ничего не записано".to_string()),
            });
            return;
        }
        if parsed.changes.is_empty() {
            self.report = Some(empty_report(
                "в ответе нет блоков изменений (<<<FILE: …>>> или <<<DELETE_FILE: …>>>)",
            ));
            return;
        }

        let mut ok: Vec<PreparedChange> = Vec::new();
        let mut errs: Vec<(String, String)> = Vec::new();
        for change in &parsed.changes {
            match prepare(root, change) {
                Ok(p) => ok.push(p),
                Err(e) => {
                    log::error!("патч {}: {}", e.path, e.reason);
                    errs.push((e.path.clone(), e.reason.clone()));
                }
            }
        }

        if !errs.is_empty() {
            let valid = if ok.is_empty() { None } else { Some(ok) };
            self.report = Some(Report {
                parse_errors: vec![],
                parse_warnings: parsed.warnings,
                applied: vec![],
                failed: errs,
                backed_up: 0,
                valid_pending: valid,
                can_undo: false,
                status: Some("на диск ничего не записано".to_string()),
            });
            return;
        }
        self.run_apply(ok, root, base, parsed.warnings);
    }

    fn apply_valid_only(&mut self, root: &Path, base: &Path) {
        let Some(rep) = self.report.as_mut() else { return };
        let Some(prepared) = rep.valid_pending.take() else { return };
        let warnings = std::mem::take(&mut rep.parse_warnings);
        self.run_apply(prepared, root, base, warnings);
    }

    /// Фаза 2: запись (внутри — бэкап и полный откат при ошибке записи).
    fn run_apply(
        &mut self,
        prepared: Vec<PreparedChange>,
        root: &Path,
        base: &Path,
        parse_warnings: Vec<String>,
    ) {
        let backup_root = backup_root(base);
        let report: ApplyReport = apply(root, &backup_root, &prepared);
        log::info!(
            "патч применён: {}, ошибок {}, в бэкапе {}",
            report.applied.len(),
            report.failed.len(),
            report.backed_up.len()
        );
        for f in &report.failed {
            log::error!("патч {}: {}", f.path, f.reason);
        }
        let status = if report.failed.is_empty() {
            Some(format!("применено файлов: {}", report.applied.len()))
        } else {
            Some("запись не выполнена (полный откат) — см. ошибки".to_string())
        };
        self.report = Some(Report {
            parse_errors: vec![],
            parse_warnings,
            applied: report.applied,
            failed: report
                .failed
                .into_iter()
                .map(|f| (f.path, f.reason))
                .collect(),
            backed_up: report.backed_up.len(),
            valid_pending: None,
            can_undo: !report.backed_up.is_empty(),
            status,
        });
    }

    fn undo(&mut self, root: &Path, base: &Path) {
        match undo_last(root, &backup_root(base)) {
            Ok(n) => {
                log::info!("откат: восстановлено файлов {n}");
                if let Some(rep) = self.report.as_mut() {
                    rep.can_undo = false;
                    rep.status = Some(format!("откат выполнен: восстановлено файлов {n}"));
                }
            }
            Err(e) => {
                log::error!("откат: {e}");
                if let Some(rep) = self.report.as_mut() {
                    rep.status = Some(format!("откат не выполнен: {e}"));
                }
            }
        }
    }
}

fn backup_root(base: &Path) -> PathBuf {
    base.join(BACKUP_DIR)
}

fn empty_report(status: &str) -> Report {
    Report {
        parse_errors: vec![],
        parse_warnings: vec![],
        applied: vec![],
        failed: vec![],
        backed_up: 0,
        valid_pending: None,
        can_undo: false,
        status: Some(status.to_string()),
    }
}
