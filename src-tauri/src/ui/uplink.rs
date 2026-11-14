//! Uplink (проект → чат): папка проекта, дерево с чекбоксами, токены,
//! опции промпта (структура/инструкция), сборка и копирование в буфер.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use tauri_plugin_egui::egui;

use agent_core::tree::prompt::{build_prompt, find_instruction_path, load_instruction};
use agent_core::tree::scanner::{FileEntry, FileStatus, ScanOutput};
use agent_core::tree::structure::{generate_all, generate_selected};
use agent_core::tree::tokens::spaced;

use super::style;
use super::tree_widget::TreeData;
use crate::clipboard;
use crate::worker::Command;

/// Файл инструкции метода (внешний md рядом с exe, требование владельца).
const METHOD_FILE: &str = "markdown_method.md";

/// Состояние Uplink-части главной вкладки.
pub struct UplinkState {
    root: Option<PathBuf>,
    scan: Option<ScanOutput>,
    tree: Option<TreeData>,
    checked: HashSet<String>,
    tokens: HashMap<String, usize>,
    structure_checked: bool,
    /// true — структура по всем файлам, false — только по отмеченным.
    structure_all: bool,
    instruction_checked: bool,
    instruction_text: Option<String>,
    instruction_error: Option<String>,
    status: Option<String>,
}

impl UplinkState {
    pub fn new() -> Self {
        Self {
            root: None,
            scan: None,
            tree: None,
            checked: HashSet::new(),
            tokens: HashMap::new(),
            structure_checked: true,
            structure_all: false,
            instruction_checked: true,
            instruction_text: None,
            instruction_error: None,
            status: None,
        }
    }

    /// Текущая папка проекта (для Downlink: применяем правки к ней же).
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// Скан завершён (активность кнопки AI-подбора в черновике).
    pub fn scan_ready(&self) -> bool {
        self.scan.is_some()
    }

    /// Открыть папку: сброс стейта, запрос скана, сохранение в конфиг.
    pub fn open_root(&mut self, root: &Path, base: &Path, tx: &Sender<Command>) {
        self.root = Some(root.to_path_buf());
        self.scan = None;
        self.tree = None;
        self.checked.clear();
        self.tokens.clear();
        self.status = Some(format!("сканирую {}…", root.display()));
        if let Err(e) = tx.send(Command::Scan { root: root.to_path_buf() }) {
            log::error!("команда скана не отправлена: {e}");
            self.status = Some("скан не запустился (воркер мёртв?) — см. лог".to_string());
        }
        let dir = root.display().to_string();
        let mut cfg = agent_core::config::load(base);
        if cfg.last_project_dir.as_deref() != Some(dir.as_str()) {
            cfg.last_project_dir = Some(dir);
            if let Err(e) = agent_core::config::save(base, &cfg) {
                log::error!("конфиг не сохранён: {e}");
            }
        }
    }

    /// Событие воркера: скан готов. Устаревшие сканы (другая папка) игнорируем.
    pub fn on_scanned(&mut self, root: &Path, out: ScanOutput) {
        if self.root.as_deref() != Some(root) {
            return;
        }
        // Отмечаются все валидные (старая версия: CHECKED всем, кроме
        // бинарных/больших/ошибок — «исключённые по умолчанию» тоже чекбоксились).
        for f in &out.files {
            if matches!(f.status, FileStatus::Ok | FileStatus::ExcludedByDefault) {
                self.checked.insert(f.rel_path.clone());
            }
        }
        self.tree = Some(TreeData::build(&out));
        self.scan = Some(out);
        self.status = None;
    }

    /// Событие воркера: токены одного файла посчитаны.
    pub fn on_tokens(&mut self, root: &Path, rel: &str, count: Option<usize>) {
        if self.root.as_deref() != Some(root) {
            return;
        }
        match count {
            Some(c) => {
                self.tokens.insert(rel.to_string(), c);
            }
            None => {
                self.tokens.remove(rel);
            }
        }
    }

