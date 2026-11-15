//! Юнит-тесты дерева/промпта (Шаг 3 PRP).

use std::fs;
use std::path::Path;

use tempfile::tempdir;

use super::prompt::{
    build_prompt, instruction_path, load_instruction, CHANGES_FILE, PLAN_FILE, InstructionSlot,
};
use super::scanner::{fnmatch, file_status, scan, FileStatus};
use super::structure::generate_selected;

// ---------------------------------------------------------------- сканер

#[test]
fn fnmatch_star_patterns() {
    assert!(fnmatch("*.pyc", "foo.pyc"));
    assert!(fnmatch("*.pyc", "BAR.PYC"));
    assert!(fnmatch("LICENSE.*", "license.mit"));
    assert!(!fnmatch("*.pyc", "foo.py"));
    assert!(fnmatch("*.ipynb_checkpoints*", "a.ipynb_checkpoints.b"));
}

#[test]
fn file_status_by_extension_and_size() {
    assert_eq!(file_status("img.png", 100), FileStatus::Binary);
    assert_eq!(file_status("code.rs", 100), FileStatus::Ok);
    assert_eq!(file_status("LICENSE", 100), FileStatus::ExcludedByDefault);
    // .log в старом BINARY_EXTENSIONS нет — это TooLarge; в скане *.log
    // отсекается глоб-игнором ещё до статуса.
    assert_eq!(file_status("huge.log", 2 * 1024 * 1024), FileStatus::TooLarge);
    assert_eq!(file_status("huge.rs", 2 * 1024 * 1024), FileStatus::TooLarge);
}

#[test]
fn scan_skips_global_ignored_dirs_and_files() {
    let root = tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::create_dir_all(root.path().join("node_modules/pkg")).unwrap();
    fs::create_dir_all(root.path().join("target/debug")).unwrap();
    fs::create_dir_all(root.path().join("tasks")).unwrap();
    fs::write(root.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(root.path().join("src/bug.pyc"), "x").unwrap();
    fs::write(root.path().join("Thumbs.db"), "x").unwrap();
    fs::write(root.path().join("node_modules/pkg/i.js"), "x").unwrap();
    fs::write(root.path().join("target/debug/a.exe"), "x").unwrap();
    fs::write(root.path().join("tasks/plan.md"), "x").unwrap();

    let out = scan(root.path());
    let rels: Vec<&str> = out.files.iter().map(|f| f.rel_path.as_str()).collect();
    assert_eq!(rels, ["src/main.rs"], "{rels:?}");
    // Папки node_modules/target/tasks тоже не попадают в дерево.
    assert!(
        !out.dirs.iter().any(|d| d.rel_path.contains("node_modules") || d.rel_path == "tasks"),
        "{:?}",
        out.dirs
    );
}

#[test]
fn scan_respects_project_gitignore() {
    let root = tempdir().unwrap();
    fs::create_dir_all(root.path().join("gen")).unwrap();
    fs::write(root.path().join(".gitignore"), "gen/\n").unwrap();
    fs::write(root.path().join("keep.rs"), "x").unwrap();
    fs::write(root.path().join("gen/made.rs"), "x").unwrap();

    let out = scan(root.path());
    let rels: Vec<&str> = out.files.iter().map(|f| f.rel_path.as_str()).collect();
    // gen/ проигнорирован по project .gitignore; сам .gitignore старая версия
    // в дереве показывала (глоб-игнор его не отсекает).
    assert_eq!(rels, [".gitignore", "keep.rs"], "{rels:?}");
}

// ---------------------------------------------------------------- структура

#[test]
fn structure_tree_rendering() {
    let tree = generate_selected(
        "MyProject",
        &[
            "src/main.rs".to_string(),
            "src/ui/mod.rs".to_string(),
            "Cargo.toml".to_string(),
        ],
    );
    // Как в старой версии: обёртка <file_map>, имя корня первой строкой,
    // папки первыми (внутри src: ui/ раньше main.rs).
    let expect = "\
<file_map>
MyProject
├── src/
│   ├── ui/
│   │   └── mod.rs
│   └── main.rs
└── Cargo.toml
</file_map>";
    assert_eq!(tree.as_deref(), Some(expect), "\n{tree:?}");
}

#[test]
fn structure_dirs_first_case_insensitive() {
    let tree = generate_selected("P", &["Beta/f.txt".to_string(), "alpha/g.txt".to_string()]);
    let expect = "\
<file_map>
P
├── alpha/
│   └── g.txt
└── Beta/
    └── f.txt
</file_map>";
    assert_eq!(tree.as_deref(), Some(expect), "\n{tree:?}");
}

#[test]
fn structure_empty_selection_is_none() {
    // Старая версия: «Структура не сгенерирована» → в промпт не попадает.
    assert_eq!(generate_selected("P", &[]), None);
}

// ---------------------------------------------------------------- промпт

fn entry(rel: &str, status: FileStatus) -> super::scanner::FileEntry {
    super::scanner::FileEntry {
        rel_path: rel.to_string(),
        size: 10,
        status,
    }
}

#[test]
fn prompt_order_instruction_structure_files() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("a.txt"), "AAA\n").unwrap();
    fs::write(root.path().join("b.txt"), "BBB\n").unwrap();

    let selected = vec![entry("a.txt", FileStatus::Ok), entry("b.txt", FileStatus::Ok)];
    let result = build_prompt(
        root.path(),
        &selected,
        Some("СТРУКТУРА".to_string()),
        &["ИНСТРУКЦИЯ".to_string()],
    );

    let expected = "ИНСТРУКЦИЯ\n\n---\n\nСТРУКТУРА\n\n---\nСодержимое выбранных файлов:\n---\n\n\
                    <<<FILE: a.txt>>>\nAAA\n\n<<<END_FILE: a.txt>>>\n\n\
                    <<<FILE: b.txt>>>\nBBB\n\n<<<END_FILE: b.txt>>>";
    assert_eq!(result.text, expected);
    assert_eq!(result.file_count, 2);
    assert!(result.warnings.is_empty());
}

