//! Стейт вкладки «Главная» GPUI (PRP 14.11.26 шаг 3): uplink + черновик +
//! downlink + entity'и компонентов (Tree, две textarea) + каналы воркера.
//! Владелец — Entity<RootView>; подписки на textarea и drain-задача событий —
//! в root_view.rs. Логика секций — соседние *_state.rs, данные дерева —
//! tree_items.rs.

use std::path::Path;
use std::sync::mpsc::{Receiver, Sender};

use gpui_kit::component::input::TextareaState;
use gpui_kit::component::tree::{TreeItem, TreeState};
use gpui_kit::{AppContext as _, Context, Entity, Window};

use super::downlink_state::DownlinkState;
use super::draft_state::DraftState;
use super::tree_items;
use super::uplink_state::UplinkState;
use crate::worker::{Command, Event};

/// Агрегат секций «Главной» + entity'и input-компонентов.
pub struct MainState {
    pub uplink: UplinkState,
    pub draft: DraftState,
    pub downlink: DownlinkState,
    /// Многострочный промпт черновика (текст — подписка в root_view).
    pub prompt: Entity<TextareaState>,
    /// Многострочный ответ LLM для downlink (текст — подписка в root_view).
    pub response: Entity<TextareaState>,
    /// Дерево «Проект → чат» (структура пересобирается на новый скан).
    pub tree_state: Entity<TreeState>,
    cmd_tx: Sender<Command>,
    event_rx: Receiver<Event>,
}

impl MainState {
    pub fn new(
        base: &Path,
        cmd_tx: Sender<Command>,
        event_rx: Receiver<Event>,
        window: &mut Window,
        cx: &mut Context<crate::gpui_ui::root_view::RootView>,
    ) -> Self {
        let draft = DraftState::load(base);
        // Многострочные поля: TextareaState (mode TextareaMode); текст читаем
        // по подписке InputEvent::Change — во время печати черновик не трогаем.
        let prompt = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("Текст запроса к LLM…")
                .rows(6)
        });
        let response = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("Вставь ответ LLM с блоками <<<FILE: путь>>>…<<<END_FILE: путь>>>")
                .rows(10)
        });
        // Заглушка дерева до первого скана.
        let tree_state = cx.new(|cx| {
            let mut st = TreeState::new(cx);
            st.set_items(
                vec![TreeItem::new(
                    "tree_empty_hint",
                    "Выбери папку проекта — появится дерево файлов.",
                )],
                cx,
            );
            st
        });
        Self {
            uplink: UplinkState::new(),
            draft,
            downlink: DownlinkState::new(),
            prompt,
            response,
            tree_state,
            cmd_tx,
            event_rx,
        }
    }

    /// Канал команд воркера (скан, токены, AI-подбор).
    pub fn cmd_tx(&self) -> &Sender<Command> {
        &self.cmd_tx
    }

    /// Выслать запрос скана (после выбора/обновления папки).
    pub fn send_scan(&self, root: std::path::PathBuf) {
        if let Err(e) = self.cmd_tx.send(Command::Scan { root }) {
            log::error!("команда скана не отправлена: {e}");
        }
    }

    /// Забрать все накопившиеся события воркера. true — было хоть одно
    /// (RootView перерисуется); черновик сохраняется при каждом вызове.
    pub fn drain_pending(&mut self, cx: &mut Context<crate::gpui_ui::root_view::RootView>) -> bool {
        let mut had = false;
        while let Ok(ev) = self.event_rx.try_recv() {
            self.on_worker_event(&ev, cx);
            had = true;
        }
        self.draft.flush_if_dirty();
        had
    }

    /// Маршрутизация события воркера в секции; структура дерева пересобирается
    /// на новый скан (ai_picked/check-изменения tree не требуют — снапшот
    /// строк собирается заново на каждом render).
    fn on_worker_event(&mut self, ev: &Event, cx: &mut Context<crate::gpui_ui::root_view::RootView>) {
        match ev {
            Event::Scanned { root, out } => {
                self.uplink.on_scanned(root, out.clone());
                self.rebuild_tree_items(cx);
            }
            Event::Tokens { root, rel, count } => {
                self.uplink.on_tokens(root, rel, *count);
            }
            Event::AiStatus { root, msg } => {
                self.uplink.on_ai_status(root, msg);
            }
            Event::AiPicked { root, selected, reasoning } => {
                self.uplink.on_ai_picked(root, selected, reasoning);
                self.draft.on_ai_finished(root);
            }
            Event::AiPickFailed { root, msg } => {
                self.uplink.on_ai_failed(root, msg);
                self.draft.on_ai_finished(root);
            }
        }
    }

    /// Пересобрать TreeItem-структуру по текущему скану (или заглушку).
    pub fn rebuild_tree_items(
        &mut self,
        cx: &mut Context<crate::gpui_ui::root_view::RootView>,
    ) {
        let items = match &self.uplink.scan() {
            Some(scan) => tree_items::build_tree_items(scan, &self.uplink.expanded),
            None => vec![TreeItem::new(
                "tree_empty_hint",
                "Выбери папку проекта — появится дерево файлов.",
            )],
        };
        let tree_state = self.tree_state.clone();
        tree_state.update(cx, |st, cx| st.set_items(items, cx));
    }
}
