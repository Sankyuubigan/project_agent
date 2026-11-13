//! Подсчёт токенов (tiktoken gpt2) с кэшем по ключу `(path, mtime, size)`.
//!
//! Энкодер — глобальный `OnceLock` (инициализация ~сотни мс, делается один раз).
//! Кэш — глобальный `Mutex<HashMap>`; повторный подсчёт неизменённого файла
//! мгновенен (вердикт спеца №5).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use tiktoken_rs::CoreBPE;

/// Порог отображения токенов ОДНОГО файла (порт file_processing.py; в старой
/// версии применялся в treeview_scanner.py): файл с бо́льшим числом токенов
/// показывается в дереве как «> 50 000», точное число не рисуется. Суммы и
/// подсчёт порог не ограничивает.
pub const MAX_TOKENS_FOR_DISPLAY: usize = 50_000;

/// Число с пробельными группами разрядов («50 000») — формат старой версии.
pub fn spaced(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, b) in s.bytes().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(' ');
        }
        out.push(b as char);
    }
    out
}

/// Энкодер gpt2: None, если инициализация не удалась (ошибка — в лог).
fn encoder() -> Option<&'static CoreBPE> {
    static ENCODER: OnceLock<Option<CoreBPE>> = OnceLock::new();
    ENCODER
        .get_or_init(|| match tiktoken_rs::r50k_base() {
            Ok(bpe) => Some(bpe),
            Err(e) => {
                log::error!("инициализация tiktoken gpt2 не удалась: {e}");
                None
            }
        })
        .as_ref()
}

/// Ключ кэша: (mtime, size).
#[derive(Clone, Copy)]
struct Stamp {
    mtime: SystemTime,
    size: u64,
}

struct Cached {
    stamp: Stamp,
    tokens: Option<usize>,
}

/// Кэш подсчёта токенов.
pub struct TokenCache {
    map: Mutex<HashMap<PathBuf, Cached>>,
}

impl TokenCache {
    /// Токены файла. `Ok(None)` — файл не текстовый для подсчёта (слишком
    /// большой/не читается как UTF-8), ошибка чтения — `Err`.
    pub fn count_file(&self, path: &Path) -> Result<Option<usize>, String> {
        let meta = std::fs::metadata(path).map_err(|e| format!("{path:?}: {e}"))?;
        let stamp = Stamp {
            mtime: meta.modified().map_err(|e| format!("{path:?}: {e}"))?,
            size: meta.len(),
        };

        if let Some(cached) = self.map.lock().unwrap_or_else(|p| p.into_inner()).get(path) {
            if cached.stamp.mtime == stamp.mtime && cached.stamp.size == stamp.size {
                return Ok(cached.tokens);
            }
        }

        if stamp.size > super::scanner::MAX_FILE_SIZE_BYTES {
            self.remember(path, stamp, None);
            return Ok(None);
        }

        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("{path:?}: не UTF-8 или не читается: {e}"))?;
        let tokens = encoder().map(|bpe| bpe.encode_ordinary(&text).len());
        self.remember(path, stamp, tokens);
        Ok(tokens)
    }

    /// Токены строки (буфер обмена/черновик), без кэша.
    pub fn count_text(&self, text: &str) -> Option<usize> {
        encoder().map(|bpe| bpe.encode_ordinary(text).len())
    }

    fn remember(&self, path: &Path, stamp: Stamp, tokens: Option<usize>) {
        let mut map = self.map.lock().unwrap_or_else(|p| p.into_inner());
        map.insert(path.to_path_buf(), Cached { stamp, tokens });
    }
}

/// Глобальный кэш приложения.
pub fn token_cache() -> &'static TokenCache {
    static CACHE: OnceLock<TokenCache> = OnceLock::new();
    CACHE.get_or_init(|| TokenCache {
        map: Mutex::new(HashMap::new()),
    })
}
