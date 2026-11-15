//! Uplink (проект → чат) — стейт GPUI-«Главной» (PRP 14.11.26 шаг 3): папка
//! проекта, скан, отметки файлов, токены, опции промпта, сборка и копирование
//! в буфер. Логика перенесена 1:1 из `ui/uplink.rs` (egui); рендер —
//! `main_view.rs`, данные дерева — `tree_items.rs`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use agent_core::tree::prompt::{build_prompt, find_instruction_path, load_instruction};
use agent_core::tree::scanner::{FileEntry, FileStatus, ScanOutput};
use agent_core::tree::structure::{generate_all, generate_selected};
use agent_core::tree::tokens::spaced;

use super::draft_state::DraftState;
use crate::clipboard;
use crate::ui::uplink_text::{normalize_ai_path, truncate_for_status};
use crate::worker::Command;

/// Файл инструкции метода (внешний md рядом с exe, требование владельца).
const METHOD_FILE: &str = "markdown_method.md";

/// Состояние uplink-части главной вкладки GPUI.
pub struct UplinkState {
    root: Option<PathBuf>,
    scan: Option<ScanOutput>,
    checked: HashSet<String>,
    /// Раскрытые папки (rel с `/` на конце): раскрытие ведём СВОИМ стейтом
    /// (библиотечный toggle_expand приватен; клики — li.on_click в main_view
    /// + синхронизация TreeEvent в root_view).
    pub expanded: HashSet<String>,
    tokens: HashMap<String, usize>,
    pub structure_checked: bool,
    /// true — структура по всем файлам, false — только по отмеченным.
    pub structure_all: bool,
    pub instruction_checked: bool,
    instruction_text: Option<String>,
    instruction_error: Option<String>,
    /// Выключен — отмеченные файлы не входят в промпт; галочки дерева не снимаются.
    pub include_selected: bool,
    status: Option<String>,
}

impl UplinkState {
    pub fn new() -> Self {
        Self {
            root: None,
            scan: None,
            checked: HashSet::new(),
            expanded: HashSet::new(),
            tokens: HashMap::new(),
            structure_checked: true,
            structure_all: false,
            instruction_checked: true,
            instruction_text: None,
            instruction_error: None,
            include_selected: true,
            status: None,
        }
    }

    /// Текущая папка проекта (для Downlink: применяем правки к ней же).
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// Скан завершён (активность AI-подбора в черновике).
    pub fn scan_ready(&self) -> bool {
        self.scan.is_some()
    }

