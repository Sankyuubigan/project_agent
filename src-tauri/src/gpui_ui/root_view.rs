//! Корневой view GPUI-окна (PRP 14.11.26 шаги 2–3, миграция на кит 15.11.26):
//! каркас — [`AppShell`] кита (вкладки/тема/кнопки окна/drag, урок WCA::Drag
//! внутри), содержимое активной вкладки — render-функции соседних модулей.
//! Тема — [`setup_theme`] кита (SSOT палитры; gpui_ui/theme.rs удалён).
//!
//! RootView владеет MainState («Главная») и подписками (`_subscriptions`,
//! правило скилла gpui-kit): InputEvent::Change двух textarea («Черновик
//! запроса», «Чат → проект»), TreeEvent раскрытия папок, SelectEvent модели
//! (через wire_model_select кита) и вечная drain-задача событий воркера
//! (`cx.spawn` + таймер, detach).

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use agent_core::config;
use my_gpui_kit::component::input::InputEvent;
use my_gpui_kit::component::tree::TreeEvent;
use my_gpui_kit::component::ActiveTheme;
use my_gpui_kit::llama_settings::wire_model_select;
use my_gpui_kit::{
    AppShell, AppTab, Context, IntoElement, ParentElement as _, Render, ThemeMode, Window,
    setup_theme,
};
use super::main_state::MainState;
use super::{logs_view, main_view, settings_view};
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
    /// Хвост vibebridge.log для вкладки «Логи» (последние 400 строк, drain).
    pub logs_tail: String,
    /// Стейт настроек (конфиг приложения + конфиг плагина llama-engine).
    pub settings: settings_view::SettingsState,
    /// Все cx.subscribe — обязаны храниться (правило скилла gpui-kit).
    _subscriptions: Vec<my_gpui_kit::Subscription>,
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
        // тоже стартует тёмным). Кит-SSOT: setup_theme = change + палитра
        // 4 слоёв; смена режима — тоже только через setup_theme.
        setup_theme(ThemeMode::Dark, cx);
        log::info!("GPUI: тема — тёмная палитра 4 слоёв из крейта my-gpui-kit");
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

        // Выбор модели: wire_model_select кита (Confirm(Option<путь>) →
        // on_pick); None = «Первая доступная» (ai_model_path = None).
        // Отсутствующий на диске GGUF в Select disabled, но проверку дублируем.
        if let Some(select) = settings.select.clone() {
            subscriptions.push(wire_model_select(&select, cx, |this: &mut Self, picked, cx| {
                match picked {
                    None => {
                        log::info!("настройки: модель — первая доступная из списка плагина");
                        this.settings.persist(&this.base, |cfg| cfg.ai_model_path = None);
                        cx.notify();
                    }
                    Some(v) => {
                        if Path::new(&v).is_file() {
                            log::info!("настройки: модель: {v}");
                            this.settings
                                .persist(&this.base, |cfg| cfg.ai_model_path = Some(v));
                        } else {
                            log::warn!("настройки: файл модели не найден, не применено: {v}");
                        }
                        cx.notify();
                    }
                }
            }));
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
            settings,
            _subscriptions: subscriptions,
        }
    }
}

impl Render for RootView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Вкладки-кнопки AppShell: активная залита primary (паритет шапки).
        let tab = |id: &'static str, label: &'static str, tab: Tab| {
            let mut t = AppTab::new(id, label);
            if self.tab == tab {
                t = t.active();
            }
            t.on_click(cx.listener(move |this: &mut Self, _, _, cx| {
                this.tab = tab;
                cx.notify();
            }))
        };
        // Подпись toggle — СЛЕДУЮЩАЯ тема (паритет прежней шапки).
        let theme_label = if self.dark { "☀ Светлая" } else { "🌙 Тёмная" };

        AppShell::new("VibeBridge — GPUI-фронтенд")
            .tab(tab("tab_main", "Главная", Tab::Main))
            .tab(tab("tab_logs", "Логи", Tab::Logs))
            .tab(tab("tab_settings", "Настройки", Tab::Settings))
            .theme_toggle(theme_label, cx.listener(|this: &mut Self, _, _, cx| {
                this.dark = !this.dark;
                if this.dark {
                    setup_theme(ThemeMode::Dark, cx);
                } else {
                    setup_theme(ThemeMode::Light, cx);
                }
                log::info!(
                    "GPUI: тема — {}",
                    if this.dark { "тёмная" } else { "светлая" }
                );
            }))
            // Содержимое активной вкладки (AppShell сам даёт flex_1 + hidden).
            .child(match self.tab {
                Tab::Main => main_view::render(self, cx),
                Tab::Logs => logs_view::render(self, cx),
                Tab::Settings => settings_view::render(self, cx),
            })
    }
}
