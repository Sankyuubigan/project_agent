//! Черновик запроса (Шаг 4 PRP): промпт + вложения, переживающие перезапуск.
//!
//! Хранилище: `<base>/drafts/` — `manifest.json` (текст промпта + метаданные
//! вложений, атомарная запись) и копии вложений в `attachments/`. Вложение
//! добавляется КОПИЕЙ: исходник можно удалить — черновик жив.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const MANIFEST_NAME: &str = "manifest.json";
/// Куда переименовывается битый манифест (диагностика, не молчание).
const MANIFEST_BAD: &str = "manifest.json.bad";
const ATTACH_DIR: &str = "attachments";

/// Расширения-картинки: копируются в чат через CF_HDROP, не текстом.
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "bmp", "webp"];

/// Одно вложение черновика (копия в `attachments/`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftAttachment {
    pub file_name: String,
    #[serde(default = "default_checked")]
    pub checked: bool,
}

fn default_checked() -> bool {
    true
}

/// Манифест черновика — единственное хранилище его стейта.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DraftManifest {
    #[serde(default)]
    pub prompt: String,
    #[serde(default = "default_checked")]
    pub prompt_checked: bool,
    #[serde(default)]
    pub attachments: Vec<DraftAttachment>,
}

/// Хранилище черновика: папка `<base>/drafts`.
pub struct DraftStore {
    dir: PathBuf,
}

impl DraftStore {
    pub fn new(base: &Path) -> Self {
        Self { dir: base.join("drafts") }
    }

    pub fn path_of(&self, file_name: &str) -> PathBuf {
        self.attachments_dir().join(file_name)
    }

    pub fn attachments_dir(&self) -> PathBuf {
        self.dir.join(ATTACH_DIR)
    }

    /// Картинка ли (по расширению, без учёта регистра).
    pub fn is_image(file_name: &str) -> bool {
        Path::new(file_name)
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| IMAGE_EXTENSIONS.contains(&e.to_lowercase().as_str()))
    }

    /// Загрузить манифест: нет файла — дефолт; битый — копия `.bad` + дефолт
    /// (перезапись битого дефолтом без копии = молчаливая потеря).
    pub fn load(&self) -> DraftManifest {
        let path = self.dir.join(MANIFEST_NAME);
        if !path.is_file() {
            return DraftManifest::default();
        }
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                log::error!("манифест {}: не прочитан ({e}) — черновик сброшен", path.display());
                return DraftManifest::default();
            }
        };
        match serde_json::from_str::<DraftManifest>(&text) {
            Ok(m) => m,
            Err(e) => {
                log::error!(
                    "манифест {} битый ({e}) — копия в {MANIFEST_BAD}, черновик сброшен",
                    path.display()
                );
                let _ = fs::rename(&path, self.dir.join(MANIFEST_BAD));
                DraftManifest::default()
            }
        }
    }

    /// Атомарная запись манифеста (tmp + rename).
    pub fn save(&self, manifest: &DraftManifest) -> Result<(), String> {
        fs::create_dir_all(&self.dir).map_err(|e| format!("{}: {e}", self.dir.display()))?;
        let path = self.dir.join(MANIFEST_NAME);
        let tmp = self.dir.join(format!("{MANIFEST_NAME}.tmp"));
        let text = serde_json::to_string_pretty(manifest)
            .map_err(|e| format!("сериализация манифеста: {e}"))?;
        fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
        fs::rename(&tmp, &path)
            .map_err(|e| format!("{} → {}: {e}", tmp.display(), path.display()))?;
        Ok(())
    }

    /// Скопировать файл в `attachments/` под уникальным именем; вернуть имя.
    pub fn add_copied_file(&self, src: &Path) -> Result<String, String> {
        if !src.is_file() {
            return Err(format!("{}: не файл", src.display()));
        }
        let raw = src
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .ok_or_else(|| format!("{}: нет имени файла", src.display()))?;
        self.write_unique(&raw, |dest| fs::copy(src, dest).map(|_| ()))
    }

    /// Записать PNG-байты (например, картинку из буфера) в `attachments/`.
    pub fn add_png_bytes(&self, stem: &str, png: &[u8]) -> Result<String, String> {
        let raw = format!("{}.png", sanitize(stem));
        let data = png.to_vec();
        self.write_unique(&raw, move |dest| fs::write(dest, &data))
    }

    /// Записать данные в `attachments/` под уникальным именем `raw`.
    /// Единая точка уникальности: `<мс>_<имя>`, коллизия — следующий штамп.
    fn write_unique(
        &self,
        raw: &str,
        write: impl FnOnce(&Path) -> std::io::Result<()>,
    ) -> Result<String, String> {
        fs::create_dir_all(self.attachments_dir())
            .map_err(|e| format!("{}: {e}", self.attachments_dir().display()))?;
        let mut ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        loop {
            let name = unique_name(ms, raw);
            let dest = self.path_of(&name);
            if !dest.exists() {
                write(&dest).map_err(|e| format!("{}: {e}", dest.display()))?;
                return Ok(name);
            }
            ms += 1;
        }
    }

    /// Удалить копию вложения. Файла уже нет — не ошибка (цель достигнута).
    pub fn remove(&self, file_name: &str) -> Result<(), String> {
        let path = self.path_of(file_name);
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                log::warn!("вложение {} уже отсутствует", path.display());
                Ok(())
            }
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }
}

