//! Фоновый воркер UI (вердикт спеца №1): тяжёлые операции ядра — вне потока
//! отрисовки; связь — два mpsc-канала (команды UI → воркер, события обратно).
//! UI опрашивает события неблокирующе в начале каждого кадра.
//! AI-подбор файлов — `ai_pick.rs`: воркер спавнит под него отдельный поток
//! (генерация блокирует, но цикл воркера продолжает обслуживать скан/токены),
//! cancel-флаг приходит прямо в команде — UI гасит генерацию без круга через
//! занятый воркер.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

use agent_core::tree::scanner::{self, ScanOutput};
use agent_core::tree::tokens::token_cache;

use crate::ai_pick;

/// Команда UI → воркер.
pub enum Command {
    /// Просканировать папку проекта.
    Scan { root: PathBuf },
    /// Посчитать токены одного файла (событие на файл — прогрессивный UI).
    Tokens { root: PathBuf, rel: String },
    /// AI-подбор файлов под черновик. `cancel` создаёт UI и живёт в нём:
    /// кнопка «Отменить» поднимает флаг напрямую (минуя командный канал).
    /// `project_paths` — selectable-пути скана UI: источник истинности тулов
    /// AI-подбора (find_files) и анти-галлюцинации.
    PickFiles {
        root: PathBuf,
        draft: String,
        cancel: Arc<AtomicBool>,
        project_paths: Vec<String>,
    },
}

/// Событие воркер → UI.
pub enum Event {
    Scanned { root: PathBuf, out: ScanOutput },
    Tokens { root: PathBuf, rel: String, count: Option<usize> },
    /// Промежуточный статус AI-подбора (прогресс в строке статуса).
    AiStatus { root: PathBuf, msg: String },
    /// AI-подбор готов: сырые предложенные пути (валидация — на стороне UI).
    AiPicked { root: PathBuf, selected: Vec<String>, reasoning: String },
    /// AI-подбор не удался; текст — для строки статуса.
    AiPickFailed { root: PathBuf, msg: String },
}

/// Запустить воркер-поток; вернуть UI-сторону каналов. `base` — папка exe:
/// воркер читает конфиг VibeBridge для AI-подбора.
pub fn spawn(base: PathBuf) -> (Sender<Command>, Receiver<Event>) {
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
    let (ev_tx, ev_rx) = std::sync::mpsc::channel();
    match std::thread::Builder::new()
        .name("vb-worker".to_string())
        .spawn(move || worker_loop(cmd_rx, ev_tx, base))
    {
        Ok(_) => log::info!("воркер-поток запущен"),
        Err(e) => log::error!(" воркер-поток не запущен: {e} — скан/токены работать не будут"),
    }
    (cmd_tx, ev_rx)
}

fn worker_loop(rx: Receiver<Command>, tx: Sender<Event>, base: PathBuf) {
    // Активность AI-подбора: защита от двух движков одновременно. UI-busy
    // привязан к root (смена папки снимает busy) — без этого гварда можно
    // было бы запустить второй движок под новую папку.
    let ai_active = Arc::new(AtomicBool::new(false));
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
            Command::PickFiles { root, draft, cancel, project_paths } => {
                if ai_active.swap(true, Ordering::SeqCst) {
                    log::warn!("ai-pick: уже идёт — новая команда отброшена");
                    let _ = tx.send(Event::AiPickFailed {
                        root,
                        msg: "уже идёт другой AI-подбор — подожди его завершения".to_string(),
                    });
                    continue;
                }
                log::info!("ai-pick: старт ({})", root.display());
                // Клоны ДО spawn: move-замыкание забирает их себе, оригиналы
                // (tx/base) остаются воркеру на следующие итерации цикла.
                let active = ai_active.clone();
                let thread_active = active.clone();
                let pick_tx = tx.clone();
                let pick_base = base.clone();
                let pick_root = root.clone();
                let pick_paths = project_paths.clone();
                let spawned = std::thread::Builder::new()
                    .name("vb-ai-pick".to_string())
                    .spawn(move || {
                        let status = {
                            let tx = pick_tx.clone();
                            let root = pick_root.clone();
                            move |msg: String| {
                                let _ = tx.send(Event::AiStatus { root: root.clone(), msg });
                            }
                        };
                        match ai_pick::pick_files(&pick_root, &pick_base, &pick_paths, &draft, cancel, &status) {
                            Ok(out) => {
                                log::info!("ai-pick: готово, файлов {}", out.selected.len());
                                let _ = pick_tx.send(Event::AiPicked {
                                    root: pick_root,
                                    selected: out.selected,
                                    reasoning: out.reasoning,
                                });
                            }
                            Err(e) => {
                                log::error!("ai-pick: {e}");
                                let _ = pick_tx.send(Event::AiPickFailed { root: pick_root, msg: e });
                            }
                        }
                        // Снять активность ДО терминального события: UI,
                        // получив его, может сразу послать новый подбор.
                        thread_active.store(false, Ordering::SeqCst);
                    });
                if let Err(e) = spawned {
                    log::error!("ai-pick: поток не запущен: {e}");
                    active.store(false, Ordering::SeqCst);
                    let _ = tx.send(Event::AiPickFailed {
                        root,
                        msg: format!("поток подбора не запущен: {e}"),
                    });
                }
            }
        }
    }
}
