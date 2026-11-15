//! Корневой view GPUI-окна (PRP 14.11.26 шаги 2–3): шапка с заголовком и
//! переключателем темы ☀/🌙, TabBar вкладок «Главная/Логи/Настройки» и
//! содержимое активной вкладки. Вкладки — render-функции соседних модулей.
//!
//! RootView владеет MainState («Главная») и подписками (`_subscriptions`,
//! правило скилла gpui-kit): InputEvent::Change двух textarea («Черновик
//! запроса», «Чат → проект») и вечная drain-задача событий воркера
//! (`cx.spawn` + таймер, detach).

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use agent_core::config;
use gpui_kit::component::input::InputEvent;
use gpui_kit::component::select::SelectEvent;
use gpui_kit::component::tree::TreeEvent;
use gpui_kit::component::{ActiveTheme, button::{Button, ButtonVariants}, h_flex};
use gpui_kit::{
    Context, Div, InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString,
    Styled as _, Window, WindowControlArea, div, px,
};

use super::main_state::MainState;
use super::settings_view::{ModelItem, file_label};
use super::{logs_view, main_view, settings_view, theme};
use crate::worker::{Command, Event};

/// Вкладки верхней панели. Соответствие индексам TabBar — единый источник.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Main,
    Logs,
    Settings,
}

/// Корень GPUI-окна: стейт каркаса и данные вкладок.
pub struct RootView {
    /// Папка exe — общая база путей приложения (воркер, конфиг, вкладки).
    pub base: PathBuf,
    /// Активная вкладка верхней панели.
    tab: Tab,
    /// Тёмная тема (переключатель ☀/🌙; действует только в GPUI-окне).
    dark: bool,
    /// Рабочее состояние радио движка (применяется кнопкой
    /// «Сохранить и перезапустить»).
    pub backend_selection: config::UiBackend,
    /// Стейт «Главной»: uplink/черновик/downlink + entity'и Tree/textarea.
    pub main: MainState,
    /// Хвост vibebridge.log для вкладки «Логи» (последние 400 строк).
    pub logs_tail: String,
    /// Путь лог-файла (лениво, при первом открытии вкладки «Логи»).
    pub logs_path: Option<std::path::PathBuf>,
    /// Стейт настроек (конфиг приложения + конфиг плагина llama-engine).
    pub settings: settings_view::SettingsState,
    /// Все cx.subscribe — обязаны храниться (правило скилла gpui-kit).
    _subscriptions: Vec<gpui_kit::Subscription>,
}

