//! Вкладка «Главная» GPUI-фронтенда (PRP 14.11.26 шаг 3): слева «Чат → проект»
//! + «Черновик запроса» (side_cards.rs), справа «Проект → чат».
//!
//! Логика секций — MainState (main_state.rs) + соседние *_state.rs; egui-двойник
//! — ui/mod.rs::draw_main. Замыкание gpui tree() требует 'static — захватывает
//! Arc-снапшот строк (tree_items::RowData) и WeakEntity<RootView> для мутаций.

use std::path::Path;
use std::sync::Arc;

use my_gpui_kit::component::checkbox::Checkbox;
use my_gpui_kit::component::list::ListItem;
use my_gpui_kit::component::radio::RadioGroup;
use my_gpui_kit::component::resizable::{h_resizable, v_resizable};
use my_gpui_kit::component::tree::tree;
use my_gpui_kit::component::{ActiveTheme, button::{Button, ButtonVariants}, h_flex};
use my_gpui_kit::{
    AnyElement, App, AppCard, Context, ElementId, IntoElement, ParentElement as _,
    PathPromptOptions, SharedString, Styled as _, WeakEntity, div, px,
};

use super::root_view::RootView;
use super::side_cards;
use super::tree_items::{self, RowData};

/// Содержимое вкладки «Главная»: две колонки с ресайзом.
pub fn render(root: &mut RootView, cx: &mut Context<RootView>) -> AnyElement {
    // Снапшот строк дерева: замыкание tree() требует 'static — захватываем Arc,
    // собранный заново на каждом render (галочки/токены всегда актуальны).
    let rows = Arc::new(tree_items::build_rows(
        root.main.uplink.scan(),
        root.main.uplink.checked(),
        root.main.uplink.tokens(),
        root.main.uplink.expanded(),
    ));
    let weak = cx.entity().downgrade();

    let left = v_resizable("left_split")
        .child(side_cards::downlink_card(root, cx))
        .child(side_cards::draft_card(root, cx));
    let right = uplink_card(root, cx, rows, weak);

    h_resizable("main_split")
        .child(left)
        .child(right)
        .into_any_element()
}

