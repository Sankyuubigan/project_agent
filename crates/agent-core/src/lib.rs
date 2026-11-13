//! # agent-core — ядро VibeBridge
//!
//! Чистый Rust без tauri/egui (вердикт спеца №5): компилятор не даст протечь
//! UI в ядро, юнит-тесты `cargo test --lib` идут за секунды.
//!
//! Модули (добавляются по шагам PRP):
//! - `patch` — парсер/аплаер «Умного Markdown» (Шаг 2)
//! - `tree` — сканер дерева файлов, токены, сборщик промпта (Шаг 3)
//! - `config` — конфиг рядом с exe (Шаг 4)
//! - `draft` — хранилище черновика запроса (Шаг 4)

pub mod config;
pub mod draft;
pub mod patch;
pub mod tree;

/// Заглушка Шага 1: проверка связки workspace → src-tauri.
pub fn core_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_is_yy_m_p() {
        let v = super::core_version();
        assert!(v.contains('.'), "версия должна быть вида YY.M.P: {v}");
    }
}