    /// Событие воркера: промежуточный статус AI-подбора (в строку статуса).
    pub fn on_ai_status(&mut self, root: &Path, msg: &str) {
        if self.root.as_deref() != Some(root) {
            return;
        }
        self.status = Some(msg.to_string());
    }

    /// Относительные пути selectable-файлов (Ok | ExcludedByDefault) — источник
    /// истинности для тулов AI-подбора: find_files ищет по этому списку,
    /// оркестратор отсекает галлюцинации модели. Скан не готов — пустой.
    pub fn selectable_relative_paths(&self) -> Vec<String> {
        match &self.scan {
            Some(scan) => scan
                .files
                .iter()
                .filter(|f| matches!(f.status, FileStatus::Ok | FileStatus::ExcludedByDefault))
                .map(|f| f.rel_path.clone())
                .collect(),
            None => Vec::new(),
        }
    }

    /// Событие воркера: AI-подбор не удался (текст уже в логе воркером).
    pub fn on_ai_failed(&mut self, root: &Path, msg: &str) {
        if self.root.as_deref() != Some(root) {
            return;
        }
        self.status = Some(format!("⚠ {msg}"));
    }

    /// Событие воркера: AI-подбор готов. Ручные галочки НЕ снимаются —
    /// только дополняются (требование 3 PRP); пути вне скана (галлюцинации
    /// модели) — в лог, в выбор не попадают.
    pub fn on_ai_picked(&mut self, root: &Path, selected: &[String], reasoning: &str) {
        if self.root.as_deref() != Some(root) {
            return;
        }
        // Своё множество валидных путей (заимствование scan живёт до конца
        // чтения — дальше только мутация checked).
        let selectable: HashSet<String> = match &self.scan {
            Some(scan) => scan
                .files
                .iter()
                .filter(|f| matches!(f.status, FileStatus::Ok | FileStatus::ExcludedByDefault))
                .map(|f| f.rel_path.clone())
                .collect(),
            None => HashSet::new(),
        };
        let mut added = 0usize;
        let mut skipped: Vec<String> = Vec::new();
        for raw in selected {
            let rel = normalize_ai_path(raw);
            if rel.is_empty() {
                continue;
            }
            if selectable.contains(&rel) {
                if self.checked.insert(rel) {
                    added += 1;
                }
            } else {
                skipped.push(raw.clone());
            }
        }
        log::info!(
            "ai-pick: отмечено {} из {} предложенных (ручные не тронуты); вне скана: {:?}",
            added,
            selected.len(),
            skipped
        );
        let n = selected.len();
        self.status = Some(if n == 0 {
            format!("ИИ не выбрал файлов: {}", truncate_for_status(reasoning, 160))
        } else if added == 0 {
            format!("ИИ подтвердил уже отмеченные ({n})")
        } else {
            format!("ИИ отметил {added} из {n}: {}", truncate_for_status(reasoning, 160))
        });
    }

