//! Двухфазный аплаер (вердикт спеца №4): dry-run в памяти → атомарная запись
//! «всё или ничего». Бэкап затронутых файлов + манифест для Undo.
//!
//! `prepare` — фаза 1 (в памяти, диск не трогается), `apply` — фаза 2 (диск),
//! `undo_last` — откат последнего применения по манифесту бэкапа.

use std::fs;
use std::path::Path;

use super::anchors::{apply_edit, AnchorError};
use super::models::{
    ApplyReport, ChangeKind, FileChange, FilePatchError, PreparedChange, PreparedKind,
};

/// Имя файла-манифеста в папке бэкапа: строки `оп\tотносительный/путь`.
const MANIFEST_NAME: &str = "manifest.txt";
/// Готовый манифест переименовывается, чтобы Undo не сработал дважды.
const MANIFEST_DONE: &str = "manifest.done.txt";

// ---------------------------------------------------------------- фаза 1

/// Dry-run одного изменения: считает результат в памяти, диск не трогает.
pub fn prepare(root: &Path, change: &FileChange) -> Result<PreparedChange, FilePatchError> {
    let full = root.join(&change.path);
    match &change.kind {
        ChangeKind::Whole { content } => Ok(whole_prepared(&change.path, content)),
        ChangeKind::Edits { edits } => prepare_edits(&change.path, &full, edits),
        ChangeKind::Delete => {
            if full.exists() {
                Ok(PreparedChange {
                    path: change.path.clone(),
                    kind: PreparedKind::Delete,
                    warnings: Vec::new(),
                })
            } else {
                Err(FilePatchError {
                    path: change.path.clone(),
                    reason: "нечего удалять: файл не существует".to_string(),
                })
            }
        }
    }
}

fn whole_prepared(path: &str, content: &str) -> PreparedChange {
    let mut warnings = Vec::new();
    if content.is_empty() {
        warnings.push("файл будет создан/оставлен пустым".to_string());
    }
    PreparedChange {
        path: path.to_string(),
        kind: PreparedKind::Write {
            content: content.to_string(),
        },
        warnings,
    }
}

/// Точечные правки: файл должен существовать и быть UTF-8; каждая правка —
/// через каскад якорей. Одна ошибка правки отклоняет весь файл.
fn prepare_edits(
    path: &str,
    full: &Path,
    edits: &[super::models::Edit],
) -> Result<PreparedChange, FilePatchError> {
    if !full.exists() {
        return Err(FilePatchError {
            path: path.to_string(),
            reason: "файл не существует — SEARCH/REPLACE только для существующих файлов".to_string(),
        });
    }
    let current = fs::read_to_string(full).map_err(|e| FilePatchError {
        path: path.to_string(),
        reason: format!("не удалось прочитать файл (нужен UTF-8): {e}"),
    })?;

    let mut buf = current;
    for (idx, edit) in edits.iter().enumerate() {
        buf = apply_edit(&buf, &edit.search, &edit.replace).map_err(|e: AnchorError| {
            FilePatchError {
                path: path.to_string(),
                reason: format!("правка {}: {}", idx + 1, e.reason()),
            }
        })?;
    }

    Ok(PreparedChange {
        path: path.to_string(),
        kind: PreparedKind::Write { content: buf },
        warnings: Vec::new(),
    })
}

// ---------------------------------------------------------------- фаза 2

/// Фаза 2: бэкап затронутых файлов → запись. Ошибка на диске откатывает всё
/// уже записанное из бэкапа (ничего не остаётся наполовину).
pub fn apply(root: &Path, backup_root: &Path, prepared: &[PreparedChange]) -> ApplyReport {
    let mut report = ApplyReport::default();

    // Бэкап: существующие файлы перед изменением/удалением.
    for p in prepared {
        let full = root.join(&p.path);
        if full.exists() {
            if let Err(e) = backup_file(root, backup_root, &p.path) {
                return fail_all(format!("бэкап {}: {e}", p.path));
            }
            report.backed_up.push(p.path.clone());
        }
    }

    // Запись/удаление. Любая ошибка → полный откат уже применённого.
    let mut applied: Vec<String> = Vec::new();
    for p in prepared {
        let result = match &p.kind {
            PreparedKind::Write { content } => atomic_write(&root.join(&p.path), content),
            PreparedKind::Delete => fs::remove_file(root.join(&p.path))
                .map_err(|e| format!("удаление {}: {e}", p.path)),
        };
        if let Err(reason) = result {
            let err = FilePatchError { path: p.path.clone(), reason };
            rollback(root, backup_root, &applied, &report.backed_up);
            report.failed.push(err);
            return report;
        }
        applied.push(p.path.clone());
    }

    // Манифест для Undo — только после полного успеха. Файлы уже записаны,
    // поэтому ошибка манифеста не выбрасывает список применённого.
    if let Err(e) = write_manifest(backup_root, prepared) {
        report.failed.push(FilePatchError {
            path: "(операция)".to_string(),
            reason: format!("манифест бэкапа: {e} (файлы записаны, Undo будет недоступен)"),
        });
    }

    report.applied = applied;
    report
}

