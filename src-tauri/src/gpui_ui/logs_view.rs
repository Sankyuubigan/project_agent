//! Вкладка «Логи» GPUI-фронтенда: [`LogsPanel`] кита (drain-режим — RootView
//! обновляет `logs_tail` ~раз в секунду в drain-цикле и отдаёт панель
//! `.tail(root.logs_tail)`; кнопка «Обновить» через `.on_refresh` делает
//! WeakEntity::update → перечитывает хвост и notify). Панель сама берёт путь
//! лог-файла из logs-core (логгер плагина и кита — один statics, SSOT).
//!
//! Кнопки панели (паритет веб-компонентов plugin-log-collector): Копировать /
//! Сохранить… / Очистить / Обновить — поведение внутри кита (arboard, экспорт
//! в папку exe, трнкейт файла).

use my_gpui_kit::{AnyElement, Context, IntoElement as _, WeakEntity};
use my_gpui_kit::LogsPanel;

use super::root_view::RootView;

/// Сколько последних строк лога показываем (паритет с egui logs_view.rs).
const LOG_TAIL_LINES: usize = 400;

/// Свежий хвост лога для RootView (drain-цикл и кнопка «Обновить»).
pub fn refresh_tail() -> String {
    plugin_log_collector::logger::read_tail(LOG_TAIL_LINES)
}

/// Содержимое вкладки «Логи»: панель логов кита в drain-режиме.
pub fn render(root: &mut RootView, cx: &mut Context<RootView>) -> AnyElement {
    // Первый показ вкладки: хвост пуст — читаем сразу, не ждать тик ~1 с.
    if root.logs_tail.is_empty() {
        root.logs_tail = refresh_tail();
    }
    let weak: WeakEntity<RootView> = cx.entity().downgrade();

    LogsPanel::new()
        .tail(root.logs_tail.clone())
        .on_refresh(move |_, cx| {
            let _ = weak.update(cx, |root, cx| {
                root.logs_tail = refresh_tail();
                cx.notify();
            });
        })
        .into_any_element()
}
