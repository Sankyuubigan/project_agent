//! Вкладка «Настройки» GPUI-фронтенда: паритет egui `ui/settings_view.rs` —
//! 4 карточки: «Движок интерфейса», «Плагин llama-engine», «Модель для
//! AI-подбора», «Карта проекта (тул codemap)». Конфиг — read-modify-write
//! через `agent_core::config`; рестарт — `crate::restart`; диалоги выбора
//! файлов — асинхронный `prompt_for_paths` (блокирующий rfd в listener'е
//! запрещён — урок краша 15.11.26), результат — по oneshot в WeakEntity.

use std::path::Path;

use agent_core::config::{self, AppConfig, UiBackend};
use agent_core::tree::repomap::DEFAULT_TOOL;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::radio::RadioGroup;
use gpui_kit::component::searchable_list::SearchableListItem;
use gpui_kit::component::select::{Select, SelectState};
use gpui_kit::component::{ActiveTheme, IndexPath, h_flex};
use gpui_kit::{
    AnyElement, AppContext as _, Context, Div, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, PathPromptOptions, SharedString, StatefulInteractiveElement as _,
    Styled as _, WeakEntity, Window, div, px,
};
use tauri_plugin_llama_engine::engine::config::EngineConfig;

use super::root_view::RootView;
use crate::restart;

/// Стейт настроек: конфиг приложения + конфиг плагина llama-engine + entity
/// выпадающего списка моделей (Select/SelectState из gpui-kit).
pub struct SettingsState {
    pub cfg: AppConfig,
    pub engine: Option<EngineConfig>,
    /// Entity создаётся при старте окна (нужен Window — init_select);
    /// выбор модели приходит событием Confirm (подписка в RootView::new).
    pub select: Option<Entity<SelectState<Vec<ModelItem>>>>,
}

impl SettingsState {
    pub fn load(base: &Path) -> Self {
        Self { cfg: config::load(base), engine: None, select: None }
    }

    /// Перечитать конфиг плагина (модели могли поменяться извне).
    pub fn refresh_engine(&mut self) {
        log::info!("настройки: перечитываю конфиг плагина llama-engine");
        self.engine = Some(tauri_plugin_llama_engine::engine::config::load_config_early());
    }

    /// Сохранить поле конфига: свежий read-modify-write с диска (паритет
    /// egui `persist` — между открытием окна и правкой конфиг могли менять).
    pub fn persist(&mut self, base: &Path, apply: impl FnOnce(&mut AppConfig)) {
        let mut fresh = config::load(base);
        apply(&mut fresh);
        match config::save(base, &fresh) {
            Ok(()) => self.cfg = fresh,
            Err(e) => log::error!("настройки: конфиг не сохранён: {e}"),
        }
    }

    /// Создать entity Select для списка моделей (RootView::new; SelectState::new
    /// требует Window).
    pub fn init_select(&mut self, window: &mut Window, cx: &mut Context<RootView>) {
        if self.select.is_some() {
            return;
        }
        let items = build_model_items(&self.engine);
        let selected = selected_model_index(&items, self.cfg.ai_model_path.as_deref());
        self.select = Some(cx.new(|cx| SelectState::new(items, selected, window, cx)));
    }

    /// Синхронизировать Select с текущим списком моделей/конфигом
    /// («↻ Перечитать», «Сбросить»).
    pub fn sync_select(&mut self, window: &mut Window, cx: &mut Context<RootView>) {
        let Some(select) = &self.select else { return };
        let items = build_model_items(&self.engine);
        let selected = selected_model_index(&items, self.cfg.ai_model_path.as_deref());
        select.update(cx, |s, cx| {
            s.set_items(items, window, cx);
            s.set_selected_index(selected, window, cx);
        });
    }
}

/// Асинхронный диалог выбора ОДНОГО файла (паритет egui rfd, но без
/// блокировки App — краш-урок): результат доставляет WeakEntity::update.
fn pick_file(
    cx: &Context<RootView>,
    err_ctx: &str,
    then: impl Fn(&mut RootView, String) + 'static,
) {
    let rx = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: None,
    });
    let weak: WeakEntity<RootView> = cx.entity().downgrade();
    let err_ctx = err_ctx.to_string();
    cx.spawn(async move |_this, cx| {
        let file = match rx.await {
            Ok(Ok(Some(mut paths))) => paths.pop(),
            Ok(Ok(None)) => None, // отмена
            Ok(Err(e)) => {
                log::error!("gpui: диалог ({err_ctx}): {e}");
                None
            }
            Err(e) => {
                log::error!("gpui: канал диалога ({err_ctx}) закрыт: {e}");
                None
            }
        };
        let Some(file) = file else { return };
        if let Err(e) = weak.update(cx, |root, cx| {
            then(root, file.display().to_string());
            cx.notify();
        }) {
            log::error!("gpui: выбор ({err_ctx}) не доставлен: {e}");
        }
    })
    .detach();
}