#[test]
fn prompt_without_optional_parts() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("a.txt"), "AAA\n").unwrap();
    let selected = vec![entry("a.txt", FileStatus::Ok)];
    let result = build_prompt(root.path(), &selected, None, &[]);
    assert_eq!(result.text, "<<<FILE: a.txt>>>\nAAA\n\n<<<END_FILE: a.txt>>>");
    assert_eq!(result.file_count, 1);
}

#[test]
fn prompt_warns_on_unreadable_file() {
    let root = tempdir().unwrap();
    // Файла нет: read_to_string упадёт → warning, не паника.
    let selected = vec![entry("ghost.txt", FileStatus::Ok)];
    let result = build_prompt(root.path(), &selected, None, &[]);
    assert_eq!(result.file_count, 0);
    assert!(result.warnings.iter().any(|w| w.contains("ghost.txt")));
}

#[test]
fn instruction_missing_file_is_error() {
    let root = tempdir().unwrap();
    let err = load_instruction(&root.path().join("doc/nope.md")).unwrap_err();
    assert!(err.contains("не найден"), "{err}");
}

#[test]
fn instruction_path_from_exe_dir() {
    assert_eq!(
        instruction_path(Path::new("C:/app"), CHANGES_FILE),
        Path::new("C:/app/doc/markdown_changes.md")
    );
    assert_eq!(
        instruction_path(Path::new("C:/app"), PLAN_FILE),
        Path::new("C:/app/doc/markdown_plan.md")
    );
}

#[test]
fn prompt_two_instructions_separated() {
    let root = tempdir().unwrap();
    let result = build_prompt(
        root.path(),
        &[],
        None,
        &["ПЕРВАЯ".to_string(), "ВТОРАЯ".to_string()],
    );
    // Разделитель между инструкциями тот же, что и после одиночной раньше:
    // пара «инструкция + разделитель» пушится целиком, trim съедает только
    // пробелы — хвостовые «---» остаются (поведение старой версии).
    assert_eq!(result.text, "ПЕРВАЯ\n\n---\n\nВТОРАЯ\n\n---");
}

#[test]
fn instruction_slot_missing_file_is_visible_error() {
    let root = tempdir().unwrap();
    let mut slot = InstructionSlot::new(true);
    slot.refresh(root.path(), "nope.md");
    assert!(slot.text().is_none());
    let (hint, is_error) = slot.hint("инструкция").expect("ошибка видима в хинте");
    assert!(is_error);
    assert!(hint.contains("не найден"), "{hint}");
}

#[test]
fn instruction_slot_loads_doc_file() {
    let root = tempdir().unwrap();
    fs::create_dir_all(root.path().join("doc")).unwrap();
    fs::write(root.path().join("doc/x.md"), "ТЕКСТ").unwrap();
    let mut slot = InstructionSlot::new(false);
    slot.refresh(root.path(), "x.md");
    assert_eq!(slot.text(), Some("ТЕКСТ"));
    let (hint, is_error) = slot.hint("инструкция").expect("хинт размера");
    assert!(!is_error);
    assert!(hint.contains("символов"), "{hint}");
}

// ---------------------------------------------------------------- токены

#[test]
fn token_cache_counts_text_and_file() {
    use super::tokens::token_cache;

    let root = tempdir().unwrap();
    let path = root.path().join("t.txt");
    fs::write(&path, "hello world, this is a token counting test\n").unwrap();

    let cache = token_cache();
    let first = cache.count_file(&path).unwrap().expect("файл маленький — токены есть");
    assert!(first > 0);
    // Повторный вызов идёт по кэшу (mtime/size не менялись).
    let second = cache.count_file(&path).unwrap().unwrap();
    assert_eq!(first, second);

    let text_tokens = cache.count_text("hello world, this is a token counting test\n").unwrap();
    assert_eq!(text_tokens, first);
}
