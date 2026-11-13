//! Каскад поиска якоря SEARCH/REPLACE (вердикт спеца №2, БЕЗ Левенштейна):
//! 1) точное совпадение после нормализации `\r\n`→`\n`;
//! 2) построчный `trim_end` (устойчивость к хвостовым пробелам/CR);
//! 3) relative indentation (одинаковый сдвиг отступа во всём фрагменте).
//!
//! Совпадений строго одно: 0 → `AnchorError::NotFound`, ≥2 → `Ambiguous`.

/// Ошибка поиска якоря.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnchorError {
    /// Совпадений 0.
    NotFound,
    /// Совпадений больше одного — нужен более длинный фрагмент.
    Ambiguous { count: usize },
}

impl AnchorError {
    pub fn reason(&self) -> String {
        match self {
            AnchorError::NotFound => "SEARCH не найден в файле".to_string(),
            AnchorError::Ambiguous { count } => {
                format!("SEARCH встречается {count} раз — нужен более длинный уникальный фрагмент")
            }
        }
    }
}

/// Применить одну точечную замену каскадом. Результат — LF.
pub fn apply_edit(content: &str, search: &str, replace: &str) -> Result<String, AnchorError> {
    let content_n = content.replace("\r\n", "\n");
    let search_n = search.replace("\r\n", "\n");
    let replace_n = replace.replace("\r\n", "\n");

    if search_n.is_empty() {
        return Err(AnchorError::NotFound);
    }

    // Уровень 1: точное совпадение.
    let hits = find_all(&content_n, &search_n);
    if hits.len() == 1 {
        return Ok(splice_str(&content_n, hits[0], search_n.len(), &replace_n));
    }
    if hits.len() > 1 {
        return Err(AnchorError::Ambiguous { count: hits.len() });
    }

    let lines: Vec<&str> = content_n.split('\n').collect();
    // Хвостовой `\n` в SEARCH — граница строки, а не требование пустой
    // строки после неё (иначе построчный матчинг этот хвост не находит).
    let search_lines = line_split(&search_n);
    if search_lines.len() > lines.len() {
        return Err(AnchorError::NotFound);
    }

    // Уровень 2: построчный trim_end.
    let mut hits2 = Vec::new();
    for start in 0..=(lines.len() - search_lines.len()) {
        let window = &lines[start..start + search_lines.len()];
        if window.iter().zip(&search_lines).all(|(a, b)| a.trim_end() == b.trim_end()) {
            hits2.push(start);
        }
    }
    if hits2.len() == 1 {
        return Ok(splice_window(&lines, hits2[0], search_lines.len(), &line_split(&replace_n)));
    }
    if hits2.len() > 1 {
        return Err(AnchorError::Ambiguous { count: hits2.len() });
    }

    // Уровень 3: relative indentation — тела строк равны, сдвиг отступа
    // одинаков по всем строкам фрагмента.
    let mut hits3 = Vec::new();
    for start in 0..=(lines.len() - search_lines.len()) {
        if rel_indent_window_match(&lines, &search_lines, start) {
            hits3.push(start);
        }
    }
    if hits3.len() == 1 {
        let start = hits3[0];
        let delta = indent_len(lines[start]) as isize - indent_len(search_lines[0]) as isize;
        let shifted = shift_lines(&replace_n, delta);
        return Ok(splice_window(&lines, start, search_lines.len(), &line_split(&shifted)));
    }
    if hits3.len() > 1 {
        return Err(AnchorError::Ambiguous { count: hits3.len() });
    }

    Err(AnchorError::NotFound)
}

/// Все позиции вхождений `needle` в `hay`.
fn find_all(hay: &str, needle: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(slice) = hay.get(from..) {
        let Some(pos) = slice.find(needle) else { break };
        out.push(from + pos);
        from += pos + 1;
    }
    out
}

/// Окно уровня 3: тела (без ведущего отступа) равны построчно И
/// дельта отступа одинакова для всех строк фрагмента.
fn rel_indent_window_match(lines: &[&str], search_lines: &[&str], start: usize) -> bool {
    let first_delta =
        indent_len(lines[start]) as isize - indent_len(search_lines[0]) as isize;
    search_lines
        .iter()
        .enumerate()
        .all(|(i, sl)| {
            let cl = lines[start + i];
            cl.trim_start() == sl.trim_start()
                && indent_len(cl) as isize - indent_len(sl) as isize == first_delta
        })
}

fn indent_len(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ' || *c == '\t').count()
}

/// Заменить в строке `content` байт-диапазон на `replacement`.
fn splice_str(content: &str, start: usize, len: usize, replacement: &str) -> String {
    let mut out = String::with_capacity(content.len() + replacement.len());
    out.push_str(&content[..start]);
    out.push_str(replacement);
    out.push_str(&content[start + len..]);
    out
}

/// Сплайс по строкам: заменить окно `start..start+len` на `replace_lines`.
/// Хвостовой перевод строки результата дают хвостовые элементы `lines`
/// (split сохраняет завершающий `""`), поэтому замена берётся без своего
/// хвостового `\n` (иначе он превращается в паразитную пустую строку).
fn splice_window(lines: &[&str], start: usize, len: usize, replace_lines: &[&str]) -> String {
    let mut out_lines: Vec<&str> = Vec::with_capacity(lines.len() + replace_lines.len());
    out_lines.extend(&lines[..start]);
    out_lines.extend(replace_lines.iter().copied());
    out_lines.extend(&lines[start + len..]);
    out_lines.join("\n")
}

/// split('\n') с отбрасыванием ОДНОГО хвостового пустого элемента: хвостовой
/// `\n` в SEARCH/REPLACE — граница строки, а не пустая строка после неё.
/// Пустая строка в середине (двойной `\n`) сохраняется как элемент.
fn line_split(s: &str) -> Vec<&str> {
    let mut v: Vec<&str> = s.split('\n').collect();
    if v.len() > 1 && v.last().is_some_and(|l| l.is_empty()) {
        v.pop();
    }
    v
}

/// Сдвиг отступа строк замены: положительный `delta` — добавить пробелы,
/// отрицательный — снять ведущие символы отступа.
fn shift_lines(replace: &str, delta: isize) -> String {
    if delta == 0 {
        return replace.to_string();
    }
    replace
        .split('\n')
        .map(|line| {
            if line.is_empty() {
                return line.to_string();
            }
            if delta > 0 {
                format!("{}{}", " ".repeat(delta as usize), line)
            } else {
                let cut = (-delta as usize).min(indent_len(line));
                line.chars().skip(cut).collect()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
