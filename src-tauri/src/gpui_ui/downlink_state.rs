//! Downlink (чат → проект) — стейт GPUI-«Главной» (PRP 14.11.26 шаг 3):
//! ответ LLM, двухфазное применение правок (dry-run → запись с бэкапом),
//! «применить только валидные», Undo. Логика 1:1 из `ui/downlink.rs` (egui),
//! но parse/prepare/apply выполняются в ФОНОВОЙ задаче gpui (PRP-требование):
//! `compute_report`/`apply_valid_report` — статические функции без `self`,
//! результат доставляется через `WeakEntity::update`; рендер — `side_cards.rs`.

use std::path::{Path, PathBuf};

use agent_core::patch::{apply, parse, prepare, undo_last, ApplyReport, PreparedChange};

/// Папка бэкапа последнего патча (относительно base — папки exe).
const BACKUP_DIR: &str = r"backup\last_patch";

fn backup_root(base: &Path) -> PathBuf {
    base.join(BACKUP_DIR)
}

/// Результат разбора/применения для показа в UI.
pub struct Report {
    pub parse_errors: Vec<(Option<String>, String)>,
    pub parse_warnings: Vec<String>,
    pub applied: Vec<String>,
    pub failed: Vec<(String, String)>,
    pub backed_up: usize,
    /// Валидные подготовленные изменения (кнопка «применить только валидные»).
    pub valid_pending: Option<Vec<PreparedChange>>,
    pub can_undo: bool,
    pub status: Option<String>,
}

impl Report {
    /// Есть ли что показывать (зеркало условия раннего выхода egui draw_report).
    pub fn has_content(&self) -> bool {
        !self.parse_errors.is_empty()
            || !self.parse_warnings.is_empty()
            || !self.applied.is_empty()
            || !self.failed.is_empty()
            || self.valid_pending.is_some()
            || self.status.is_some()
    }
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

/// Состояние downlink-части главной вкладки GPUI.
pub struct DownlinkState {
    /// Текст ответа LLM (синхронизируется подпиской InputEvent::Change).
    pub response: String,
    /// Отчёт последнего разбора/применения (показ + кнопки).
    report: Option<Report>,
    /// Идёт фоновое применение (кнопка «Применить» задизейблена).
    pub applying: bool,
}

impl DownlinkState {
    pub fn new() -> Self {
        Self { response: String::new(), report: None, applying: false }
    }

    /// Кнопка «Применить» активна: есть непустой текст.
    pub fn can_apply(&self) -> bool {
        !self.response.trim().is_empty()
    }

    pub fn report(&self) -> Option<&Report> {
        self.report.as_ref()
    }

    /// Отчёт готов (из фоновой задачи): заменить и снять флаг «применяю…».
    pub fn set_report(&mut self, report: Report) {
        self.applying = false;
        self.report = Some(report);
    }

    /// Забрать отложенные валидные изменения (клик «Применить только валидные»)
    /// вместе с сохранёнными предупреждениями разбора.
    pub fn take_valid_pending(&mut self) -> Option<(Vec<PreparedChange>, Vec<String>)> {
        let rep = self.report.as_mut()?;
        let prepared = rep.valid_pending.take()?;
        Some((prepared, std::mem::take(&mut rep.parse_warnings)))
    }

    /// Полный цикл parse → prepare → apply (без self — исполняется в фоновой
    /// задаче). При ошибках на диск не пишется ничего; валидные ждут решения
    /// владельца.
    pub fn compute_report(root: &Path, base: &Path, text: &str) -> Report {
        let parsed = parse(text);
        for w in &parsed.warnings {
            log::warn!("патч: {w}");
        }
        if !parsed.errors.is_empty() {
            for e in &parsed.errors {
                log::error!("патч: {}: {}", e.path.as_deref().unwrap_or("(без пути)"), e.reason);
            }
            return Report {
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
            };
        }
        if parsed.changes.is_empty() {
            return empty_report(
                "в ответе нет блоков изменений (<<<FILE: …>>> или <<<DELETE_FILE: …>>>)",
            );
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
            return Report {
                parse_errors: vec![],
                parse_warnings: parsed.warnings,
                applied: vec![],
                failed: errs,
                backed_up: 0,
                valid_pending: valid,
                can_undo: false,
                status: Some("на диск ничего не записано".to_string()),
            };
        }
        Self::apply_valid_report(root, base, ok, parsed.warnings)
    }

    /// Фаза 2: запись (внутри — бэкап и полный откат при ошибке записи).
    /// Без self — тоже для фоновой задачи.
    pub fn apply_valid_report(
        root: &Path,
        base: &Path,
        prepared: Vec<PreparedChange>,
        parse_warnings: Vec<String>,
    ) -> Report {
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
        Report {
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
        }
    }

    /// Откат последнего патча (быстрая операция — выполняется в обработчике
    /// клика, как в egui).
    pub fn undo(&mut self, root: &Path, base: &Path) {
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
