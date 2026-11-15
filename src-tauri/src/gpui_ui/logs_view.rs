//! Вкладка «Логи» GPUI-фронтенда: хвост vibebridge.log (последние 400 строк)
//! с автообновлением из drain-цикла RootView (~раз в секунду при открытой
//! вкладке) + кнопки плагина (паритет веб-компонентов tauri-plugin-logs:
//! Копировать / Сохранить / Очистить) и «Обновить». Шапка показывает путь
//! лог-файла (паритет egui LogView).
//!
//! «Копировать» — буфер обмена через arboard (уже в зависимостях);
//! «Сохранить» — экспорт хвоста в файл рядом с exe (модальный диалог сохранения
//! в GPUI listener недоступен — блокирующий rfd запрещён, урок краша 15.11.26);
//! «Очистить» — обнуление лог-файла (плагин пишет append-ом, трнкейт безопасен).

use gpui_kit::component::button::Button;
use gpui_kit::component::{ActiveTheme, h_flex};
use gpui_kit::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, div, px,
};

use super::root_view::RootView;

/// Сколько последних строк лога показываем (паритет с egui logs_view.rs).
const LOG_TAIL_LINES: usize = 400;

/// Свежий хвост лога для RootView (drain-цикл и кнопка «Обновить»).
pub fn refresh_tail() -> String {
    tauri_plugin_logs::logger::read_tail(LOG_TAIL_LINES)
}

/// Содержимое вкладки «Логи»: карточка с шапкой, кнопками и monospace-блоком.
pub fn render(root: &mut RootView, cx: &mut Context<RootView>) -> AnyElement {
    // Путь лог-файла — лениво при первом открытии вкладки (паритет egui LogView).
    if root.logs_path.is_none() {
        root.logs_path = tauri_plugin_logs::logger::log_file_path();
    }
    // Первый показ вкладки: хвост пуст — читаем сразу, не ждать тик ~1 с.
    if root.logs_tail.is_empty() {
        root.logs_tail = refresh_tail();
    }
    let path_text = root
        .logs_path
        .as_deref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "файл лога не найден (лог пишется в память/консоль)".to_string());

    let copy = Button::new("logs_copy").label("Копировать").on_click(
        cx.listener(|root: &mut RootView, _, _, _| {
            match arboard::Clipboard::new() {
                Ok(mut clipboard) => match clipboard.set_text(root.logs_tail.clone()) {
                    Ok(()) => log::info!(
                        "логи скопированы в буфер обмена ({} строк)",
                        root.logs_tail.lines().count()
                    ),
                    Err(e) => log::error!("буфер обмена недоступен: {e}"),
                },
                Err(e) => log::error!("буфер обмена недоступен: {e}"),
            }
        }),
    );

    let save = Button::new("logs_save").label("Сохранить…").on_click(
        cx.listener(|root: &mut RootView, _, _, _| {
            // Экспорт в папку exe с timestamp (плагинный «Сохранить» открывает
            // диалог, в GPUI-листенере блокирующие диалоги запрещены).
            let name = format!("log_export_{}.log", tauri_plugin_logs::logger::timestamp());
            let path = root.base.join(&name);
            match std::fs::write(&path, &root.logs_tail) {
                Ok(()) => log::info!("логи сохранены: {}", path.display()),
                Err(e) => log::error!("логи не сохранены: {e}"),
            }
        }),
    );

    let clear = Button::new("logs_clear").label("Очистить").on_click(
        cx.listener(|root: &mut RootView, _, _, cx| {
            match root.logs_path.clone() {
                Some(path) => match std::fs::write(&path, "") {
                    // Плагин пишет append-ом на каждый вызов — трнкейт безопасен.
                    Ok(()) => {
                        log::info!("лог-файл очищен владельцем");
                        root.logs_tail = refresh_tail();
                        cx.notify();
                    }
                    Err(e) => log::error!("лог-файл не очищен: {e}"),
                },
                None => log::error!("очистка невозможна: путь лог-файла неизвестен"),
            }
        }),
    );

    let refresh = Button::new("logs_refresh").label("Обновить").on_click(
        cx.listener(|root: &mut RootView, _, _, cx| {
            root.logs_tail = refresh_tail();
            cx.notify();
        }),
    );

    div()
        .id("logs_card")
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.0))
        .gap_2()
        .p_3()
        // Карточка — слой СВЕТЛЕЕ фона окна (план спеца №1).
        .bg(cx.theme().secondary)
        .border_1()
        .border_color(cx.theme().border)
        .rounded(px(6.0))
        .overflow_hidden()
        // Шапка: заголовок + путь лог-файла (паритет egui).
        .child(
            h_flex()
                .gap_3()
                .child(div().flex_shrink_0().child(SharedString::from(
                    "Логи приложения (последние 400 строк)",
                )))
                .child(
                    div()
                        .font_family("Consolas")
                        .text_size(px(12.0))
                        .text_color(cx.theme().muted_foreground)
                        .truncate()
                        .child(SharedString::from(path_text)),
                ),
        )
        // Кнопки: плагинный набор (Копировать/Сохранить/Очистить) + Обновить.
        .child(
            h_flex()
                .gap_2()
                .child(copy)
                .child(save)
                .child(clear)
                .child(refresh),
        )
        .child(div().h(px(1.0)).w_full().bg(cx.theme().border))
        // Текст лога: Consolas 12px, приглушённый, утопленный фон поля, скролл.
        .child(
            div()
                .id("logs_text")
                .flex_1()
                .min_h(px(0.0))
                .p_2()
                .bg(cx.theme().input)
                .rounded(px(6.0))
                .font_family("Consolas")
                .text_size(px(12.0))
                .text_color(cx.theme().muted_foreground)
                .overflow_y_scroll()
                .child(SharedString::from(root.logs_tail.clone())),
        )
        .into_any_element()
}
