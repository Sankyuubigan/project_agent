//! Текст структуры проекта (перенос generate_project_structure_text /
//! generate_full_project_structure): `<file_map>` + имя корня + дерево.
//!
//! Старая сортировка — `(не папка, name.lower())`: папки первыми, регистр
//! не важен. Пустой результат — `None` (старая версия возвращала строку
//! «Структура не сгенерирована…» и не включала её в промпт).

use std::collections::BTreeMap;
use std::path::Path;

use super::scanner::{self, ScanOutput};

/// Узел дерева: папка (с детьми) или файл.
struct Node {
    dirs: BTreeMap<String, Node>,
    files: Vec<String>,
}

impl Node {
    fn new() -> Self {
        Self {
            dirs: BTreeMap::new(),
            files: Vec::new(),
        }
    }
}

/// Структура по списку относительных путей (режим «только выделенные»).
pub fn generate_selected(root_name: &str, rel_paths: &[String]) -> Option<String> {
    let mut root = Node::new();
    for rel in rel_paths {
        insert_path(&mut root, rel);
    }
    let mut lines = vec![root_name.to_string()];
    render_node(&root, "", &mut lines);
    if lines.len() <= 1 {
        return None;
    }
    Some(format!("<file_map>\n{}\n</file_map>", lines.join("\n")))
}

/// Полная структура проекта (режим «все файлы», с каскадными .gitignore).
pub fn generate_all(root_dir: &Path) -> Option<String> {
    let scan: ScanOutput = scanner::scan(root_dir);
    let mut rels: Vec<String> = scan
        .files
        .iter()
        .map(|f| f.rel_path.clone())
        .collect();
    rels.sort();
    let root_name = root_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    generate_selected(&root_name, &rels)
}

fn insert_path(root: &mut Node, rel: &str) {
    let mut node = root;
    let segments: Vec<&str> = rel.split('/').collect();
    for (i, seg) in segments.iter().enumerate() {
        if i + 1 == segments.len() {
            node.files.push((*seg).to_string());
        } else {
            node = node.dirs.entry((*seg).to_string()).or_insert_with(Node::new);
        }
    }
}

fn render_node(node: &Node, prefix: &str, lines: &mut Vec<String>) {
    // Папки первыми, внутри группы — без учёта регистра (key=(not is_dir, lower)).
    let mut dir_names: Vec<&String> = node.dirs.keys().collect();
    dir_names.sort_by_key(|n| n.to_lowercase());
    let mut file_names: Vec<&String> = node.files.iter().collect();
    file_names.sort_by_key(|n| n.to_lowercase());

    let total = dir_names.len() + file_names.len();
    let mut idx = 0usize;

    for name in dir_names {
        idx += 1;
        let last = idx == total;
        let branch = if last { "└── " } else { "├── " };
        lines.push(format!("{prefix}{branch}{name}/"));
        let child_prefix = if last { format!("{prefix}    ") } else { format!("{prefix}│   ") };
        render_node(&node.dirs[name.as_str()], &child_prefix, lines);
    }

    for name in file_names {
        idx += 1;
        let last = idx == total;
        let branch = if last { "└── " } else { "├── " };
        lines.push(format!("{prefix}{branch}{name}"));
    }
}
