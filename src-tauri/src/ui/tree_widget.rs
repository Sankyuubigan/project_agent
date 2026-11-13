//! Виджет дерева с чекбоксами: 3 состояния у папок (вкл/выкл/частично),
//! каскадная отметка, статусы файлов, токены в строке.
//!
//! Повторяет поведение Tkinter-дерева старой версии: свёрнуто по умолчанию
//! (`open=False` у каждого узла), треугольник раскрытия крайней левой
//! колонкой, ступеньки отступа у вложенных уровней, папки первыми.

use std::collections::{HashMap, HashSet};

use tauri_plugin_egui::egui;

use agent_core::tree::scanner::{FileStatus, ScanOutput};
use agent_core::tree::tokens::{spaced, MAX_TOKENS_FOR_DISPLAY};

/// Клетка колонок треугольника/чекбокса — фиксированная, чтобы имена файлов
/// и папок стояли ровно друг под другом независимо от ширины глифов.
const CELL: f32 = 18.0;

/// Готовое к отрисовке дерево.
pub struct TreeData {
    root: DirNode,
    /// Раскрытые папки (rel с `/` на конце). Пусто — всё свёрнуто.
    open: HashSet<String>,
}

#[derive(Default)]
struct DirNode {
    /// Подпапки: (имя, узел). Сортируются без учёта регистра в `collect_desc`.
    dirs: Vec<(String, DirNode)>,
    /// Индексы файлов этой папки (в scan.files).
    files: Vec<usize>,
    /// Индексы ВСЕХ файлов под этой папкой (для каскада и 3-состояния).
    desc_files: Vec<usize>,
}

impl TreeData {
    pub fn build(scan: &ScanOutput) -> Self {
        let mut root = DirNode::default();
        for (idx, f) in scan.files.iter().enumerate() {
            insert_file(&mut root, &f.rel_path, idx);
        }
        collect_desc(&mut root);
        // Всё свёрнуто: в старой версии каждый узел вставлялся с open=False.
        Self { root, open: HashSet::new() }
    }

    /// Отрисовать дерево; отметки — общий `checked` UplinkState.
    pub fn draw(
        &mut self,
        ui: &mut egui::Ui,
        scan: &ScanOutput,
        checked: &mut HashSet<String>,
        tokens: &HashMap<String, usize>,
    ) {
        // Папки первыми (как в старом дереве), затем файлы корня.
        let mut open = std::mem::take(&mut self.open);
        draw_dirs(ui, &self.root, "", scan, checked, tokens, &mut open);
        self.open = open;
        for &idx in &self.root.files {
            file_row(ui, scan, idx, checked, tokens);
        }
    }
}

fn insert_file(root: &mut DirNode, rel: &str, idx: usize) {
    let (dir, _name) = match rel.rsplit_once('/') {
        Some((d, n)) => (d, n),
        None => {
            root.files.push(idx);
            return;
        }
    };
    let mut cur = root;
    for seg in dir.split('/') {
        let pos = cur.dirs.iter().position(|(n, _)| n == seg);
        let i = match pos {
            Some(i) => i,
            None => {
                cur.dirs.push((seg.to_string(), DirNode::default()));
                cur.dirs.len() - 1 // только что push — список не пуст
            }
        };
        cur = &mut cur.dirs[i].1;
    }
    cur.files.push(idx);
}

/// Собрать desc_files (все файлы под папкой, включая подпапки) и отсортировать
/// подпапки без учёта регистра (как в старой версии: name.lower()).
fn collect_desc(node: &mut DirNode) {
    node.dirs.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    let mut all = node.files.clone();
    for (_, child) in node.dirs.iter_mut() {
        collect_desc(child);
        all.extend(child.desc_files.iter().copied());
    }
    all.sort_unstable();
    node.desc_files = all;
}

#[allow(clippy::too_many_arguments)]
fn draw_dirs(
    ui: &mut egui::Ui,
    node: &DirNode,
    prefix: &str,
    scan: &ScanOutput,
    checked: &mut HashSet<String>,
    tokens: &HashMap<String, usize>,
    open: &mut HashSet<String>,
) {
    for (name, child) in &node.dirs {
        let rel = format!("{prefix}{name}/");
        let expanded = open.contains(&rel);

        // Каскад: все отмечены / никто / частично.
        let selectable: Vec<bool> = child
            .desc_files
            .iter()
            .map(|&i| is_selectable(&scan.files[i].status))
            .collect();
        let total = selectable.iter().filter(|&&s| s).count();
        let marked = child
            .desc_files
            .iter()
            .zip(&selectable)
            .filter(|&(i, s)| *s && checked.contains(&scan.files[*i].rel_path))
            .count();

        ui.horizontal(|ui| {
            // Треугольник раскрытия — крайняя левая колонка (как в Tkinter);
            // у папки он есть всегда, клик переключает раскрытость.
            // Рисуется painter'ом: глифов ▸/▾ в Segoe UI нет — текстовый
            // вариант рендерился пустым квадратом (тофу).
            if triangle_cell(ui, expanded).clicked() {
                if expanded {
                    open.remove(&rel);
                } else {
                    open.insert(rel.clone());
                }
            }
            // Чекбокс папки с каскадом; отмечать нечего — пустая клетка.
            let glyph = if total == 0 {
                "  "
            } else if marked == total {
                "☑"
            } else if marked == 0 {
                "☐"
            } else {
                "▣"
            };
            if total > 0 && glyph_cell(ui, glyph).clicked() {
                let add = marked != total; // частично/пусто → отметить всё
                for (&i, &s) in child.desc_files.iter().zip(&selectable) {
                    if s && add {
                        checked.insert(scan.files[i].rel_path.clone());
                    } else if s {
                        checked.remove(&scan.files[i].rel_path);
                    }
                }
            } else if total == 0 {
                empty_cell(ui);
            }
            // Имя папки обычным текстом; клик тоже переключает раскрытость.
            if ui
                .add(egui::Label::new(name.as_str()).sense(egui::Sense::click()))
                .clicked()
            {
                if expanded {
                    open.remove(&rel);
                } else {
                    open.insert(rel.clone());
                }
            }
            if total > 0 {
                ui.weak(format!("({marked}/{total})"));
            }
        });

        if expanded {
            // Ступенька отступа — аналог вложенности Tkinter-дерева.
            ui.indent(egui::Id::new(&rel), |ui| {
                // Подпапки первыми, затем файлы этой папки.
                draw_dirs(ui, child, &rel, scan, checked, tokens, open);
                for &idx in &child.files {
                    file_row(ui, scan, idx, checked, tokens);
                }
            });
        }
    }
}