impl RootView {
    pub fn new(
        base: PathBuf,
        cmd_tx: Sender<Command>,
        event_rx: Receiver<Event>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let backend_selection = config::load(&base).ui_backend;
        // Дефолт GPUI-окна — тёмная тема (решение владельца 15.11.26; egui
        // тоже стартует тёмным: ui/mod.rs `dark: true`). set_dark = change +
        // кастомная палитра 4 слоёв (SSOT egui visuals); change сбрасывает
        // colors на дефолт, поэтому палитра ставится только после него.
        theme::set_dark(cx);
        log::info!("GPUI: тема — тёмная палитра 4 слоёв (план спеца 15.11.26)");
        let dark = cx.theme().mode.is_dark();
        let main = MainState::new(&base, cmd_tx, event_rx, window, cx);
        // Настройки: конфиг + конфиг плагина llama-engine (модели для
        // AI-подбора) — читаются один раз на старте, «↻ Перечитать» во вкладке.
        let mut settings = settings_view::SettingsState::load(&base);
        settings.refresh_engine();
        // Выпадающий список модели (Select): entity — здесь (SelectState::new
        // требует Window), выбор доставляется событием Confirm.
        settings.init_select(window, cx);

        // Текст textarea → стейт секций (по факту изменения, не на каждый кадр).
        let prompt_entity = main.prompt.clone();
        let response_entity = main.response.clone();
        let tree_entity = main.tree_state.clone();
        let mut subscriptions = vec![
            cx.subscribe(&prompt_entity, |this: &mut Self, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let text = this.main.prompt.read(cx).value().as_ref().to_string();
                    this.main.draft.set_prompt(text);
                }
            }),
            cx.subscribe(&response_entity, |this: &mut Self, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let text = this.main.response.read(cx).value().as_ref().to_string();
                    this.main.downlink.response = text;
                }
            }),
        ];
        // Раскрытие из библиотечных путей (клик мимо ListItem-контента,
        // клавиатура): синхронизируем наш set и пересобираем дерево.
        subscriptions.push(cx.subscribe(
            &tree_entity,
            |this: &mut Self, _, event: &TreeEvent, cx| {
                let (id, opened) = match event {
                    TreeEvent::Expanded(id) => (id, true),
                    TreeEvent::Collapsed(id) => (id, false),
                };
                let Some(rel) = id.strip_prefix("dir_") else { return };
                if opened {
                    this.main.uplink.expanded.insert(rel.to_string());
                } else {
                    this.main.uplink.expanded.remove(rel);
                }
                log::info!(
                    "дерево (lib): {} {rel}",
                    if opened { "раскрыто" } else { "свёрнуто" }
                );
                this.main.rebuild_tree_items(cx);
                cx.notify();
            },
        ));

        // Выбор модели в выпадающем списке настроек: Confirm(Option<путь>),
        // "" = «Первая доступная» (ai_model_path = None). Отсутствующий на
        // диске GGUF в Select disabled, но проверку дублируем в лог.
        if let Some(select) = settings.select.clone() {
            subscriptions.push(cx.subscribe(
                &select,
                |this: &mut Self, _, event: &SelectEvent<Vec<ModelItem>>, cx| match event {
                    SelectEvent::Confirm(Some(v)) if v.is_empty() => {
                        log::info!("настройки: модель — первая доступная из списка плагина");
                        this.settings.persist(&this.base, |cfg| cfg.ai_model_path = None);
                        cx.notify();
                    }
                    SelectEvent::Confirm(Some(v)) => {
                        if Path::new(v).is_file() {
                            log::info!("настройки: модель: {}", file_label(v));
                            let picked = v.clone();
                            this.settings
                                .persist(&this.base, |cfg| cfg.ai_model_path = Some(picked));
                        } else {
                            log::warn!("настройки: файл модели не найден, не применено: {v}");
                        }
                        cx.notify();
                    }
                    SelectEvent::Confirm(None) => {}
                },
            ));
        }

        // Drain событий воркера: mpsc-канал не асинхронный — поллим каждые
        // 150 мс в foreground-задаче; перерисовка — при событиях. Каждые ~1 с
        // при открытой вкладке «Логи» — свежий хвост vibebridge.log.
        cx.spawn(async move |this, cx| {
            let mut tick: u32 = 0;
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(150))
                    .await;
                tick = tick.wrapping_add(1);
                let _ = this.update(cx, |root, cx| {
                    if root.main.drain_pending(cx) {
                        cx.notify();
                    }
                    if tick % 7 == 0 && root.tab == Tab::Logs {
                        root.logs_tail = logs_view::refresh_tail();
                        cx.notify();
                    }
                });
            }
        })
        .detach();

        Self {
            base,
            tab: Tab::Main,
            dark,
            backend_selection,
            main,
            logs_tail: String::new(),
            logs_path: None,
            settings,
            _subscriptions: subscriptions,
        }
    }
}

impl Render for RootView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            // Шапка — своя (см. header_bar): библиотечный TitleBar делает
            // вкладки некликабельными (WCA::Drag на всём контейнере детей).
            .child(header_bar(self, cx))
            // Содержимое активной вкладки — остаток высоты.
            .child(
                div().flex().flex_col().flex_1().overflow_hidden().child(match self.tab {
                    Tab::Main => main_view::render(self, cx),
                    Tab::Logs => logs_view::render(self, cx),
                    Tab::Settings => settings_view::render(self, cx),
                }),
            )
    }
}

