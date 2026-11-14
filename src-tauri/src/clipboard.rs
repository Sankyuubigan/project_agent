//! Буфер обмена Windows (инфраструктура UI, ядро буфера не знает):
//! - текст — `arboard` (промпт, ответ LLM);
//! - список файлов CF_HDROP — `clipboard-win` («как из Проводника»):
//!   картинки-вложения в веб-чат вставляются одним Ctrl+V;
//! - картинка ИЗ буфера (Win+Shift+S) → PNG-байты (`png`, чистый Rust).

use std::path::PathBuf;

/// Скопировать текст в буфер.
pub fn copy_text(text: &str) -> Result<(), String> {
    arboard::Clipboard::new()
        .and_then(|mut c| c.set_text(text.to_owned()))
        .map_err(|e| format!("буфер обмена (текст): {e}"))
}

/// Прочитать текст из буфера обмена.
pub fn read_text() -> Result<String, String> {
    arboard::Clipboard::new()
        .and_then(|mut c| c.get_text())
        .map_err(|e| format!("буфер обмена (чтение): {e}"))
}

/// Скопировать список файлов как CF_HDROP.
pub fn copy_file_list(paths: &[PathBuf]) -> Result<(), String> {
    if paths.is_empty() {
        return Err("список файлов для буфера пуст".to_string());
    }
    let strs: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
    // Клипборд открыт на время записи; drop закрывает (обязательное правило Win32).
    let _clip = clipboard_win::Clipboard::new_attempts(10)
        .map_err(|e| format!("буфер обмена: не открыт ({e})"))?;
    clipboard_win::raw::set_file_list(&strs).map_err(|e| format!("буфер обмена (CF_HDROP): {e}"))
}

/// Достать картинку из буфера (скриншот Win+Shift+S) и закодировать в PNG.
pub fn png_from_clipboard() -> Result<Vec<u8>, String> {
    let img = arboard::Clipboard::new()
        .and_then(|mut c| c.get_image())
        .map_err(|e| format!("в буфере нет картинки: {e}"))?;
    encode_png(img.width as u32, img.height as u32, &img.bytes)
}

/// RGBA → PNG в память. arboard отдаёт ровно `w*h*4` байт RGBA8.
fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    use png::{BitDepth, ColorType, Encoder};
    let expected = width as usize * height as usize * 4;
    if rgba.len() != expected {
        return Err(format!(
            "картинка из буфера: ожидалось {expected} байт RGBA, получено {}",
            rgba.len()
        ));
    }
    let mut out = Vec::new();
    // Скоуп нужен: Writer держит &mut out до конца блока.
    {
        let mut encoder = Encoder::new(&mut out, width, height);
        encoder.set_color(ColorType::Rgba);
        encoder.set_depth(BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| format!("PNG header: {e}"))?;
        writer
            .write_image_data(rgba)
            .map_err(|e| format!("PNG data: {e}"))?;
    }
    Ok(out)
}