/// Строка файла: клетка треугольника (пустая), чекбокс (или прочерк), имя,
/// статус, токены.
fn file_row(
    ui: &mut egui::Ui,
    scan: &ScanOutput,
    idx: usize,
    checked: &mut HashSet<String>,
    tokens: &HashMap<String, usize>,
) {
    let entry = &scan.files[idx];
    let selectable = is_selectable(&entry.status);
    let is_checked = checked.contains(&entry.rel_path);

    ui.horizontal(|ui| {
        // Пустая клетка на месте треугольника — выравнивание с папками.
        empty_cell(ui);
        let glyph = if !selectable {
            "  "
        } else if is_checked {
            "☑"
        } else {
            "☐"
        };
        if selectable && glyph_cell(ui, glyph).clicked() {
            if is_checked {
                checked.remove(&entry.rel_path);
            } else {
                checked.insert(entry.rel_path.clone());
            }
        } else if !selectable {
            empty_cell(ui);
        }

        let name = entry.rel_path.rsplit('/').next().unwrap_or(&entry.rel_path);
        let (note, color) = status_note(&entry.status);
        let mut rich = egui::RichText::new(name);
        if let Some(c) = color {
            rich = rich.color(c);
        }
        ui.add(egui::Label::new(rich)).on_hover_text(&entry.rel_path);
        if !note.is_empty() {
            ui.weak(note);
        }
        if let Some(&t) = tokens.get(&entry.rel_path) {
            // Файл свыше порога — «> 50 000» без точного числа, как в старой
            // версии (treeview_scanner.py:141 → treeview_logic.py:47).
            let text = if t > MAX_TOKENS_FOR_DISPLAY {
                format!("> {} ткн", spaced(MAX_TOKENS_FOR_DISPLAY))
            } else {
                format!("{} ткн", spaced(t))
            };
            ui.weak(egui::RichText::new(text).monospace());
        }
    });
}

/// Отмечать можно обычные и «исключённые по умолчанию» файлы; в старой версии
/// НЕ отмечались только бинарные/большие/ошибки чтения.
fn is_selectable(status: &FileStatus) -> bool {
    matches!(status, FileStatus::Ok | FileStatus::ExcludedByDefault)
}

fn status_note(status: &FileStatus) -> (&'static str, Option<egui::Color32>) {
    match status {
        FileStatus::Ok => ("", None),
        FileStatus::Binary => ("[бинарный]", Some(egui::Color32::GRAY)),
        FileStatus::TooLarge => ("[> 1 МБ]", Some(egui::Color32::LIGHT_RED)),
        FileStatus::ExcludedByDefault => ("[исключён]", Some(egui::Color32::YELLOW)),
        FileStatus::ReadError(_) => ("[ошибка чтения]", Some(egui::Color32::LIGHT_RED)),
    }
}

/// Кнопка-глиф фиксированной клетки (чекбокс/треугольник) — колонки ровные
/// независимо от ширины глифов в шрифте.
fn glyph_cell(ui: &mut egui::Ui, glyph: &str) -> egui::Response {
    ui.add_sized(
        [CELL, CELL],
        egui::Button::new(egui::RichText::new(glyph).size(13.0)).frame(false),
    )
}

/// Клетка треугольника раскрытия папки: сам треугольник рисуется painter'ом
/// (текстовые глифы ▸/▾ в системном шрифте отсутствуют), клик/hover — как у кнопки.
fn triangle_cell(ui: &mut egui::Ui, expanded: bool) -> egui::Response {
    // allocate_exact_size возвращает (Rect, Response) — нужен только response.
    let response = ui
        .allocate_exact_size(egui::vec2(CELL, CELL), egui::Sense::click())
        .1;
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let color = ui.style().interact(&response).fg_stroke.color;
    let c = response.rect.center();
    // Свёрнута — указывает вправо, раскрыта — вниз.
    let points = if expanded {
        vec![
            c + egui::vec2(-4.0, -2.0),
            c + egui::vec2(4.0, -2.0),
            c + egui::vec2(0.0, 2.5),
        ]
    } else {
        vec![
            c + egui::vec2(-2.0, -4.0),
            c + egui::vec2(-2.0, 4.0),
            c + egui::vec2(2.5, 0.0),
        ]
    };
    ui.painter()
        .add(egui::Shape::convex_polygon(points, color, egui::Stroke::NONE));
    response
}

/// Пустая клетка (колонка треугольника у файлов, отсутствие чекбокса).
fn empty_cell(ui: &mut egui::Ui) {
    ui.allocate_exact_size(egui::vec2(CELL, CELL), egui::Sense::hover());
}