    pub fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }

    /// Данные для снапшота строк дерева (tree_items::build_rows).
    pub fn scan(&self) -> Option<&ScanOutput> {
        self.scan.as_ref()
    }

    pub fn checked(&self) -> &HashSet<String> {
        &self.checked
    }

    pub fn tokens(&self) -> &HashMap<String, usize> {
        &self.tokens
    }

    /// Открыть папку: сброс стейта, запрос скана, сохранение в конфиг.
    pub fn open_root(&mut self, root: &Path, base: &Path, tx: &Sender<Command>) {
        self.root = Some(root.to_path_buf());
        self.scan = None;
        self.checked.clear();
        self.expanded.clear(); // новый проект — дерево свёрнуто
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
    /// Отмечаются все валидные файлы (паритет со старой версией).
    pub fn on_scanned(&mut self, root: &Path, out: ScanOutput) {
        if self.root.as_deref() != Some(root) {
            return;
        }
        for f in &out.files {
            if matches!(f.status, FileStatus::Ok | FileStatus::ExcludedByDefault) {
                self.checked.insert(f.rel_path.clone());
            }
        }
        self.scan = Some(out);
        self.status = None;
    }

    /// Раскрыть/свернуть папку (rel с `/` на конце). Журнал — проверка, что
    /// клики по строкам дерева доходят до стейта.
    pub fn toggle_expanded(&mut self, rel: &str) {
        if self.expanded.remove(rel) {
            log::info!("дерево: свёрнуто {rel}");
        } else {
            self.expanded.insert(rel.to_string());
            log::info!("дерево: раскрыто {rel}");
        }
    }

    /// Сет раскрытых папок (tree_items::build_rows / build_tree_items).
    pub fn expanded(&self) -> &HashSet<String> {
        &self.expanded
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

    /// Отметить/снять один файл (только selectable — проверка по скану).
    pub fn set_file_checked(&mut self, rel: &str, add: bool) {
        if self.is_selectable(rel) {
            if add {
                self.checked.insert(rel.to_string());
            } else {
                self.checked.remove(rel);
            }
        }
    }

    /// Каскад папки: отметить/снять все selectable-файлы под префиксом.
    pub fn set_dir_checked(&mut self, prefix: &str, add: bool) {
        let Some(scan) = &self.scan else { return };
        for f in &scan.files {
            if f.rel_path.starts_with(prefix) && matches!(f.status, FileStatus::Ok | FileStatus::ExcludedByDefault) {
                if add {
                    self.checked.insert(f.rel_path.clone());
                } else {
                    self.checked.remove(&f.rel_path);
                }
            }
        }
    }

    /// Отметить всё / снять всё (только selectable; паритет с egui check_all).
    pub fn check_all(&mut self, add: bool) {
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

    /// Запросить пересчёт токенов всех отмеченных файлов.
    pub fn request_tokens(&mut self, tx: &Sender<Command>) {
        let Some(root) = self.root.clone() else { return };
        for rel in &self.checked {
            if let Err(e) = tx.send(Command::Tokens { root: root.clone(), rel: rel.clone() }) {
                log::error!("запрос токенов не отправлен: {e}");
            }
        }
    }

    /// Строка «Выделено токенов: … (+N не посчитано)» для тулбара.
    pub fn total_tokens_text(&self) -> String {
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
        text
    }

    /// Подсказка под чекбоксом инструкции: ошибка красным или счётчик символов.
    pub fn instruction_hint(&self) -> Option<(String, bool)> {
        if !self.instruction_checked {
            return None;
        }
        match &self.instruction_error {
            Some(err) => Some((err.clone(), true)),
            None => self
                .instruction_text
                .as_ref()
                .map(|t| (format!("инструкция: {} символов", t.len()), false)),
        }
    }

    /// Включили чекбокс инструкции — (пере)читать файл с диска. Ошибка видима,
    /// не молчит (паритет с egui refresh_instruction).
    pub fn refresh_instruction(&mut self, base: &Path) {
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

    fn is_selectable(&self, rel: &str) -> bool {
        match &self.scan {
            Some(scan) => scan
                .files
                .iter()
                .any(|f| f.rel_path == rel && matches!(f.status, FileStatus::Ok | FileStatus::ExcludedByDefault)),
            None => false,
        }
    }

    /// Собрать промпт (проект-часть: инструкция → структура → файлы;
    /// черновик-часть: текст по «Промпт» + вложения) и скопировать в буфер.
    /// Единая кнопка копирования (паритет с egui copy_prompt).
    pub fn copy_prompt(&mut self, base: &Path, draft: &DraftState) {
        let Some(root) = self.root.clone() else {
            self.status = Some("папка проекта не выбрана".to_string());
            return;
        };
        let Some(scan) = &self.scan else {
            self.status = Some("проект ещё не отсканирован".to_string());
            return;
        };
        // Мастер-чекбокс выключен — файлы исключаются из копии, галочки дерева не трогаются.
        let selected: Vec<FileEntry> = if self.include_selected {
            scan.files
                .iter()
                .filter(|f| self.checked.contains(&f.rel_path))
                .cloned()
                .collect()
        } else {
            Vec::new()
        };

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
                    self.status = Some("инструкция не прочитана (см. ошибку выше)".to_string());
                    return;
                }
            }
        } else {
            None
        };

        let project = build_prompt(&root, &selected, structure, instruction);
        for w in &project.warnings {
            log::warn!("промпт: {w}");
        }
        let draft_parts = match draft.text_parts() {
            Ok(parts) => parts,
            Err(e) => {
                log::error!("копия: черновик: {e}");
                self.status = Some(e);
                return;
            }
        };
        let draft_text = draft_parts.join("\n\n");
        log::info!(
            "копия: проект {} симв. (файлов {}/{}), черновик {} симв.",
            project.text.len(),
            project.file_count,
            selected.len(),
            draft_text.len()
        );
        let mut text = project.text;
        if !draft_text.is_empty() {
            if !text.trim().is_empty() {
                text.push_str("\n\n");
            }
            text.push_str(&draft_text);
        }
        if text.trim().is_empty() {
            let msg = "нечего копировать — нет ни одного включённого блока".to_string();
            log::warn!("копия: {msg}");
            self.status = Some(msg);
            return;
        }
        match clipboard::copy_text(&text) {
            Ok(()) => {
                let mut msg = format!(
                    "скопировано: {} файлов, {} символов",
                    project.file_count,
                    text.len()
                );
                if self.include_selected && self.checked.is_empty() {
                    msg.push_str("; файлы проекта: ни один не отмечен");
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::tree::scanner::FileEntry;

    fn scan() -> ScanOutput {
        ScanOutput {
            dirs: Vec::new(),
            files: vec![
                FileEntry { rel_path: "a.rs".into(), size: 10, status: FileStatus::Ok },
                FileEntry { rel_path: "dir/b.rs".into(), size: 10, status: FileStatus::Ok },
                FileEntry { rel_path: "dir/c.bin".into(), size: 10, status: FileStatus::Binary },
                FileEntry {
                    rel_path: "big.py".into(),
                    size: 10,
                    status: FileStatus::ExcludedByDefault,
                },
            ],
        }
    }

    fn state_with_scan() -> (UplinkState, PathBuf) {
        let mut st = UplinkState::new();
        let root = PathBuf::from("D:/tmp/proj");
        st.root = Some(root.clone());
        st.on_scanned(&root, scan());
        (st, root)
    }

    #[test]
    fn initial_checked_ok_and_excluded() {
        let (st, _) = state_with_scan();
        // Ok и ExcludedByDefault отмечены; бинарный — нет.
        assert!(st.checked.contains("a.rs"));
        assert!(st.checked.contains("big.py"));
        assert!(!st.checked.contains("dir/c.bin"));
    }

    #[test]
    fn stale_scan_ignored() {
        let mut st = UplinkState::new();
        st.root = Some(PathBuf::from("D:/tmp/other"));
        st.on_scanned(&PathBuf::from("D:/tmp/proj"), scan());
        assert!(st.scan.is_none());
        assert!(st.checked.is_empty());
    }

    #[test]
    fn ai_pick_adds_without_unchecking_and_skips_ghosts() {
        let (mut st, root) = state_with_scan();
        // Ручные снятия: a.rs (ИИ не должен вернуть) и dir/b.rs (должен
        // вернуть — иначе добавления не будет: скан отмечает всё сразу).
        st.checked.remove("a.rs");
        st.checked.remove("dir/b.rs");
        st.on_ai_picked(
            &root,
            &["dir\\b.rs".to_string(), "ghost.rs".to_string()],
            "reasoning",
        );
        // dir/b.rs нормализовалась и вернулась (added 1 из 2); a.rs осталась
        // снятой (ИИ не трогает ручные); ghost.rs вне скана — пропущена.
        assert!(st.checked.contains("dir/b.rs"));
        assert!(!st.checked.contains("a.rs"));
        assert!(st.status.as_deref().unwrap_or("").contains("ИИ отметил 1 из 2"));
    }

    #[test]
    fn dir_cascade_and_totals() {
        let (mut st, _) = state_with_scan();
        st.set_dir_checked("dir/", true);
        assert!(st.checked.contains("dir/b.rs"));
        assert!(!st.checked.contains("dir/c.bin")); // бинарный не трогаем
        st.set_dir_checked("dir/", false);
        assert!(!st.checked.contains("dir/b.rs"));
        st.set_file_checked("dir/c.bin", true);
        assert!(!st.checked.contains("dir/c.bin")); // не-selectable
        assert!(st.total_tokens_text().starts_with("Выделено токенов:"));
    }
}