/// Правая колонка «Проект → чат»: корневая папка, тулбар, опции, дерево,
/// кнопка «Собрать и скопировать».
fn uplink_card(
    root: &mut RootView,
    cx: &mut Context<RootView>,
    rows: Arc<Vec<RowData>>,
    weak: WeakEntity<RootView>,
) -> AnyElement {
    let total = root.main.uplink.total_tokens_text();
    let status = root.main.uplink.status().map(str::to_string);
    let path_text = root
        .main
        .uplink
        .root()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "папка не выбрана".to_string());

    // — корневая папка: выбрать / обновить
    // Асинхронный системный диалог (prompt_for_paths): блокирующий rfd внутри
    // listener'а ре-ентрантно конфликтует с borrow App gpui — краш приложения
    // (RefCell already borrowed → unwrap → abort). Результат — по oneshot-
    // каналу в async-задачу, доставку мутаций делает WeakEntity::update.
    let pick = Button::new("uplink_pick_dir")
        .label("Выбрать папку…")
        .on_click(cx.listener(|_root: &mut RootView, _, _, cx| {
            let rx = cx.prompt_for_paths(PathPromptOptions {
                files: false,
                directories: true,
                multiple: false,
                prompt: None,
            });
            let weak = cx.entity().downgrade();
            cx.spawn(async move |_this, cx| {
                let dir = match rx.await {
                    Ok(Ok(Some(mut paths))) => paths.pop(),
                    Ok(Ok(None)) => None, // отмена
                    Ok(Err(e)) => {
                        log::error!("gpui: диалог выбора папки: {e}");
                        None
                    }
                    Err(e) => {
                        log::error!("gpui: канал диалога закрыт: {e}");
                        None
                    }
                };
                let Some(dir) = dir else { return };
                if let Err(e) = weak.update(cx, |root, cx| {
                    let tx = root.main.cmd_tx().clone();
                    root.main.uplink.open_root(&dir, &root.base, &tx);
                    // Скан сброшен — дерево до заглушки (паритет с egui).
                    root.main.rebuild_tree_items(cx);
                    cx.notify();
                }) {
                    log::error!("gpui: выбор папки не доставлен: {e}");
                }
            })
            .detach();
        }));

    let refresh = Button::new("uplink_refresh")
        .label("Обновить")
        .on_click(cx.listener(|root: &mut RootView, _, _, cx| {
            if let Some(root_path) = root.main.uplink.root().map(Path::to_path_buf) {
                root.main.send_scan(root_path);
                cx.notify();
            }
        }));

    let path_line = div()
        .text_color(cx.theme().muted_foreground)
        .truncate()
        .child(SharedString::from(path_text));

    // — тулбар: отметки и токены
    let check_all = Button::new("uplink_check_all")
        .label("Отметить всё")
        .on_click(cx.listener(|root, _, _, cx| {
            root.main.uplink.check_all(true);
            cx.notify();
        }));
    let clear_all = Button::new("uplink_clear_all")
        .label("Снять всё")
        .on_click(cx.listener(|root, _, _, cx| {
            root.main.uplink.check_all(false);
            cx.notify();
        }));
    let tokens_btn = Button::new("uplink_tokens")
        .label("Посчитать токены")
        .on_click(cx.listener(|root, _, _, cx| {
            let tx = root.main.cmd_tx().clone();
            root.main.uplink.request_tokens(&tx);
            cx.notify();
        }));
    let total_label = div()
        .text_color(cx.theme().muted_foreground)
        .child(SharedString::from(total));

    // — опции промпта
    let opt_include = Checkbox::new("opt_include_selected")
        .label("Включить выбранные файлы")
        .checked(root.main.uplink.include_selected)
        .on_click(cx.listener(|root, checked: &bool, _, cx| {
            root.main.uplink.include_selected = *checked;
            cx.notify();
        }));

    let opt_structure = Checkbox::new("opt_structure")
        .label("Включить структуру")
        .checked(root.main.uplink.structure_checked)
        .on_click(cx.listener(|root, checked: &bool, _, cx| {
            root.main.uplink.structure_checked = *checked;
            cx.notify();
        }));

    let structure_radio = RadioGroup::horizontal("structure_mode")
        .children(["отмеченным", "всем файлам"])
        .selected_index(Some(if root.main.uplink.structure_all { 1 } else { 0 }))
        .on_change(cx.listener(|root, idx: &usize, _, cx| {
            root.main.uplink.structure_all = *idx == 1;
            cx.notify();
        }));

    let instruction_hint = root.main.uplink.instruction_hint();
    let opt_instruction = Checkbox::new("opt_instruction")
        .label("Включить инструкцию метода")
        .checked(root.main.uplink.instruction_checked)
        .on_click(cx.listener(|root, checked: &bool, _, cx| {
            let was = root.main.uplink.instruction_checked;
            root.main.uplink.instruction_checked = *checked;
            if *checked && !was {
                root.main.uplink.refresh_instruction(&root.base);
            }
            cx.notify();
        }));
    let instruction_line = instruction_hint
        .map(|(text, is_error)| {
            div()
                .text_color(if is_error { cx.theme().danger } else { cx.theme().muted_foreground })
                .child(SharedString::from(text))
        })
        .map(|el| el.into_any_element());

    // — дерево файлов (кастомные строки: чекбокс + имя + статус + токены)
    let tree_el = tree(&root.main.tree_state, move |ix, entry, _selected, _window, cx: &mut App| {
        let mut li = match rows.get(ix) {
            Some(row) => ListItem::new(SharedString::from(format!("row_{}", row.rel))),
            None => ListItem::new(ElementId::named_usize("row_fallback", ix)),
        };
        let Some(row) = rows.get(ix) else { return li };
        li = li.pl(px(16.0 * entry.depth() as f32 + 12.0));

        if row.is_dir {
            // Треугольник свернутости: ▾ раскрыта, ▸ свёрнута; пустая папка —
            // пустая клетка (раскрывать нечего). У файлов — пустая клетка той
            // же ширины (выравнивание колонки).
            let chevron = if row.has_children {
                div()
                    .w(px(14.0))
                    .justify_center()
                    .text_color(cx.theme().muted_foreground)
                    .child(SharedString::from(if row.expanded { "▾" } else { "▸" }))
            } else {
                div().w(px(14.0))
            };
            // Папка: чекбокс-каскад (gpui Checkbox бинарный; частично отмеченная
            // папка рисуется снятой — счётчик рядом показывает состояние).
            let label = if row.total > 0 {
                format!("{} ({}/{})", row.name, row.marked, row.total)
            } else {
                row.name.clone()
            };
            if row.selectable {
                let weak = weak.clone();
                let prefix = row.rel.clone();
                let cascade = Checkbox::new(SharedString::from(format!("cb_{}", row.rel)))
                    .checked(row.checked)
                    .on_click(move |checked: &bool, _w, cx: &mut App| {
                        // Не «всплывать» в строку: клик по чекбоксу не должен
                        // тогглить раскрытие папки.
                        cx.stop_propagation();
                        if let Err(e) = weak.update(cx, |root, cx| {
                            root.main.uplink.set_dir_checked(&prefix, *checked);
                            cx.notify();
                        }) {
                            log::error!("gpui tree: папка не обновлена: {e}");
                        }
                    });
                li = li.child(
                    h_flex()
                        .gap_2()
                        .child(chevron)
                        .child(cascade)
                        .child(SharedString::from(label)),
                );
            } else {
                li = li.child(h_flex().gap_2().child(chevron).child(SharedString::from(label)));
            }
            // Раскрытие — НАША логика на ListItem-контенте (библиотечный
            // toggle_expand приватен): тогглит set, пересобирает TreeState.
            // stop_propagation глушит библиотечный on_entry_click строки.
            if row.has_children {
                let weak = weak.clone();
                let dir_rel = row.rel.clone();
                li = li.on_click(move |_ev, _window, cx: &mut App| {
                    cx.stop_propagation();
                    if let Err(e) = weak.update(cx, |root, cx| {
                        root.main.uplink.toggle_expanded(&dir_rel);
                        root.main.rebuild_tree_items(cx);
                        cx.notify();
                    }) {
                        log::error!("gpui tree: папка не раскрыта: {e}");
                    }
                });
            }
        } else {
            // Файл: клетка под треугольник + чекбокс (или пустая клетка) +
            // имя + статус + токены.
            let mut line = h_flex().gap_2().child(div().w(px(14.0)));
            if row.selectable {
                let weak = weak.clone();
                let rel = row.rel.clone();
                let cb = Checkbox::new(SharedString::from(format!("cb_{}", row.rel)))
                    .checked(row.checked)
                    .on_click(move |checked: &bool, _w, cx: &mut App| {
                        // Как у папок: клик по чекбоксу не тогглит строку.
                        cx.stop_propagation();
                        if let Err(e) = weak.update(cx, |root, cx| {
                            root.main.uplink.set_file_checked(&rel, *checked);
                            cx.notify();
                        }) {
                            log::error!("gpui tree: файл не отмечен: {e}");
                        }
                    });
                line = line.child(cb);
            } else {
                line = line.child(div().w(px(18.0)));
            }
            line = line.child(SharedString::from(row.name.clone()));
            if !row.note.is_empty() {
                let note_color = if row.note_is_error {
                    cx.theme().danger
                } else {
                    cx.theme().muted_foreground
                };
                line = line.child(
                    div()
                        .text_color(note_color)
                        .child(SharedString::from(row.note)),
                );
            }
            if let Some(tokens) = &row.tokens {
                line = line.child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child(SharedString::from(tokens.clone())),
                );
            }
            li = li.child(line);
        }
        li
    });
    let tree_box = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.0))
        // Утопленный слой: фон полей ввода + рамка (план спеца №5).
        .p_2()
        .bg(cx.theme().input)
        .border_1()
        .border_color(cx.theme().border)
        .rounded(px(6.0))
        .overflow_hidden()
        .child(tree_el);

    // — сборка и копирование промпта
    let copy = Button::new("uplink_copy")
        .label("Собрать и скопировать")
        .primary()
        .on_click(cx.listener(|root, _, _, cx| {
            let base = root.base.clone();
            root.main.uplink.copy_prompt(&base, &root.main.draft);
            cx.notify();
        }));
    let status_line = status
        .map(|s| div().text_color(cx.theme().muted_foreground).child(SharedString::from(s)))
        .map(|el| el.into_any_element());

    // — карточка кита (bg/border/rounded/p_3 внутри; растяжение — refine).
    AppCard::new()
        .title("Проект → чат")
        .flex_1()
        .min_h(px(0.0))
        .overflow_hidden()
        .child(h_flex().gap_2().child(pick).child(refresh))
        .child(path_line)
        .child(div().h(px(1.0)).w_full().bg(cx.theme().border))
        .child(h_flex().gap_2().items_center().child(check_all).child(clear_all).child(tokens_btn).child(total_label))
        .child(tree_box)
        // Опции и «Собрать и скопировать» — ПОД деревом (паритет egui
        // uplink.rs:221: bottom-панель = separator → options → copy row).
        .child(div().h(px(1.0)).w_full().bg(cx.theme().border))
        // Опции в одну строку (план спеца №5: gap_2 + items_center; перенос
        // на узком окне — flex_wrap). «Собрать и скопировать» — ниже, отдельной строкой.
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .flex_wrap()
                .child(opt_include)
                .child(opt_structure)
                .child(structure_radio)
                .child(opt_instruction)
                .children(instruction_line),
        )
        .child(h_flex().gap_2().items_center().child(copy).children(status_line))
        .into_any_element()
}
