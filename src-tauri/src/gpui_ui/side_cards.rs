//! Карточки левой колонки «Главной» GPUI (PRP 14.11.26 шаг 3):
//! «Чат → проект» (downlink, применение правок в фоновой задаче) и
//! «Черновик запроса» (промпт + вложения + AI-подбор). Логика —
//! downlink_state.rs / draft_state.rs; egui-двойники — ui/downlink.rs и
//! ui/draft_view.rs.

use std::path::Path;

use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::Textarea;
use gpui_kit::component::{ActiveTheme, Disableable, h_flex, button::{Button, ButtonVariants}};
use gpui_kit::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement as _, PathPromptOptions,
    SharedString, StatefulInteractiveElement as _, Styled as _, div, px,
};

use super::downlink_state::{DownlinkState, Report};
use super::draft_state::DraftState;
use super::root_view::RootView;

/// Карточка «Чат → проект»: поле ответа LLM + применение правок в фоне.
pub fn downlink_card(root: &mut RootView, cx: &mut Context<RootView>) -> AnyElement {
    let has_report = root
        .main
        .downlink
        .report()
        .is_some_and(Report::has_content);
    let can_undo = root
        .main
        .downlink
        .report()
        .is_some_and(|r| r.can_undo);
    let root_ready = root.main.uplink.root().is_some();
    let can_apply = root_ready && root.main.downlink.can_apply() && !root.main.downlink.applying;
    let applying = root.main.downlink.applying;

    // — применение правок: parse/prepare/apply в ФОНОВОЙ задаче gpui
    // (PRP-требование); отчёт доставляется через WeakEntity::update.
    // Зелёная кнопка (план спеца №2): успех-действие, а не синий primary.
    let apply_btn = Button::new("downlink_apply")
        .label(if applying { "Применение…" } else { "Применить изменения" })
        .success()
        .disabled(!can_apply)
        .on_click(cx.listener(|root, _, _, cx| {
            let Some(project_root) = root.main.uplink.root().map(Path::to_path_buf) else { return };
            let base = root.base.clone();
            let text = root.main.downlink.response.clone();
            root.main.downlink.applying = true;
            log::info!("downlink: применение правок (фоновая задача)");
            let bg = cx
                .background_executor()
                .spawn(async move { DownlinkState::compute_report(&project_root, &base, &text) });
            cx.spawn(async move |this, cx| {
                let rep = bg.await;
                if let Err(e) = this.update(cx, |root, cx| {
                    root.main.downlink.set_report(rep);
                    cx.notify();
                }) {
                    log::error!("downlink: отчёт не доставлен: {e}");
                }
            })
            .detach();
        }));

    let undo_btn = Button::new("downlink_undo")
        .label("Откатить последний патч")
        .disabled(!can_undo)
        .on_click(cx.listener(|root, _, _, cx| {
            let Some(project_root) = root.main.uplink.root().map(Path::to_path_buf) else { return };
            let base = root.base.clone();
            root.main.downlink.undo(&project_root, &base);
            cx.notify();
        }));

    let hint = (!root_ready)
        .then(|| div().text_color(cx.theme().muted_foreground).child(SharedString::from("сначала выбери папку проекта справа")));

    // — отчёт (скролл с потолком, паритет с egui draw_report)
    let mut report_parts: Vec<AnyElement> = Vec::new();
    if has_report {
        if let Some(rep) = root.main.downlink.report() {
            if let Some(status) = &rep.status {
                report_parts.push(
                    div().child(SharedString::from(status.clone())).into_any_element(),
                );
            }
            if !rep.parse_errors.is_empty() {
                report_parts.push(
                    div()
                        .text_color(cx.theme().danger)
                        .child(SharedString::from(format!("Ошибки разбора ({}):", rep.parse_errors.len())))
                        .into_any_element(),
                );
                for (path, reason) in &rep.parse_errors {
                    let who = path.as_deref().unwrap_or("(без пути)");
                    report_parts.push(
                        div()
                            .text_color(cx.theme().danger)
                            .child(SharedString::from(format!("{who}: {reason}")))
                            .into_any_element(),
                    );
                }
            }
            for w in &rep.parse_warnings {
                report_parts.push(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child(SharedString::from(format!("⚠ {w}")))
                        .into_any_element(),
                );
            }
            if !rep.applied.is_empty() {
                report_parts.push(
                    div()
                        .child(SharedString::from(format!("Применено ({}):", rep.applied.len())))
                        .into_any_element(),
                );
                for rel in &rep.applied {
                    report_parts.push(
                        div().child(SharedString::from(format!("✓ {rel}"))).into_any_element(),
                    );
                }
            }
            if !rep.failed.is_empty() {
                report_parts.push(
                    div()
                        .text_color(cx.theme().danger)
                        .child(SharedString::from(format!("Ошибки ({}):", rep.failed.len())))
                        .into_any_element(),
                );
                for (path, reason) in &rep.failed {
                    report_parts.push(
                        div()
                            .text_color(cx.theme().danger)
                            .child(SharedString::from(format!("✗ {path}: {reason}")))
                            .into_any_element(),
                    );
                }
            }
            if rep.backed_up > 0 {
                report_parts.push(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child(SharedString::from(format!("в бэкапе файлов: {} (Undo вернёт всё)", rep.backed_up)))
                        .into_any_element(),
                );
            }
            if rep.valid_pending.is_some() {
                let count = rep.valid_pending.as_ref().map_or(0, |p| p.len());
                let total = count + rep.failed.len();
                report_parts.push(
                    div()
                        .child(SharedString::from(format!("Валидных изменений: {count} из {total}")))
                        .into_any_element(),
                );
                // Кнопка добавляется после — замыканию нужен отдельный listener.
                report_parts.push(
                    Button::new("downlink_apply_valid")
                        .label("Применить только валидные")
                        .disabled(false)
                        .on_click(cx.listener(|root, _, _, cx| {
                            let Some(project_root) = root.main.uplink.root().map(Path::to_path_buf) else { return };
                            let base = root.base.clone();
                            let Some((prepared, warnings)) = root.main.downlink.take_valid_pending() else { return };
                            root.main.downlink.applying = true;
                            let bg = cx.background_executor().spawn(async move {
                                DownlinkState::apply_valid_report(&project_root, &base, prepared, warnings)
                            });
                            cx.spawn(async move |this, cx| {
                                let rep = bg.await;
                                if let Err(e) = this.update(cx, |root, cx| {
                                    root.main.downlink.set_report(rep);
                                    cx.notify();
                                }) {
                                    log::error!("downlink: отчёт не доставлен: {e}");
                                }
                            })
                            .detach();
                        }))
                        .into_any_element(),
                );
            }
        }
    }
    let report_block = (!report_parts.is_empty()).then(|| {
        div()
            .id("downlink_report")
            .flex()
            .flex_col()
            .gap_1()
            .max_h(px(150.0))
            .overflow_y_scroll()
            .children(report_parts)
            .into_any_element()
    });

    // — карточка
    div()
        .id("downlink_card")
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
        .child(div().child(SharedString::from("Чат → проект")))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h(px(0.0))
                .overflow_hidden()
                .child(Textarea::new(&root.main.response).h_full()),
        )
        .child(h_flex().gap_2().items_center().child(apply_btn).child(undo_btn).children(hint))
        .children(report_block)
        .into_any_element()
}

