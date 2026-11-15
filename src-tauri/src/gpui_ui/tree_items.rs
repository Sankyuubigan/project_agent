//! Данные дерева «Проект → чат» для GPUI-Tree (PRP 14.11.26 шаг 3): иерархия
//! `TreeItem` из `ScanOutput` + плоский снапшот строк (чекбоксы, статусы,
//! токены, каскадные счётчики папок). Повторяет поведение `ui/tree_widget.rs`
//! (egui): папки первыми, свёрнуто по умолчанию, каскад, файлы после папок.
//!
//! Снапшот нужен потому, что render-closure gpui `tree()` требует `'static`:
//! он захватывает `Arc<Vec<RowData>>`, собранный в render, вместо ссылки на
//! стейт. ElementId строк и чекбоксов — доменные, от rel-пути.

use std::collections::{HashMap, HashSet};

use agent_core::tree::scanner::{FileStatus, ScanOutput};
use agent_core::tree::tokens::{spaced, MAX_TOKENS_FOR_DISPLAY};
use gpui_kit::component::tree::TreeItem;
use gpui_kit::SharedString;

/// Данные одной строки дерева (папка или файл) для render-closure.
#[derive(Clone, Debug)]
pub struct RowData {
    /// rel-путь; у папок — с `/` на конце (домен ElementId и каскада).
    pub rel: String,
    pub is_dir: bool,
    /// Короткое имя (последний сегмент) для показа.
    pub name: String,
    /// Файл: Ok | ExcludedByDefault (отмечать можно); папка: total > 0.
    pub selectable: bool,
    /// Файл: стоит ли галочка; папка: все под-файлы отмечены.
    pub checked: bool,
    /// Папка: (отмечено, всего) selectable-файлов под ней.
    pub marked: usize,
    pub total: usize,
    /// Заметка статуса («[бинарный]» и т.п.); пустая — обычный файл.
    pub note: &'static str,
    /// Заметка — ошибка чтения/размера (красить цветом danger).
    pub note_is_error: bool,
    /// Токены файла («1 234 ткн» / «> 50 000 ткн»), если посчитаны.
    pub tokens: Option<String>,
    /// Папка: есть ли подпапки/файлы (определяет chevron и клик-раскрытие).
    pub has_children: bool,
    /// Папка: раскрыта ли (стейт UplinkState::expanded, не библиотечный).
    pub expanded: bool,
}

#[derive(Default)]
struct DirNode {
    /// Подпапки: (имя, узел); сортируются без учёта регистра.
    dirs: Vec<(String, DirNode)>,
    /// Индексы файлов этой папки (в scan.files).
    files: Vec<usize>,
    /// Индексы ВСЕХ файлов под папкой (для каскада и счётчиков).
    desc_files: Vec<usize>,
}

/// Отмечать можно обычные и «исключённые по умолчанию» файлы; бинарные,
/// большие и с ошибкой чтения — нельзя (паритет с egui).
fn is_selectable(status: &FileStatus) -> bool {
    matches!(status, FileStatus::Ok | FileStatus::ExcludedByDefault)
}

/// (текст заметки, это ошибка?) — паритет с `ui/tree_widget.rs::status_note`.
fn status_note(status: &FileStatus) -> (&'static str, bool) {
    match status {
        FileStatus::Ok => ("", false),
        FileStatus::Binary => ("[бинарный]", false),
        FileStatus::TooLarge => ("[> 1 МБ]", true),
        FileStatus::ExcludedByDefault => ("[исключён]", false),
        FileStatus::ReadError(_) => ("[ошибка чтения]", true),
    }
}

fn build_tree(scan: &ScanOutput) -> DirNode {
    let mut root = DirNode::default();
    for (idx, f) in scan.files.iter().enumerate() {
        insert_file(&mut root, &f.rel_path, idx);
    }
    collect_desc(&mut root);
    root
}

