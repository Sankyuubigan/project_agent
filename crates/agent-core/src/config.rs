//! Конфиг приложения: `config.json` рядом с exe (правило владельца §6 —
//! пути от `current_exe().parent()`). Атомарная запись: tmp + rename.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Имя файла конфига рядом с exe.
const CONFIG_NAME: &str = "config.json";

/// Настройки, пережидающие перезапуск.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppConfig {
    /// Последняя открытая папка проекта.
    #[serde(default)]
    pub last_project_dir: Option<String>,
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
        let cfg = AppConfig { last_project_dir: Some("D:/tmp/proj".to_string()) };
        save(dir.path(), &cfg).unwrap();
        assert_eq!(load(dir.path()), cfg);
        // tmp-файл после атомарной записи не остаётся.
        assert!(!dir.path().join("config.json.tmp").exists());
    }

    #[test]
    fn corrupt_config_falls_back_to_default() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("config.json"), "{битый json").unwrap();
        assert_eq!(load(dir.path()), AppConfig::default());
    }
}
