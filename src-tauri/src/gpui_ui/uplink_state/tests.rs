//! Юнит-тесты UplinkState GPUI-фронта. Вынесены из `uplink_state.rs`
//! (лимит файла 500 строк, правило владельца).

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