/// Рамка карточки (паритет egui style::card_frame): заголовок + контент.
fn card(title: &str, cx: &Context<RootView>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        // Карточка — слой СВЕТЛЕЕ фона окна (план спеца №1).
        .bg(cx.theme().secondary)
        .border_1()
        .border_color(cx.theme().border)
        .rounded(px(6.0))
        .child(div().child(SharedString::from(title.to_string())))
}

/// Строка «ключ: значение» (паритет egui kv_row): ключ приглушённый,
/// значение — Consolas с обрезкой.
fn kv_row(cx: &Context<RootView>, key: &str, value: &str) -> Div {
    h_flex()
        .gap_2()
        .child(
            div()
                .flex_shrink_0()
                .text_color(cx.theme().muted_foreground)
                .child(SharedString::from(key.to_string())),
        )
        .child(
            div()
                .font_family("Consolas")
                .text_size(px(12.0))
                .truncate()
                .child(SharedString::from(value.to_string())),
        )
}

/// Имя файла из пути (для списка моделей) — паритет egui file_label.
pub fn file_label(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

/// Элемент выпадающего списка моделей: value — путь модели (пустая строка =
/// «Первая доступная», ai_model_path = None).
#[derive(Clone)]
pub struct ModelItem {
    label: String,
    path: String,
    missing: bool,
}

impl SearchableListItem for ModelItem {
    type Value = String;

    fn title(&self) -> SharedString {
        SharedString::from(self.label.clone())
    }

    fn value(&self) -> &String {
        &self.path
    }

    /// Отсутствующий на диске GGUF — серым, применить нельзя (паритет egui:
    /// «файл не найден → warn, не сохранять»).
    fn disabled(&self) -> bool {
        self.missing
    }
}

/// Список элементов Select: [0] «Первая доступная», далее модели плагина
/// (отсутствующие на диске — disabled с меткой, паритет egui-меток).
fn build_model_items(engine: &Option<EngineConfig>) -> Vec<ModelItem> {
    let mut items = vec![ModelItem {
        label: "Первая доступная".into(),
        path: String::new(),
        missing: false,
    }];
    if let Some(engine) = engine {
        for m in &engine.models {
            let (label, missing) = if Path::new(m).is_file() {
                (file_label(m), false)
            } else {
                (format!("{} — файл не найден", file_label(m)), true)
            };
            items.push(ModelItem { label, path: m.clone(), missing });
        }
    }
    items
}

/// IndexPath выбранного элемента Select (None в конфиге → «Первая доступная»).
fn selected_model_index(items: &[ModelItem], current: Option<&str>) -> Option<IndexPath> {
    let ix = match current {
        None => 0,
        Some(p) => items.iter().position(|i| i.path == p).unwrap_or(0),
    };
    Some(IndexPath::default().row(ix))
}

/// Содержимое вкладки: карточки в скролл-колонке (паритет egui draw). Контент
/// не растягивается на всю ширину широких окон (план спеца №3, max 920).
pub fn render(root: &mut RootView, cx: &mut Context<RootView>) -> AnyElement {
    div()
        .id("settings_scroll")
        .flex()
        .flex_col()
        .flex_1()
        .gap_3()
        .p_4()
        .max_w(px(920.0))
        .overflow_y_scroll()
        .child(backend_card(root, cx))
        .child(engine_card(root, cx))
        .child(model_card(root, cx))
        .child(repomap_card(root, cx))
        .into_any_element()
}

/// Карточка «Движок интерфейса»: радио egui/GPUI + сохранение и рестарт.
fn backend_card(root: &mut RootView, cx: &Context<RootView>) -> Div {
    let selected = match root.backend_selection {
        UiBackend::Egui => 0,
        UiBackend::Gpui => 1,
    };
    card("Движок интерфейса", cx)
        .child(
            // ID доменный: единственный переключатель движка в GPUI.
            RadioGroup::horizontal("ui_backend_radio")
                .children(["egui (Текущий)", "GPUI (Новый)"])
                .selected_index(Some(selected))
                .on_change(cx.listener(|root: &mut RootView, idx: &usize, _, cx| {
                    root.backend_selection = match *idx {
                        0 => UiBackend::Egui,
                        _ => UiBackend::Gpui,
                    };
                    log::info!("GPUI: выбран движок интерфейса — {}", root.backend_selection);
                    cx.notify();
                })),
        )
        .child(
            Button::new("btn_save_restart")
                .label("Сохранить и перезапустить")
                .primary()
                .on_click(cx.listener(|root: &mut RootView, _, _, cx| {
                    let backend = root.backend_selection;
                    let mut cfg = config::load(&root.base);
                    cfg.ui_backend = backend;
                    if let Err(e) = config::save(&root.base, &cfg) {
                        log::error!("GPUI: конфиг не сохранён: {e}");
                        return;
                    }
                    log::info!("GPUI: конфиг сохранён, перезапуск в ветку {backend}");
                    if restart::relaunch() {
                        cx.quit();
                    } else {
                        log::error!("GPUI: перезапуск не состоялся — окно остаётся открытым");
                    }
                })),
        )
}

/// Карточка «Плагин llama-engine»: параметры движка + счётчик моделей.
fn engine_card(root: &mut RootView, cx: &Context<RootView>) -> Div {
    let mut card = card("Плагин llama-engine (движок локальных моделей)", cx);
    match root.settings.engine.as_ref() {
        None => {
            card = card.child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child("— конфиг плагина не прочитан —"),
            );
        }
        Some(engine) => {
            card = card
                .child(kv_row(
                    cx,
                    "Движок (llamacpp):",
                    engine.llamacpp_dir.as_deref().unwrap_or("— не задан —"),
                ))
                .child(kv_row(
                    cx,
                    "Каталог моделей:",
                    engine.models_dir.as_deref().unwrap_or("— не задан —"),
                ))
                .child(kv_row(
                    cx,
                    "Источник:",
                    engine.engine_source.as_deref().unwrap_or("ggml-org (дефолт)"),
                ))
                .child(kv_row(
                    cx,
                    "Вариант:",
                    engine.engine_variant.as_deref().unwrap_or("авто"),
                ));
            let existing = engine.models.iter().filter(|m| Path::new(m).is_file()).count();
            card = card.child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(SharedString::from(format!(
                        "Моделей в списке: {} (файлов на диске: {})",
                        engine.models.len(),
                        existing
                    ))),
            );
            if engine.models.is_empty() {
                card = card.child(
                    div()
                        .text_color(cx.theme().danger)
                        .child("Список моделей пуст — добавь модели в плагин (через его основное приложение)."),
                );
            }
            card = card.child(
                Button::new("engine_refresh")
                    .label("↻ Перечитать")
                    .on_click(cx.listener(|root: &mut RootView, _, window, cx| {
                        root.settings.refresh_engine();
                        // Список моделей в Select мог измениться — синхрон.
                        root.settings.sync_select(window, cx);
                        cx.notify();
                    })),
            );
        }
    }
    card
}

