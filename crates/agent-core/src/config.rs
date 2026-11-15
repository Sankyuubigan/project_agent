//! Конфиг приложения: `config.json` рядом с exe (правило владельца §6 —
//! пути от `current_exe().parent()`). Атомарная запись: tmp + rename.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Имя файла конфига рядом с exe.
const CONFIG_NAME: &str = "config.json";

/// Движок интерфейса приложения (PRP 14.11.26 «GPUI-фронтенд»).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UiBackend {
    /// Текущий фронтенд: окно Tauri + egui через tauri-plugin-egui.
    #[default]
    #[serde(rename = "egui")]
    Egui,
    /// Экспериментальный: нативное окно gpui-kit, Tauri не инициализируется.
    #[serde(rename = "gpui")]
    Gpui,
}

impl std::fmt::Display for UiBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Egui => write!(f, "egui"),
            Self::Gpui => write!(f, "gpui"),
        }
    }
}

/// Настройки, пережидающие перезапуск.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppConfig {
    /// Последняя открытая папка проекта.
    #[serde(default)]
    pub last_project_dir: Option<String>,
    /// Путь к GGUF-модели для AI-подбора файлов (PRP 13.11.26). None — первая
    /// существующая модель из каталога движка (tauri-plugin-llama-engine).
    #[serde(default)]
    pub ai_model_path: Option<String>,
    /// Путь к тул-бинарю agent-tools.exe для карты проекта. None — дефолт
    /// (машина владельца, см. `tree::repomap::DEFAULT_TOOL`).
    #[serde(default)]
    pub repomap_tool: Option<String>,
    /// Движок интерфейса: egui (дефолт) или gpui (экспериментальный). Меняется
    /// во вкладке «Настройки» + «Сохранить и перезапустить».
    #[serde(default)]
    pub ui_backend: UiBackend,
}

/// Загрузить конфиг; файла нет — дефолт; битый — дефолт с громким логом
/// (молчаливых подмен нет).
pub fn load(base: &Path) -> AppConfig {
    let path = base.join(CONFIG_NAME);
    if !path.is_file() {
        return AppConfig::default();
    }
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            log::error!("конфиг {}: не прочитан ({e}) — используется дефолт", path.display());
            return AppConfig::default();
        }
    };
    match serde_json::from_str::<AppConfig>(&text) {
        Ok(cfg) => cfg,
        Err(e) => {
            log::error!("конфиг {}: не разобран ({e}) — используется дефолт", path.display());
            AppConfig::default()
        }
    }
}

/// Сохранить конфиг атомарно (tmp + rename).
pub fn save(base: &Path, cfg: &AppConfig) -> Result<(), String> {
    let path = base.join(CONFIG_NAME);
    let tmp = base.join(format!("{CONFIG_NAME}.tmp"));
    let text =
        serde_json::to_string_pretty(cfg).map_err(|e| format!("сериализация конфига: {e}"))?;
    fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    fs::rename(&tmp, &path)
        .map_err(|e| format!("{} → {}: {e}", tmp.display(), path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn missing_config_is_default() {
        let dir = tempdir().unwrap();
        assert_eq!(load(dir.path()), AppConfig::default());
    }

    #[test]
    fn save_load_roundtrip() {
        let dir = tempdir().unwrap();
        let cfg = AppConfig { last_project_dir: Some("D:/tmp/proj".to_string()), ..Default::default() };
        save(dir.path(), &cfg).unwrap();
        assert_eq!(load(dir.path()), cfg);
        // tmp-файл после атомарной записи не остаётся.
        assert!(!dir.path().join("config.json.tmp").exists());
    }

    #[test]
    fn ai_fields_survive_roundtrip() {
        let dir = tempdir().unwrap();
        let cfg = AppConfig {
            last_project_dir: None,
            ai_model_path: Some("D:/models/qwen.gguf".to_string()),
            repomap_tool: Some("C:/tools/agent-tools.exe".to_string()),
            ..Default::default()
        };
        save(dir.path(), &cfg).unwrap();
        assert_eq!(load(dir.path()), cfg);
    }

    #[test]
    fn corrupt_config_falls_back_to_default() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("config.json"), "{битый json").unwrap();
        assert_eq!(load(dir.path()), AppConfig::default());
    }

    #[test]
    fn old_config_without_backend_defaults_to_egui() {
        // Конфиг версии без ui_backend: поле отсутствует — дефолт egui, остальные
        // значения не теряются (serde(default), молчаливых подмен нет).
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join(CONFIG_NAME),
            r#"{ "last_project_dir": "D:/tmp/proj" }"#,
        )
        .unwrap();
        let cfg = load(dir.path());
        assert_eq!(cfg.ui_backend, UiBackend::Egui);
        assert_eq!(cfg.last_project_dir.as_deref(), Some("D:/tmp/proj"));
    }

    #[test]
    fn ui_backend_roundtrip_and_lowercase_serialization() {
        let dir = tempdir().unwrap();
        let cfg = AppConfig { ui_backend: UiBackend::Gpui, ..Default::default() };
        save(dir.path(), &cfg).unwrap();
        assert_eq!(load(dir.path()).ui_backend, UiBackend::Gpui);
        // В файле значение сериализуется lowercase-строкой (контракт config.json).
        let text = std::fs::read_to_string(dir.path().join(CONFIG_NAME)).unwrap();
        assert!(text.contains("\"gpui\""), "ожидалась lowercase-сериализация: {text}");
        // Display — человекочитаемое имя для логов/настроек.
        assert_eq!(UiBackend::Egui.to_string(), "egui");
        assert_eq!(UiBackend::Gpui.to_string(), "gpui");
    }
}