    pub fn draw(&mut self, ui: &mut egui::Ui, base: &Path, tx: &Sender<Command>) {
        ui.heading("Проект → чат");
        self.draw_root_row(ui, base, tx);
        ui.separator();
        self.draw_toolbar(ui, tx);

        // Опции и кнопка «Собрать и скопировать» — в нижней панели: обычные
        // виджеты после ScrollArea вытеснялись за нижний край (ScrollArea без
        // ограничения высоты забирает всё место — кнопку не было видно).
        // Панель egui честно резервирует низ, дерево скроллится в остатке.
        egui::TopBottomPanel::bottom("uplink_actions")
            .frame(egui::Frame::new())
            .show_inside(ui, |ui| {
                ui.separator();
                self.draw_options(ui, base);
                self.draw_copy_row(ui, base);
            });

        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            match (&mut self.tree, &self.scan) {
                (Some(tree), Some(scan)) => tree.draw(ui, scan, &mut self.checked, &self.tokens),
                _ => {
                    ui.weak("Выбери папку проекта — появится дерево файлов.");
                }
            }
        });
    }

    fn draw_root_row(&mut self, ui: &mut egui::Ui, base: &Path, tx: &Sender<Command>) {
        // Кнопки строкой, путь — следующей строкой с обрезкой: длинный путь
        // не должен задавать минимальную ширину панели (egui держит панель
        // по контенту — иначе ресайз «отпрыгивает» к ширине контента).
        ui.horizontal(|ui| {
            if ui.button("Выбрать папку…").clicked() {
                if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                    self.open_root(&dir, base, tx);
                }
            }
            if ui.button("Обновить").clicked() {
                if let Some(root) = self.root.clone() {
                    if let Err(e) = tx.send(Command::Scan { root }) {
                        log::error!("команда скана не отправлена: {e}");
                    }
                }
            }
        });
        match &self.root {
            Some(root) => {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(root.display().to_string()).monospace(),
                    )
                    .truncate(),
                );
            }
            None => {
                ui.weak("папка не выбрана");
            }
        }
    }

    fn draw_toolbar(&mut self, ui: &mut egui::Ui, tx: &Sender<Command>) {
        // Кнопки одной строкой, итог токенов — следующей: суммарная ширина
        // ряда задаёт минимальную ширину панели, перегруженный ряд делал
        // панель «резиновой» (ресайз прыгал к ширине контента).
        ui.horizontal(|ui| {
            if ui.button("Отметить всё").clicked() {
                self.check_all(true);
            }
            if ui.button("Снять всё").clicked() {
                self.check_all(false);
            }
            if ui.button("Посчитать токены").clicked() {
                self.request_tokens(tx);
            }
        });
        self.draw_total_tokens(ui);
    }

    fn check_all(&mut self, add: bool) {
        let Some(scan) = &self.scan else { return };
        for f in &scan.files {
            if matches!(f.status, FileStatus::Ok | FileStatus::ExcludedByDefault) {
                if add {
                    self.checked.insert(f.rel_path.clone());
                } else {
                    self.checked.remove(&f.rel_path);
                }
            }
        }
    }

    fn request_tokens(&mut self, tx: &Sender<Command>) {
        let Some(root) = self.root.clone() else { return };
        for rel in &self.checked {
            if let Err(e) = tx.send(Command::Tokens { root: root.clone(), rel: rel.clone() }) {
                log::error!("запрос токенов не отправлен: {e}");
            }
        }
    }

    fn draw_total_tokens(&mut self, ui: &mut egui::Ui) {
        let total: usize = self.checked.iter().filter_map(|r| self.tokens.get(r)).sum();
        let counted = self.checked.iter().filter(|r| self.tokens.contains_key(*r)).count();
        let missing = self.checked.len().saturating_sub(counted);
        // Сумма выделения — без порога, как в старой версии
        // (treeview_logic.py update_selected_tokens_display): порог касается
        // только отображения отдельных файлов в дереве.
        let mut text = format!("Выделено токенов: {}", spaced(total));
        if missing > 0 {
            text.push_str(&format!(" (+{missing} не посчитано)"));
        }
        ui.label(text);
    }

    fn draw_options(&mut self, ui: &mut egui::Ui, base: &Path) {
        ui.checkbox(&mut self.structure_checked, "Включить структуру");
        ui.add_enabled_ui(self.structure_checked, |ui| {
            ui.horizontal(|ui| {
                ui.weak("по:");
                ui.radio_value(&mut self.structure_all, false, "отмеченным");
                ui.radio_value(&mut self.structure_all, true, "всем файлам");
            });
        });

        let before = self.instruction_checked;
        ui.checkbox(&mut self.instruction_checked, "Включить инструкцию метода");
        if self.instruction_checked && !before {
            self.refresh_instruction(base);
        }
        if self.instruction_checked {
            match &self.instruction_error {
                Some(err) => {
                    ui.label(egui::RichText::new(err).color(egui::Color32::LIGHT_RED));
                }
                None => {
                    if let Some(text) = &self.instruction_text {
                        ui.weak(format!("инструкция: {} символов", text.len()));
                    }
                }
            }
        }
    }

    /// (Пере)читать инструкцию метода с диска; ошибка — видима, не молчит.
    fn refresh_instruction(&mut self, base: &Path) {
        let result = find_instruction_path(base, METHOD_FILE)
            .ok_or_else(|| format!("doc/{METHOD_FILE} не найден рядом с exe (и выше по папкам)"))
            .and_then(|p| load_instruction(&p).map(|text| (text, p)));
        match result {
            Ok((text, path)) => {
                log::info!("инструкция метода: {}", path.display());
                self.instruction_text = Some(text);
                self.instruction_error = None;
            }
            Err(e) => {
                log::warn!("инструкция метода: {e}");
                self.instruction_text = None;
                self.instruction_error = Some(e);
            }
        }
    }

    fn draw_copy_row(&mut self, ui: &mut egui::Ui, base: &Path) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if style::accent_button(
                ui,
                "Собрать и скопировать",
                true,
                style::BLUE,
                style::BLUE_HOVER,
            ) {
                self.copy_prompt(base);
            }
            if let Some(status) = &self.status {
                ui.label(status);
            }
        });
    }

    /// Собрать промпт (порядок — как в старой версии, формат блоков v2) и
    /// скопировать в буфер.
    fn copy_prompt(&mut self, base: &Path) {
        let Some(root) = self.root.clone() else {
            self.status = Some("папка проекта не выбрана".to_string());
            return;
        };
        let Some(scan) = &self.scan else {
            self.status = Some("проект ещё не отсканирован".to_string());
            return;
        };
        let selected: Vec<FileEntry> = scan
            .files
            .iter()
            .filter(|f| self.checked.contains(&f.rel_path))
            .cloned()
            .collect();
        if selected.is_empty() {
            self.status = Some("не отмечено ни одного файла".to_string());
            return;
        }

        let structure = if self.structure_checked {
            if self.structure_all {
                generate_all(&root)
            } else {
                let rels: Vec<String> = self.checked.iter().cloned().collect();
                let root_name = root
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                generate_selected(&root_name, &rels)
            }
        } else {
            None
        };

        let instruction = if self.instruction_checked {
            if self.instruction_text.is_none() {
                self.refresh_instruction(base);
            }
            match self.instruction_text.clone() {
                Some(text) => Some(text),
                None => {
                    self.status =
                        Some("инструкция не прочитана (см. ошибку выше)".to_string());
                    return;
                }
            }
        } else {
            None
        };

        let result = build_prompt(&root, &selected, structure, instruction);
        for w in &result.warnings {
            log::warn!("промпт: {w}");
        }
        match clipboard::copy_text(&result.text) {
            Ok(()) => {
                let msg = format!("скопировано: {} файлов, {} символов", result.file_count, result.text.len());
                log::info!("промпт: {msg}");
                self.status = Some(msg);
            }
            Err(e) => {
                log::error!("промпт: {e}");
                self.status = Some(e);
            }
        }
    }
}