fn fail_all(reason: String) -> ApplyReport {
    let mut report = ApplyReport::default();
    report.failed.push(FilePatchError {
        path: "(операция)".to_string(),
        reason,
    });
    report
}

/// Откат уже применённых файлов: восстановить из бэкапа «заменённые»,
/// удалить «созданные» (их нет в бэкапе).
fn rollback(root: &Path, backup_root: &Path, applied: &[String], backed_up: &[String]) {
    for path in applied {
        let full = root.join(path);
        if backed_up.contains(path) {
            let backup = backup_root.join(path);
            if let Err(e) = fs::copy(&backup, &full) {
                log_restore_failure(path, &e);
            }
        } else if let Err(e) = fs::remove_file(&full) {
            log_restore_failure(path, &e);
        }
    }
}

fn log_restore_failure(path: &str, e: &std::io::Error) {
    // Ошибку отката молча не глотаем: наружу через log (логгер хоста).
    log::error!("откат {}: {e}", path);
}

/// Скопировать текущее состояние файла в бэкап с сохранением относительного пути.
fn backup_file(root: &Path, backup_root: &Path, rel_path: &str) -> std::io::Result<()> {
    let dest = backup_root.join(rel_path);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(root.join(rel_path), &dest)?;
    Ok(())
}

/// Атомарная запись: tmp в той же папке → rename поверх существующего.
fn atomic_write(dest: &Path, content: &str) -> Result<(), String> {
    let parent = dest.parent().ok_or_else(|| format!("нет родителя у {dest:?}"))?;
    fs::create_dir_all(parent).map_err(|e| format!("создание папки {parent:?}: {e}"))?;

    let file_name = dest
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .ok_or_else(|| format!("некорректное имя файла {dest:?}"))?;
    let tmp = parent.join(format!(".{file_name}.vibebridge-tmp"));

    fs::write(&tmp, content).map_err(|e| format!("запись {tmp:?}: {e}"))?;
    fs::rename(&tmp, dest).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("переименование {tmp:?} → {dest:?}: {e}")
    })
}

/// Манифест Undo: `replaced|created|deleted <TAB> относительный/путь`.
fn write_manifest(backup_root: &Path, prepared: &[PreparedChange]) -> std::io::Result<()> {
    fs::create_dir_all(backup_root)?;
    let mut text = String::new();
    for p in prepared {
        let existed = backup_root.join(&p.path).exists();
        let op = match &p.kind {
            PreparedKind::Delete => "deleted",
            _ if existed => "replaced",
            _ => "created",
        };
        text.push_str(&format!("{op}\t{}\n", p.path));
    }
    atomic_write(&backup_root.join(MANIFEST_NAME), &text).map_err(|e| std::io::Error::new(
        std::io::ErrorKind::Other,
        e,
    ))
}

// ---------------------------------------------------------------- undo

/// Откат последнего применения по манифесту: replaced/deleted — вернуть файл
/// из бэкапа, created — удалить. Возвращает число восстановленных файлов.
pub fn undo_last(root: &Path, backup_root: &Path) -> Result<usize, String> {
    let manifest_path = backup_root.join(MANIFEST_NAME);
    if !manifest_path.exists() {
        return Err("нет манифеста бэкапа — откатывать нечего".to_string());
    }
    let text = fs::read_to_string(&manifest_path)
        .map_err(|e| format!("чтение манифеста: {e}"))?;

    let mut restored = 0;
    for line in text.lines() {
        let Some((op, rel)) = line.split_once('\t') else {
            return Err(format!("битая строка манифеста: `{line}`"));
        };
        match op {
            "replaced" | "deleted" => {
                let src = backup_root.join(rel);
                let dst = root.join(rel);
                if let Some(parent) = dst.parent() {
                    fs::create_dir_all(parent).map_err(|e| format!("папка {parent:?}: {e}"))?;
                }
                fs::copy(&src, &dst).map_err(|e| format!("восстановление {rel}: {e}"))?;
                restored += 1;
            }
            "created" => {
                let dst = root.join(rel);
                if dst.exists() {
                    fs::remove_file(&dst).map_err(|e| format!("удаление созданного {rel}: {e}"))?;
                }
                restored += 1;
            }
            other => return Err(format!("неизвестная операция манифеста: `{other}`")),
        }
    }

    // Манифест «гасим», чтобы повторный Undo не откатил чужие изменения.
    fs::rename(&manifest_path, backup_root.join(MANIFEST_DONE))
        .map_err(|e| format!("закрытие манифеста: {e}"))?;
    Ok(restored)
}