fn insert_file(root: &mut DirNode, rel: &str, idx: usize) {
    let Some((dir, _)) = rel.rsplit_once('/') else {
        root.files.push(idx);
        return;
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

/// desc_files + сортировка подпапок без учёта регистра (паритет со старой версией).
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

/// Плоский список ВИДИМЫХ строк в порядке pre-order — строго 1:1 с flatten
/// TreeState (gpui-base tree.rs:324 push_root / :340 add_entry): поддерево
/// попадает в список ТОЛЬКО если папка раскрыта. Без этого uniform_list
/// рисует entries.len() строк, но берёт их из полного preorder — индексы
/// разъезжаются и клики по папкам не дают видимого эффекта (баг 15.11.26).
/// Скана нет — пустой список (в tree_state остаётся заглушка).
pub fn build_rows(
    scan: Option<&ScanOutput>,
    checked: &HashSet<String>,
    tokens: &HashMap<String, usize>,
    expanded: &HashSet<String>,
) -> Vec<RowData> {
    let Some(scan) = scan else { return Vec::new() };
    let root = build_tree(scan);
    let mut rows = Vec::new();
    for (name, child) in &root.dirs {
        push_dir("", name, child, scan, checked, tokens, expanded, &mut rows);
    }
    for &idx in &root.files {
        push_file(idx, scan, checked, tokens, &mut rows);
    }
    rows
}

/// Иерархия TreeItem для TreeState; раскрытость папок — из `expanded` (наш
/// стейт: библиотечный toggle_expand приватен). Вызывается на новый скан и
/// на каждый клик-раскрытие.
pub fn build_tree_items(scan: &ScanOutput, expanded: &HashSet<String>) -> Vec<TreeItem> {
    let root = build_tree(scan);
    let mut items = Vec::new();
    for (name, child) in &root.dirs {
        items.push(dir_item("", name, child, scan, expanded));
    }
    for &idx in &root.files {
        items.push(file_item(&scan.files[idx].rel_path));
    }
    items
}

fn push_dir(
    prefix: &str,
    name: &str,
    node: &DirNode,
    scan: &ScanOutput,
    checked: &HashSet<String>,
    tokens: &HashMap<String, usize>,
    expanded: &HashSet<String>,
    rows: &mut Vec<RowData>,
) {
    let rel = format!("{prefix}{name}/");
    let total = node
        .desc_files
        .iter()
        .filter(|&&i| is_selectable(&scan.files[i].status))
        .count();
    let marked = node
        .desc_files
        .iter()
        .filter(|&&i| {
            is_selectable(&scan.files[i].status) && checked.contains(&scan.files[i].rel_path)
        })
        .count();
    // Строка папки: pre-order — папка, затем её содержимое.
    rows.push(RowData {
        rel: rel.clone(),
        is_dir: true,
        name: name.to_string(),
        selectable: total > 0,
        checked: marked == total,
        marked,
        total,
        note: "",
        note_is_error: false,
        tokens: None,
        has_children: !node.dirs.is_empty() || !node.files.is_empty(),
        expanded: expanded.contains(&rel),
    });
    // Содержимое — только у раскрытой папки (паритет с flatten TreeState).
    if expanded.contains(&rel) {
        for (cname, c) in &node.dirs {
            push_dir(&rel, cname, c, scan, checked, tokens, expanded, rows);
        }
        for &idx in &node.files {
            push_file(idx, scan, checked, tokens, rows);
        }
    }
}

fn push_file(
    idx: usize,
    scan: &ScanOutput,
    checked: &HashSet<String>,
    tokens: &HashMap<String, usize>,
    rows: &mut Vec<RowData>,
) {
    let entry = &scan.files[idx];
    let (note, note_is_error) = status_note(&entry.status);
    // Файл свыше порога — «> 50 000» без точного числа (паритет со старой
    // версией: treeview_scanner.py:141 → treeview_logic.py:47).
    let tokens_text = tokens.get(&entry.rel_path).map(|&t| {
        if t > MAX_TOKENS_FOR_DISPLAY {
            format!("> {} ткн", spaced(MAX_TOKENS_FOR_DISPLAY))
        } else {
            format!("{} ткн", spaced(t))
        }
    });
    rows.push(RowData {
        rel: entry.rel_path.clone(),
        is_dir: false,
        name: entry.rel_path.rsplit('/').next().unwrap_or(&entry.rel_path).to_string(),
        selectable: is_selectable(&entry.status),
        checked: checked.contains(&entry.rel_path),
        marked: 0,
        total: 0,
        note,
        note_is_error,
        tokens: tokens_text,
        has_children: false,
        expanded: false,
    });
}

fn dir_item(
    prefix: &str,
    name: &str,
    node: &DirNode,
    scan: &ScanOutput,
    expanded: &HashSet<String>,
) -> TreeItem {
    let rel = format!("{prefix}{name}/");
    let mut children: Vec<TreeItem> = node
        .dirs
        .iter()
        .map(|(cname, c)| dir_item(&rel, cname, c, scan, expanded))
        .collect();
    for &idx in &node.files {
        children.push(file_item(&scan.files[idx].rel_path));
    }
    TreeItem::new(SharedString::from(format!("dir_{rel}")), SharedString::from(name))
        .expanded(expanded.contains(&rel))
        .children(children)
}

/// Лист-элемент файла: id и лейбл — rel-путь (домен, уникален; кастомный
/// рендер строки использует `RowData` по индексу flatten-а, не лейбл).
fn file_item(rel: &str) -> TreeItem {
    TreeItem::new(SharedString::from(format!("file_{rel}")), SharedString::from(rel))
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

    #[test]
    fn rows_preorder_and_cascade() {
        let s = scan();
        // Имитация on_scanned: Ok и ExcludedByDefault отмечены, бинарный нет.
        let checked: HashSet<String> = ["a.rs", "dir/b.rs", "big.py"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        // Свёрнуто: поддерево dir/ НЕ попадает в снапшот (1:1 с flatten TreeState).
        let collapsed = build_rows(Some(&s), &checked, &HashMap::new(), &HashSet::new());
        let rels: Vec<&str> = collapsed.iter().map(|r| r.rel.as_str()).collect();
        assert_eq!(rels, ["dir/", "a.rs", "big.py"]);

        // Раскрыто: полное поддерево в pre-order (папки первыми, файлы после).
        let expanded: HashSet<String> = ["dir/".to_string()].into_iter().collect();
        let rows = build_rows(Some(&s), &checked, &HashMap::new(), &expanded);
        let rels: Vec<&str> = rows.iter().map(|r| r.rel.as_str()).collect();
        assert_eq!(rels, ["dir/", "dir/b.rs", "dir/c.bin", "a.rs", "big.py"]);
        let dir = &rows[0];
        assert!(dir.is_dir && dir.selectable);
        assert!(dir.has_children && dir.expanded);
        // Паритет с egui tree_widget.rs:121: total/marked — только selectable;
        // c.bin бинарный — вне счёта, поэтому 1/1 и папка «полностью отмечена».
        assert_eq!((dir.marked, dir.total), (1, 1));
        assert!(dir.checked);
        assert!(!rows[2].selectable); // бинарный
        assert_eq!(rows[2].note, "[бинарный]");
        assert!(rows[4].selectable && rows[4].checked); // ExcludedByDefault
    }

    #[test]
    fn items_mirror_rows_count() {
        let s = scan();
        let items = build_tree_items(&s, &HashSet::new());
        let rows = build_rows(Some(&s), &HashSet::new(), &HashMap::new(), &HashSet::new());
        // Свёрнуто: entries = корни (1 папка + 2 файла) = строки снапшота.
        assert_eq!(items.len(), 3);
        assert_eq!(rows.len(), 3);
        // Раскрыто: flatten TreeState вырастает до 5 — снапшот синхронно.
        let expanded: HashSet<String> = ["dir/".to_string()].into_iter().collect();
        let items_open = build_tree_items(&s, &expanded);
        let rows_open = build_rows(Some(&s), &HashSet::new(), &HashMap::new(), &expanded);
        assert_eq!(items_open.len(), 3);
        assert_eq!(rows_open.len(), 5);
    }

    #[test]
    fn items_expand_from_state() {
        let s = scan();
        let expanded: HashSet<String> = ["dir/".to_string()].into_iter().collect();
        let items = build_tree_items(&s, &expanded);
        let rows = build_rows(Some(&s), &HashSet::new(), &HashMap::new(), &expanded);
        // Папка раскрыта и в TreeItem, и в снапшоте строк; файл — не папка.
        assert!(items[0].is_expanded());
        assert!(rows[0].expanded && rows[0].has_children);
        assert!(!items[1].is_expanded());
        // Дети раскрытой папки в снапшоте: индексы строк = индексам entries.
        assert_eq!(rows[1].rel, "dir/b.rs");
    }

    #[test]
    fn tokens_display_over_threshold() {
        let s = scan();
        let mut tokens = HashMap::new();
        tokens.insert("a.rs".to_string(), 60_000usize);
        tokens.insert("dir/b.rs".to_string(), 1234usize);
        let expanded: HashSet<String> = ["dir/".to_string()].into_iter().collect();
        let rows = build_rows(Some(&s), &HashSet::new(), &tokens, &expanded);
        assert_eq!(rows[3].tokens.as_deref(), Some("> 50 000 ткн"));
        assert_eq!(rows[1].tokens.as_deref(), Some("1 234 ткн"));
    }
}