/// Своя шапка окна: слева заголовок + вкладки-кнопки (кликабельные), пустая
/// зона — drag окна, справа переключатель темы и кнопки окна.
///
/// Библиотечный `TitleBar` НЕ подходит: его render вешает `window_control_area
/// (Drag)` на контейнер ВСЕХ детей, а hit-test окна возвращает drag-область,
/// если WCA-hitbox есть в стеке hitbox'ов под мышью (gpui window.rs:2258) —
/// на Windows клики по детям уходят в HTCAPTION и в приложение не попадают.
/// Поэтому drag-зона — отдельный пустой div: кликабельные элементы вне её.
fn header_bar(root: &RootView, cx: &mut Context<RootView>) -> Div {
    // Вкладки-кнопки: активная залита primary, неактивные ghost (план спеца №4).
    let tab = |id: &'static str, label: &'static str, active: bool| {
        let btn = Button::new(id).label(label);
        if active { btn.primary() } else { btn.ghost() }
    };
    let tab_main = tab("tab_main", "Главная", root.tab == Tab::Main).on_click(
        cx.listener(|this: &mut RootView, _, _, cx| {
            this.tab = Tab::Main;
            cx.notify();
        }),
    );
    let tab_logs = tab("tab_logs", "Логи", root.tab == Tab::Logs).on_click(cx.listener(
        |this: &mut RootView, _, _, cx| {
            this.tab = Tab::Logs;
            cx.notify();
        },
    ));
    let tab_settings = tab("tab_settings", "Настройки", root.tab == Tab::Settings).on_click(
        cx.listener(|this: &mut RootView, _, _, cx| {
            this.tab = Tab::Settings;
            cx.notify();
        }),
    );

    // Переключатель темы: change сбрасывает палитру на дефолт registered-темы,
    // поэтому каждая смена режима идёт через theme::set_dark/set_light
    // (тёмная — change + наша палитра, светлая — дефолт gpui).
    let toggle = Button::new("theme_toggle")
        .ghost()
        .label(if root.dark { "☀ Светлая" } else { "🌙 Тёмная" })
        .on_click(cx.listener(|this: &mut RootView, _, _, cx| {
            this.dark = !this.dark;
            if this.dark {
                theme::set_dark(cx);
            } else {
                theme::set_light(cx);
            }
            log::info!("GPUI: тема — {}", if this.dark { "тёмная" } else { "светлая" });
        }));

    // Кнопки окна (библиотечные WindowControls приватны): свернуть/развернуть/закрыть.
    let win_min = Button::new("win_minimize")
        .label("─")
        .ghost()
        .on_click(|_, window, _| window.minimize_window());
    let win_max = Button::new("win_maximize")
        .label("❐")
        .ghost()
        .on_click(|_, window, _| window.zoom_window());
    let win_close = Button::new("win_close")
        .label("✕")
        .ghost()
        .on_click(|_, window, _| window.remove_window());

    div()
        .flex()
        .flex_row()
        .items_center()
        .h(px(36.0))
        .bg(cx.theme().title_bar)
        .border_b_1()
        .border_color(cx.theme().title_bar_border)
        .pl(px(12.0))
        .child(
            h_flex()
                .gap_3()
                .items_center()
                .child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child(SharedString::from("VibeBridge — GPUI-фронтенд")),
                )
                .child(tab_main)
                .child(tab_logs)
                .child(tab_settings),
        )
        // Пустая зона — таскание окна (drag-hitbox только здесь: вкладки,
        // тема и кнопки окна не попадают в него и остаются кликабельными).
        .child(
            div()
                .id("header_drag")
                .flex_1()
                .h_full()
                .window_control_area(WindowControlArea::Drag),
        )
        .child(toggle)
        .child(win_min)
        .child(win_max)
        .child(win_close)
}
