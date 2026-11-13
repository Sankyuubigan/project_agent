//! Фоновый воркер UI (вердикт спеца №1): тяжёлые операции ядра — вне потока
//! отрисовки; связь — два mpsc-канала (команды UI → воркер, события обратно).
//! UI опрашивает события неблокирующе в начале каждого кадра.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};

use agent_core::tree::scanner::{self, ScanOutput};
use agent_core::tree::tokens::token_cache;

/// Команда UI → воркер.
pub enum Command {
    /// Просканировать папку проекта.
    Scan { root: PathBuf },
    /// Посчитать токены одного файла (событие на файл — прогрессивный UI).
    Tokens { root: PathBuf, rel: String },
}

/// Событие воркер → UI.
pub enum Event {
    Scanned { root: PathBuf, out: ScanOutput },
    Tokens { root: PathBuf, rel: String, count: Option<usize> },
}

/// Запустить воркер-поток; вернуть UI-сторону каналов.
pub fn spawn() -> (Sender<Command>, Receiver<Event>) {
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let (ev_tx, ev_rx) = std::sync::mpsc::channel();
    match std::thread::Builder::new()
        .name("vb-worker".to_string())
        .spawn(move || worker_loop(cmd_rx, ev_tx))
    {
        Ok(_) => log::info!("воркер-поток запущен"),
        Err(e) => log::error!("воркер-поток не запущен: {e} — скан/токены работать не будут"),
    }
    (cmd_tx, ev_rx)
}

fn worker_loop(rx: Receiver<Command>, tx: Sender<Event>) {
    while let Ok(cmd) = rx.recv() {
        match cmd {
            Command::Scan { root } => {
                if !root.is_dir() {
                    log::error!("скан: папка не найдена {}", root.display());
                }
                let out = scanner::scan(&root);
                log::info!(
                    "скан {}: файлов {}, папок {}",
                    root.display(),
                    out.files.len(),
                    out.dirs.len()
                );
                let _ = tx.send(Event::Scanned { root, out });
            }
            Command::Tokens { root, rel } => {
                let count = token_cache()
                    .count_file(&root.join(&rel))
                    .unwrap_or_else(|e| {
                        log::warn!("токены {rel}: {e}");
                        None
                    });
                let _ = tx.send(Event::Tokens { root, rel, count });
            }
        }
    }
}
