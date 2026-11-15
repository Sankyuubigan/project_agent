//! Перезапуск приложения при смене движка интерфейса (PRP 14.11.26, шаг 2).
//!
//! Запускает новый процесс того же exe (конфиг к этому моменту уже записан
//! вызывающей стороной); новый запуск сам выберет ветку `ui_backend` в
//! `main.rs`. Текущий процесс после успешного spawn закрывает своё окно и
//! завершается штатно.

use std::path::{Path, PathBuf};

/// Запускает новый процесс того же exe. При ошибке — в лог и `false`
/// (вызывающая сторона НЕ закрывает текущее окно).
pub fn relaunch() -> bool {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            log::error!("restart: не удалось определить current_exe: {e}");
            return false;
        }
    };
    // Рабочая папка нового процесса — папка exe (правило владельца §6).
    let cwd = exe
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let mut cmd = std::process::Command::new(&exe);
    cmd.current_dir(&cwd);
    // Правило владельца: никакого консольного окна (и для GUI-варианта тоже).
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    match cmd.spawn() {
        Ok(child) => {
            log::info!("restart: запущен новый процесс (PID {})", child.id());
            true
        }
        Err(e) => {
            log::error!("restart: не удалось запустить {}: {e}", exe.display());
            false
        }
    }
}