/// Карточка «Модель для AI-подбора»: компактный выпадающий список моделей
/// (Select из gpui-kit — паритет egui ComboBox) + ручной GGUF + сброс.
fn model_card(root: &mut RootView, cx: &mut Context<RootView>) -> Div {
    let mut card = card("Модель для AI-подбора файлов", cx);
    if root.settings.engine.is_none() {
        return card.child(
            div()
                .text_color(cx.theme().muted_foreground)
                .child("— конфиг плагина не прочитан —"),
        );
    }

    // Выпадающий список моделей: выбор доставляется событием Confirm
    // (подписка в RootView::new) — persist делает она.
    if let Some(select) = root.settings.select.as_ref() {
        card = card.child(
            Select::new(select)
                .id("ai_model_select")
                .menu_width(px(420.0)),
        );
    }

    let mut row = h_flex().gap_2().child(
        Button::new("model_pick_gguf").label("Другой GGUF…").on_click(
            cx.listener(|_root: &mut RootView, _, _, cx| {
                pick_file(cx, "выбор модели", |root, path| {
                    log::info!("настройки: модель указана вручную: {path}");
                    root.settings.persist(&root.base, |cfg| cfg.ai_model_path = Some(path));
                });
            }),
        ),
    );
    if root.settings.cfg.ai_model_path.is_some() {
        row = row.child(
            Button::new("model_reset").label("Сбросить («первая доступная»)").on_click(
                cx.listener(|root: &mut RootView, _, window, cx| {
                    root.settings.persist(&root.base, |cfg| cfg.ai_model_path = None);
                    root.settings.sync_select(window, cx);
                    cx.notify();
                }),
            ),
        );
        let current = root.settings.cfg.ai_model_path.clone().unwrap_or_default();
        card = card.child(kv_row(cx, "Текущий ai_model_path:", &current));
    }
    card = card
        .child(row)
        .child(
            div()
                .text_color(cx.theme().muted_foreground)
                .child("Используется кнопкой «✨ Подобрать файлы» в черновике; остальное (params) — из конфига плагина."),
        );
    card
}

