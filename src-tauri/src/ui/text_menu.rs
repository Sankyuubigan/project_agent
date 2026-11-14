//! Контекстное меню ПКМ для текстовых полей: Вставить / Копировать / Выделить всё.
//!
//! В egui 0.32 у TextEdit встроенного контекстного меню нет. Вставка реализована
//! той же механикой, что у хост-плагина на Ctrl+V: `Event::Paste` пушится в
//! очередь событий и обрабатывается TextEdit в следующем кадре (вставка в позицию
//! курсора штатной логикой egui). ПКМ-клик фокус полю не даёт, поэтому перед
//! пушем события поле фокусируется явно — иначе вставка ушла бы в чужое поле.

use tauri_plugin_egui::egui;

use crate::clipboard;

/// Навесить контекстное меню ПКМ на ответ текстового поля.
///
/// `text` — текущее содержимое поля: нужно для «Копировать» (выделение хранится
/// в egui-state как посимвольные индексы) и «Выделить всё».
pub(super) fn attach(response: &egui::Response, text: &str) {
    response.context_menu(|ui| {
        let ctx = ui.ctx().clone();
        if ui.button("Вставить").clicked() {
            paste_at_cursor(&ctx, response.id);
            ui.close();
        }
        // Выделение читается из egui-state поля: пустое — пункт серый.
        let selected = selected_text(&ctx, response.id, text);
        if ui
            .add_enabled(selected.is_some(), egui::Button::new("Копировать"))
            .clicked()
        {
            if let Some(sel) = selected {
                let n = sel.chars().count();
                match clipboard::copy_text(&sel) {
                    Ok(()) => log::debug!("меню: скопировано {n} символов"),
                    Err(e) => log::warn!("меню: копирование: {e}"),
                }
            }
            ui.close();
        }
        if ui
            .add_enabled(!text.is_empty(), egui::Button::new("Выделить всё"))
            .clicked()
        {
            select_all(&ctx, response.id, text);
            ui.close();
        }
    });
}

/// Прочитать буфер и закинуть вставку в очередь событий egui (кадр N+1).
/// Чтение буфера — по клику пункта, не при рисовании меню (меню рисуется каждый кадр).
fn paste_at_cursor(ctx: &egui::Context, id: egui::Id) {
    match clipboard::read_text() {
        Ok(text) if !text.is_empty() => {
            // Размер считаем ДО пуша: текст уезжает в Event::Paste по значению.
            let n = text.chars().count();
            ctx.memory_mut(|m| m.request_focus(id));
            ctx.input_mut(|i| i.raw.events.push(egui::Event::Paste(text)));
            log::debug!("меню: вставка {n} символов");
        }
        Ok(_) => log::debug!("меню: вставка: буфер пуст"),
        Err(e) => log::warn!("меню: вставка: {e}"),
    }
}

/// Выделенный текст поля (None, если выделения нет).
/// Индексы CCursor — посимвольные, поэтому слайс через chars().
fn selected_text(ctx: &egui::Context, id: egui::Id, text: &str) -> Option<String> {
    let state = egui::TextEdit::load_state(ctx, id)?;
    let range = state.cursor.char_range()?.as_sorted_char_range();
    if range.is_empty() {
        return None;
    }
    Some(
        text.chars()
            .skip(range.start)
            .take(range.end - range.start)
            .collect(),
    )
}

/// Выделить весь текст поля (индексы CCursor — посимвольные).
fn select_all(ctx: &egui::Context, id: egui::Id, text: &str) {
    let Some(mut state) = egui::TextEdit::load_state(ctx, id) else {
        return;
    };
    state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
        egui::text::CCursor::new(0),
        egui::text::CCursor::new(text.chars().count()),
    )));
    egui::TextEdit::store_state(ctx, id, state);
}
