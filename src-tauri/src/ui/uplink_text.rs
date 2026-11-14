//! Небольшие текстовые хелперы Uplink. Вынесены из uplink.rs ради лимита
//! 500 строк на файл; используются кодом uplink и модульными тестами.

/// Нормализовать предложенный ИИ путь: `\` → `/`, без ведущего `./`.
pub(super) fn normalize_ai_path(p: &str) -> String {
    let p = p.trim().replace('\\', "/");
    p.strip_prefix("./").unwrap_or(&p).to_string()
}

/// Reasoning для строки статуса: обрезка до `max_chars` символов
/// (полный текст — в логе воркера).
pub(super) fn truncate_for_status(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max_chars).collect();
    t.push('…');
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_slashes_and_dot() {
        assert_eq!(normalize_ai_path("src\\main.rs"), "src/main.rs");
        assert_eq!(normalize_ai_path("./src/main.rs"), "src/main.rs");
        assert_eq!(normalize_ai_path("  a/b.rs  "), "a/b.rs");
        assert_eq!(normalize_ai_path(""), "");
        // Абсолютные пути не чинятся в относительные — их отсеет скан-фильтр.
        assert_eq!(normalize_ai_path("D:\\proj\\a.rs"), "D:/proj/a.rs");
    }

    #[test]
    fn truncate_keeps_short_and_cuts_long() {
        assert_eq!(truncate_for_status("короткий", 10), "короткий");
        let long = "ж".repeat(200);
        let cut = truncate_for_status(&long, 160);
        assert_eq!(cut.chars().count(), 161); // 160 + многоточие
        assert!(cut.ends_with('…'));
    }
}