/// Карточка «Карта проекта (тул codemap)»: путь тул-бинаря + выбор/сброс.
fn repomap_card(root: &mut RootView, cx: &mut Context<RootView>) -> Div {
    let tool = root
        .settings
        .cfg
        .repomap_tool
        .clone()
        .unwrap_or_else(|| DEFAULT_TOOL.to_string());
    let mut card = card("Карта проекта (тул codemap)", cx)
        .child(kv_row(cx, "Тул-бинарь:", &tool));
    if !Path::new(&tool).is_file() {
        card = card.child(
            div()
                .text_color(cx.theme().danger)
                .child("⚠ файл не найден — AI-подбор не сможет строить карту проекта"),
        );
    }
    let mut row = h_flex().gap_2().child(
        Button::new("repomap_pick_exe").label("Выбрать exe…").on_click(
            cx.listener(|_root: &mut RootView, _, _, cx| {
                pick_file(cx, "выбор тул-бинаря", |root, path| {
                    log::info!("настройки: тул-бинарь карты проекта: {path}");
                    root.settings.persist(&root.base, |cfg| cfg.repomap_tool = Some(path));
                });
            }),
        ),
    );
    if root.settings.cfg.repomap_tool.is_some() {
        row = row.child(
            Button::new("repomap_reset").label("Сбросить на дефолт").on_click(
                cx.listener(|root: &mut RootView, _, _, cx| {
                    root.settings.persist(&root.base, |cfg| cfg.repomap_tool = None);
                    cx.notify();
                }),
            ),
        );
    }
    card.child(row).child(
        div()
            .text_color(cx.theme().muted_foreground)
            .child("agent-tools.exe codemap map --no-cache --tokens 2048 — скелет проекта для модели."),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// build_items: [0] — «Первая доступная» с пустым value (ai_model_path=None).
    #[test]
    fn model_items_first_entry_is_always_available() {
        let items = build_model_items(&None);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "Первая доступная");
        assert_eq!(items[0].path, "");
        assert!(!items[0].missing);
    }

    /// Модель без файла на диске — disabled с меткой (паритет egui).
    #[test]
    fn model_items_marks_missing_files() {
        let engine = Some(EngineConfig {
            models: vec![r"D:\точно\нет\missing.gguf".to_string()],
            ..Default::default()
        });
        let items = build_model_items(&engine);
        assert_eq!(items.len(), 2);
        assert!(items[1].missing);
        assert!(items[1].disabled());
        assert!(items[1].label.contains("файл не найден"));
        assert_eq!(items[1].path, r"D:\точно\нет\missing.gguf");
    }

    /// IndexPath выбранного: None → 0 («Первая доступная»), Some → позиция
    /// модели; неизвестный путь — фолбэк «Первая доступная».
    #[test]
    fn selected_index_maps_config_to_row() {
        let items = vec![
            ModelItem {
                label: "Первая доступная".into(),
                path: String::new(),
                missing: false,
            },
            ModelItem { label: "a.gguf".into(), path: "D:/a.gguf".into(), missing: false },
        ];
        let row = |ix: Option<IndexPath>| ix.map(|p| p.row);
        assert_eq!(row(selected_model_index(&items, None)), Some(0));
        assert_eq!(row(selected_model_index(&items, Some("D:/a.gguf"))), Some(1));
        assert_eq!(row(selected_model_index(&items, Some("X:/b.gguf"))), Some(0));
    }
}