/// Карточка «Черновик запроса»: промпт + вложения + AI-подбор файлов.
pub fn draft_card(root: &mut RootView, cx: &mut Context<RootView>) -> AnyElement {
    let prompt_checked = root.main.draft.manifest().prompt_checked;
    let (busy, pick_enabled, pick_hint) = root
        .main
        .draft
        .pick_button_state(root.main.uplink.root(), root.main.uplink.scan_ready());
    let status = root.main.draft.status().map(str::to_string);

    // — тулбар
    let opt_prompt = Checkbox::new("draft_prompt_checked")
        .label("Промпт")
        .checked(prompt_checked)
        .on_click(cx.listener(|root, checked: &bool, _, cx| {
            root.main.draft.set_prompt_checked(*checked);
            cx.notify();
        }));
    let paste_img = Button::new("draft_paste_image")
        .label("Вставить картинку из буфера")
        .on_click(cx.listener(|root, _, _, cx| {
            root.main.draft.add_image_from_clipboard();
            cx.notify();
        }));
    // Асинхронный системный диалог (см. uplink_pick_dir): блокирующий rfd
    // внутри listener'а крашит приложение (ре-ентрантный borrow App).
    let add_files = Button::new("draft_add_files")
        .label("Добавить файлы…")
        .on_click(cx.listener(|_root, _, _, cx| {
            let rx = cx.prompt_for_paths(PathPromptOptions {
                files: true,
                directories: false,
                multiple: true,
                prompt: None,
            });
            let weak = cx.entity().downgrade();
            cx.spawn(async move |_this, cx| {
                let paths = match rx.await {
                    Ok(Ok(Some(paths))) => Some(paths),
                    Ok(Ok(None)) => None, // отмена
                    Ok(Err(e)) => {
                        log::error!("gpui: диалог выбора файлов: {e}");
                        None
                    }
                    Err(e) => {
                        log::error!("gpui: канал диалога закрыт: {e}");
                        None
                    }
                };
                let Some(paths) = paths else { return };
                if let Err(e) = weak.update(cx, |root, cx| {
                    root.main.draft.add_files_from(paths);
                    cx.notify();
                }) {
                    log::error!("gpui: выбранные файлы не доставлены: {e}");
                }
            })
            .detach();
        }));
    let copy_images = Button::new("draft_copy_images")
        .label("Скопировать картинки (файлы)")
        .on_click(cx.listener(|root, _, _, cx| {
            root.main.draft.copy_images();
            cx.notify();
        }));

    // — AI-подбор файлов: «✨ Подобрать» / «✖ Отменить», disabled с подсказкой
    let pick = Button::new("draft_ai_pick")
        .label(if busy { "✖ Отменить подбор" } else { "✨ Подобрать файлы" })
        .disabled(!busy && !pick_enabled)
        .on_click(cx.listener(move |root, _, _, cx| {
            let tx = root.main.cmd_tx().clone();
            let paths = root.main.uplink.selectable_relative_paths();
            root.main.draft.pick_or_cancel(&tx, root.main.uplink.root(), &paths);
            cx.notify();
        }));
    let pick_hint_line = (!busy && !pick_hint.is_empty()).then(|| {
        div()
            .text_color(cx.theme().muted_foreground)
            .child(SharedString::from(pick_hint.clone()))
    });

    // — вложения (доменные id от file_name; галочка — войдёт в копирование)
    let mut att_rows: Vec<AnyElement> = Vec::new();
    for att in &root.main.draft.manifest().attachments {
        let name = att.file_name.clone();
        let glyph = if DraftState::is_image(&name) { "🖼" } else { "📄" };
        // Каждое замыкание забирает свою копию имени (move-семантика).
        let name_cb = name.clone();
        let name_rm = name.clone();
        att_rows.push(
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    Checkbox::new(SharedString::from(format!("att_cb_{name}")))
                        .checked(att.checked)
                        .on_click(cx.listener(move |root, checked: &bool, _, cx| {
                            root.main.draft.set_attachment_checked(&name_cb, *checked);
                            cx.notify();
                        })),
                )
                .child(SharedString::from(format!("{glyph} {name}")))
                .child(
                    Button::new(SharedString::from(format!("att_rm_{name}")))
                        .label("✖")
                        .on_click(cx.listener(move |root, _, _, cx| {
                            root.main.draft.remove_attachment_by_name(&name_rm);
                            cx.notify();
                        })),
                )
                .into_any_element(),
        );
    }
    let empty_note = att_rows.is_empty();
    let attachments_block = div()
        .id("draft_attachments")
        .flex()
        .flex_col()
        .gap_1()
        .max_h(px(160.0))
        .overflow_y_scroll()
        .children(att_rows);

    // — карточка
    div()
        .id("draft_card")
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
        .child(div().child(SharedString::from("Черновик запроса")))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(h_flex().gap_3().items_center().child(opt_prompt).child(paste_img).child(add_files))
                .child(h_flex().gap_3().items_center().child(copy_images).child(pick))
                .children(pick_hint_line),
        )
        .child(Textarea::new(&root.main.prompt).h(px(150.0)))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child(SharedString::from(
                            "Вложения (галочка — войдёт в копирование, ✖ — убрать):",
                        )),
                )
                .children(if empty_note {
                    Some(
                        div()
                            .text_color(cx.theme().muted_foreground)
                            .child(SharedString::from("— вложений нет —"))
                            .into_any_element(),
                    )
                } else {
                    None
                })
                .child(attachments_block),
        )
        .children(
            status
                .map(|s| div().text_color(cx.theme().muted_foreground).child(SharedString::from(s))),
        )
        .into_any_element()
}
