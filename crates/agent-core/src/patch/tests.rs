//! Юнит-тесты патчера «Умный Markdown» (чек-лист PRP Шага 2).

use std::fs;
use std::path::Path;

use tempfile::tempdir;

use super::anchors::apply_edit;
use super::applier::{apply, prepare, undo_last};
use super::models::{ChangeKind, PreparedKind};
use super::parser::parse;

// ---------------------------------------------------------------- парсер

#[test]
fn parse_whole_file_happy_path() {
    let out = parse("текст до\n<<<FILE: src/app.py>>>\nfn main() {}\n<<<END_FILE: src/app.py>>>\nтекст после");
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    assert_eq!(out.changes.len(), 1);
    assert_eq!(out.changes[0].path, "src/app.py");
    match &out.changes[0].kind {
        ChangeKind::Whole { content } => assert_eq!(content, "fn main() {}"),
        other => panic!("ожидался Whole, получен {other:?}"),
    }
}

#[test]
fn parse_backslash_path_normalized() {
    let out = parse("<<<FILE: src\\nested\\mod.rs>>>\nx\n<<<END_FILE: src\\nested\\mod.rs>>>");
    assert!(out.errors.is_empty());
    assert_eq!(out.changes[0].path, "src/nested/mod.rs");
}

#[test]
fn parse_bare_end_inside_block_is_content() {
    // Self-hosting: голый END_FILE (как в документации формата) не закрывает блок.
    let body = "описание формата:\n<<<END_FILE>>>\nконец примера";
    let out = parse(format!("<<<FILE: doc/format.md>>>\n{body}\n<<<END_FILE: doc/format.md>>>").as_str());
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    assert!(!out.warnings.is_empty(), "должен быть WARN о голом END_FILE");
    match &out.changes[0].kind {
        ChangeKind::Whole { content } => assert!(content.contains("<<<END_FILE>>>")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn parse_end_path_mismatch_is_error() {
    let out = parse("<<<FILE: src/a.py>>>\nx\n<<<END_FILE: src/b.py>>>");
    assert_eq!(out.changes.len(), 0, "блок с неверным END не применяется");
    assert_eq!(out.errors.len(), 1);
    assert_eq!(out.errors[0].path.as_deref(), Some("src/a.py"));
    assert!(out.errors[0].reason.contains("не совпадает"));
}

#[test]
fn parse_unclosed_block_is_error() {
    let out = parse("<<<FILE: src/a.py>>>\nfn f() {}\n");
    assert_eq!(out.errors.len(), 1);
    assert!(out.errors[0].reason.contains("незакрытый"));
}

#[test]
fn parse_delete_file_tag() {
    let out = parse("пояснение\n<<<DELETE_FILE: src/old.py>>>\n");
    assert!(out.errors.is_empty());
    assert_eq!(out.changes.len(), 1);
    assert_eq!(out.changes[0].kind, ChangeKind::Delete);
}

#[test]
fn parse_empty_block_creates_empty_file_with_warning() {
    // Решение владельца: пустой блок = пустой файл, НЕ удаление.
    let out = parse("<<<FILE: .gitkeep>>>\n<<<END_FILE: .gitkeep>>>");
    assert!(out.errors.is_empty());
    assert!(out.warnings.iter().any(|w| w.contains("пустой файл")));
    match &out.changes[0].kind {
        ChangeKind::Whole { content } => assert!(content.is_empty()),
        other => panic!("{other:?}"),
    }
}

#[test]
fn parse_path_escape_rejected() {
    for bad in ["../secret.txt", "a/../../b.txt", "/abs/path.txt", "C:\\win\\x.txt", "   "] {
        let out = parse(format!("<<<FILE: {bad}>>>\nx\n<<<END_FILE: {bad}>>>").as_str());
        assert_eq!(out.errors.len(), 1, "путь `{bad}` должен быть отклонён");
    }
}

#[test]
fn parse_search_replace_block() {
    let body = "<<<<<<< SEARCH\nold line\n=======\nnew line\n>>>>>>> REPLACE";
    let out = parse(format!("<<<FILE: src/a.py>>>\n{body}\n<<<END_FILE: src/a.py>>>").as_str());
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    match &out.changes[0].kind {
        ChangeKind::Edits { edits } => {
            assert_eq!(edits.len(), 1);
            assert_eq!(edits[0].search, "old line");
            assert_eq!(edits[0].replace, "new line");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn parse_multiple_files_and_ignored_text() {
    let out = parse(
        "пояснение 1\n<<<FILE: a.txt>>>\nA\n<<<END_FILE: a.txt>>>\nпояснение 2\n\
         <<<FILE: b.txt>>>\nB\n<<<END_FILE: b.txt>>>",
    );
    assert!(out.errors.is_empty());
    assert_eq!(out.changes.len(), 2);
    assert_eq!(out.changes[0].path, "a.txt");
    assert_eq!(out.changes[1].path, "b.txt");
}

// ---------------------------------------------------------------- якоря

#[test]
fn anchor_level1_exact() {
    let out = apply_edit("a\nold line\nb", "old line", "new line").unwrap();
    assert_eq!(out, "a\nnew line\nb");
}

#[test]
fn anchor_level2_trailing_whitespace_and_crlf() {
    let content = "a\r\nold line   \r\nb\r\n";
    let out = apply_edit(content, "old line\n", "new line\n").unwrap();
    assert!(out.starts_with("a\nnew line\nb\n"), "{out:?}");
}

#[test]
fn anchor_level3_relative_indentation() {
    let content = "fn f() {\n    if x {\n        old_call();\n    }\n}";
    let search = "if x {\n    old_call();\n}";
    let replace = "if x {\n    new_call();\n    log();\n}";
    let out = apply_edit(content, search, replace).unwrap();
    assert!(
        out.contains("        new_call();\n        log();"),
        "сдвиг на 4 пробела не применён: {out:?}"
    );
    assert!(!out.contains("old_call"));
}

#[test]
fn anchor_not_found() {
    let err = apply_edit("abc", "nope", "x").unwrap_err();
    assert_eq!(err, super::anchors::AnchorError::NotFound);
}

#[test]
fn anchor_ambiguous() {
    let err = apply_edit("x\nold\ny\nold\nz", "old", "new").unwrap_err();
    assert!(matches!(err, super::anchors::AnchorError::Ambiguous { count: 2 }));
}

// ---------------------------------------------------------------- аплаер

fn make_file(root: &Path, rel: &str, content: &str) {
    let full = root.join(rel);
    fs::create_dir_all(full.parent().unwrap()).unwrap();
    fs::write(full, content).unwrap();
}

fn change_whole(path: &str, content: &str) -> super::models::FileChange {
    super::models::FileChange {
        path: path.to_string(),
        kind: ChangeKind::Whole { content: content.to_string() },
    }
}

#[test]
fn apply_creates_new_file_in_nested_dir() {
    let root = tempdir().unwrap();
    let backup = tempdir().unwrap();
    let change = change_whole("deep/nested/new.rs", "fn new() {}\n");
    let prepared = prepare(root.path(), &change).unwrap();
    let report = apply(root.path(), backup.path(), &[prepared]);
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert_eq!(report.applied, ["deep/nested/new.rs"]);
    assert_eq!(
        fs::read_to_string(root.path().join("deep/nested/new.rs")).unwrap(),
        "fn new() {}\n"
    );
}

#[test]
fn apply_edits_to_existing_file() {
    let root = tempdir().unwrap();
    let backup = tempdir().unwrap();
    make_file(root.path(), "app.txt", "line1\nold\nline3\n");
    let change = super::models::FileChange {
        path: "app.txt".to_string(),
        kind: ChangeKind::Edits {
            edits: vec![super::models::Edit {
                search: "old".to_string(),
                replace: "new".to_string(),
            }],
        },
    };
    let prepared = prepare(root.path(), &change).unwrap();
    let report = apply(root.path(), backup.path(), &[prepared]);
    assert!(report.failed.is_empty());
    assert_eq!(
        fs::read_to_string(root.path().join("app.txt")).unwrap(),
        "line1\nnew\nline3\n"
    );
}

#[test]
fn prepare_edits_missing_file_is_error() {
    let root = tempdir().unwrap();
    let change = super::models::FileChange {
        path: "ghost.txt".to_string(),
        kind: ChangeKind::Edits { edits: vec![] },
    };
    let err = prepare(root.path(), &change).unwrap_err();
    assert!(err.reason.contains("не существует"), "{}", err.reason);
}

#[test]
fn apply_delete_existing_and_missing() {
    let root = tempdir().unwrap();
    let backup = tempdir().unwrap();
    make_file(root.path(), "gone.txt", "x");
    let del = super::models::FileChange { path: "gone.txt".to_string(), kind: ChangeKind::Delete };
    let prepared = prepare(root.path(), &del).unwrap();
    let report = apply(root.path(), backup.path(), &[prepared]);
    assert!(report.failed.is_empty());
    assert!(!root.path().join("gone.txt").exists());

    let del_missing =
        super::models::FileChange { path: "ghost.txt".to_string(), kind: ChangeKind::Delete };
    assert!(prepare(root.path(), &del_missing).is_err(), "удаление несуществующего — ошибка");
}

#[test]
fn backup_and_undo_restore_state() {
    let root = tempdir().unwrap();
    let backup = tempdir().unwrap();
    make_file(root.path(), "existing.txt", "СТАРОЕ\n");
    let changes = vec![
        change_whole("existing.txt", "НОВОЕ\n"),
        change_whole("created.txt", "создан\n"),
    ];
    let prepared: Vec<_> = changes.iter().map(|c| prepare(root.path(), c).unwrap()).collect();
    let report = apply(root.path(), backup.path(), &prepared);
    assert!(report.failed.is_empty());
    assert_eq!(report.backed_up, ["existing.txt"]);

    let restored = undo_last(root.path(), backup.path()).unwrap();
    assert_eq!(restored, 2);
    assert_eq!(
        fs::read_to_string(root.path().join("existing.txt")).unwrap(),
        "СТАРОЕ\n"
    );
    assert!(!root.path().join("created.txt").exists(), "созданный файл удаляется при Undo");
}

#[test]
fn second_undo_is_error_no_manifest() {
    let root = tempdir().unwrap();
    let backup = tempdir().unwrap();
    make_file(root.path(), "f.txt", "1\n");
    let change = change_whole("f.txt", "2\n");
    let prepared = prepare(root.path(), &change).unwrap();
    apply(root.path(), backup.path(), &[prepared]);
    assert!(undo_last(root.path(), backup.path()).is_ok());
    assert!(undo_last(root.path(), backup.path()).is_err(), "манифест погашен");
}

#[test]
fn apply_reports_failed_write_without_touching_others() {
    // Файл-каталог вместо файла: запись падает, остальное не должно примениться.
    let root = tempdir().unwrap();
    let backup = tempdir().unwrap();
    fs::create_dir_all(root.path().join("blocker.txt")).unwrap();
    let changes = vec![change_whole("ok.txt", "ok\n"), change_whole("blocker.txt", "x")];
    let prepared: Vec<_> = changes.iter().map(|c| prepare(root.path(), c).unwrap()).collect();
    let report = apply(root.path(), backup.path(), &prepared);
    assert_eq!(report.failed.len(), 1);
    assert!(report.applied.is_empty(), "откат: ничего не должно остаться");
    assert!(!root.path().join("ok.txt").exists(), "ok.txt должен быть откачен");
}

#[test]
fn prepared_write_content_is_lf_normalized() {
    // Парсер собирает содержимое через .lines()+join — CRLF внутри блока
    // нормализуется ещё на разборе.
    let out = parse("<<<FILE: a.txt>>>\r\nline1\r\nline2\r\n<<<END_FILE: a.txt>>>");
    assert!(out.errors.is_empty());
    match &out.changes[0].kind {
        ChangeKind::Whole { content } => assert_eq!(content, "line1\nline2"),
        other => panic!("{other:?}"),
    }
    // Аплаер: PreparedKind::Write содержит уже нормализованный текст.
    let root = tempdir().unwrap();
    let prepared = prepare(root.path(), &out.changes[0]).unwrap();
    match prepared.kind {
        PreparedKind::Write { content } => assert!(!content.contains('\r')),
        other => panic!("{other:?}"),
    }
}