/// Нормализовать предложенный ИИ путь: `\` → `/`, без ведущего `./`.
fn normalize_ai_path(p: &str) -> String {
    let p = p.trim().replace('\\', "/");
    p.strip_prefix("./").unwrap_or(&p).to_string()
}

/// Reasoning для строки статуса: обрезка до `max_chars` символов
/// (полный текст — в логе воркера).
fn truncate_for_status(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max_chars).collect();
    t.push('…');
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_slashes_and_dot() {
        assert_eq!(normalize_ai_path("src\\main.rs"), "src/main.rs");
        assert_eq!(normalize_ai_path("./src/main.rs"), "src/main.rs");
        assert_eq!(normalize_ai_path("  a/b.rs  "), "a/b.rs");
        assert_eq!(normalize_ai_path(""), "");
        // Абсолютные пути не чинятся в относительные — их отсеет скан-фильтр.
        assert_eq!(normalize_ai_path("D:\\proj\\a.rs"), "D:/proj/a.rs");
    }

    #[test]
    fn truncate_keeps_short_and_cuts_long() {
        assert_eq!(truncate_for_status("короткий", 10), "короткий");
        let long = "ж".repeat(200);
        let cut = truncate_for_status(&long, 160);
        assert_eq!(cut.chars().count(), 161); // 160 + многоточие
        assert!(cut.ends_with('…'));
    }
}