/// `<мс-эпоха>_<безопасное имя>`: префикс гарантирует уникальность,
/// санитизация — валидность имени на Windows.
fn unique_name(ms: u128, raw: &str) -> String {
    let file = Path::new(raw);
    let stem = file
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    let ext = file.extension().map(|e| e.to_string_lossy().to_string());
    let safe = sanitize(&stem);
    match ext {
        Some(e) => format!("{ms}_{safe}.{e}"),
        None => format!("{ms}_{safe}"),
    }
}

/// Буквы/цифры (включая юникод) и `._- ` остаются, прочее → `_`.
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '.' | '_' | '-' | ' ') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn is_image_by_extension() {
        assert!(DraftStore::is_image("a.PNG"));
        assert!(DraftStore::is_image("b.jpeg"));
        assert!(!DraftStore::is_image("c.txt"));
        assert!(!DraftStore::is_image("png"));
    }

    #[test]
    fn save_load_roundtrip() {
        let dir = tempdir().unwrap();
        let store = DraftStore::new(dir.path());
        let mut m = DraftManifest::default();
        m.prompt = "запрос".to_string();
        m.attachments.push(DraftAttachment { file_name: "1_x.txt".into(), checked: false });
        store.save(&m).unwrap();
        assert_eq!(store.load(), m);
    }

    #[test]
    fn corrupt_manifest_goes_to_bad_and_resets() {
        let dir = tempdir().unwrap();
        let store = DraftStore::new(dir.path());
        fs::create_dir_all(&store.dir).unwrap();
        fs::write(store.dir.join(MANIFEST_NAME), "{битый").unwrap();
        assert_eq!(store.load(), DraftManifest::default());
        assert!(store.dir.join(MANIFEST_BAD).is_file());
    }

    #[test]
    fn add_copied_file_and_remove() {
        let dir = tempdir().unwrap();
        let store = DraftStore::new(dir.path());
        let src = dir.path().join("оригинал.txt");
        fs::write(&src, "данные").unwrap();

        let name = store.add_copied_file(&src).unwrap();
        assert_ne!(name, "оригинал.txt", "имя должно получить префикс-штамп");
        assert_eq!(fs::read_to_string(store.path_of(&name)).unwrap(), "данные");

        let second = store.add_copied_file(&src).unwrap();
        assert_ne!(name, second, "повторное добавление — другое имя");

        store.remove(&name).unwrap();
        assert!(!store.path_of(&name).exists());
        store.remove(&name).unwrap(); // повторное удаление — не ошибка
    }

    #[test]
    fn add_png_bytes_writes_file() {
        let dir = tempdir().unwrap();
        let store = DraftStore::new(dir.path());
        let name = store.add_png_bytes("clipboard", b"\x89PNG fake").unwrap();
        assert!(name.ends_with(".png"));
        assert_eq!(fs::read(store.path_of(&name)).unwrap(), b"\x89PNG fake");
    }

    #[test]
    fn sanitize_replaces_path_separators() {
        assert_eq!(sanitize("a/b\\c:d"), "a_b_c_d");
        assert_eq!(sanitize("русский файл-1.txt"), "русский файл-1.txt");
    }
}
