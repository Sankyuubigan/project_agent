//! Палитра GPUI-окна: 4 слоя глубины как в egui (SSOT — `ui/style.rs::visuals`,
//! тёмная ветка) + акценты из плана спеца 15.11.26. Дефолтная тёмная тема
//! gpui (#09090B, «угольная») заменяется графитовой схемой.
//!
//! ВАЖНО: `Theme::change(mode)` сбрасывает colors на дефолт registered-темы
//! (apply_config), поэтому кастомная палитра ставится СТРОГО после change —
//! через `Theme::update` (он не трогает mode, reconcile синхронизирует
//! semantic-токены, смена font_family пере-резолвит шрифты).

use gpui_kit::component::theme::{Theme, ThemeMode};
use gpui_kit::{App, Hsla, SharedString, px};

// — слои (паритет egui visuals(dark=true)) —
const WINDOW_BG: u32 = 0x181A20; // фон окна (панель)
const CARD_BG: u32 = 0x1F222A; // карточки — СВЕТЛЕЕ фона (слой вверх)
const FIELD_BG: u32 = 0x121419; // поля ввода/дерево — утопленный слой
const BORDER: u32 = 0x323846; // рамки
const TEXT: u32 = 0xE6EDF3; // основной текст (не белый на угле)
const MUTED: u32 = 0x8B949E; // второстепенный текст
const FAINT: u32 = 0x16181E; // faint-плашки (egui faint_bg_color)

// — акценты (план спеца: GitHub-палитра) —
const PRIMARY: u32 = 0x1F6FEB; // синие кнопки
const PRIMARY_HOVER: u32 = 0x388BFD;
const SUCCESS: u32 = 0x238636; // зелёные кнопки
const SUCCESS_HOVER: u32 = 0x2EA043;
const WHITE: u32 = 0xFFFFFF;

// — вторичные/дефолтные кнопки (паритет egui widgets inactive/hovered/active) —
const SEC_BG: u32 = 0x282C34;
const SEC_HOVER: u32 = 0x32363F;
const SEC_ACTIVE: u32 = 0x3B4252;

// — скроллбар (паритет egui inactive/hovered хэндл) —
const SCROLL_THUMB: u32 = 0x3B4252;
const SCROLL_THUMB_HOVER: u32 = 0x4C566A;

/// Шрифты (план спеца №6: Segoe UI 14px; mono — Consolas как в egui).
const FONT_UI: &str = "Segoe UI";
const FONT_MONO: &str = "Consolas";
const FONT_SIZE: f32 = 14.0;

/// Литерал hex → Hsla (ThemeColor хранит Hsla).
fn c(hex: u32) -> Hsla {
    gpui_kit::rgb(hex).into()
}

/// Переключение в тёмную тему: дефолтная change + наша палитра поверх.
/// Вызывать при старте окна и при переключении ☀/🌙 (change сбрасывает кастом).
pub fn set_dark(cx: &mut App) {
    Theme::change(ThemeMode::Dark, None, cx);
    apply_dark_palette(cx);
}

/// Светлая тема — дефолтная палитра gpui (план спеца кастомизирует тёмную).
pub fn set_light(cx: &mut App) {
    Theme::change(ThemeMode::Light, None, cx);
}

/// Тёмная палитра: слои, акценты, шрифт. Только после change(Dark).
fn apply_dark_palette(cx: &mut App) {
    Theme::update(cx, |t| {
        // Слои фона: окно → карточка (secondary/popover) → поле (input/list).
        t.colors.background = c(WINDOW_BG);
        t.colors.secondary = c(CARD_BG);
        t.colors.secondary_foreground = c(TEXT);
        t.colors.secondary_hover = c(SEC_HOVER);
        t.colors.secondary_active = c(SEC_ACTIVE);
        t.colors.popover = c(CARD_BG);
        t.colors.popover_foreground = c(TEXT);
        t.colors.input = c(FIELD_BG);
        t.colors.list = c(FIELD_BG);
        t.colors.list_even = c(FIELD_BG);
        t.colors.list_hover = c(SEC_BG);
        t.colors.list_head = c(CARD_BG);
        t.colors.list_active = c(SEC_ACTIVE);
        t.colors.list_active_border = c(PRIMARY);

        // Текст и рамки.
        t.colors.foreground = c(TEXT);
        t.colors.muted = c(FAINT);
        t.colors.muted_foreground = c(MUTED);
        t.colors.border = c(BORDER);
        t.colors.window_border = c(BORDER);

        // Дефолтные и вторичные кнопки (кнопки без варианта — как egui secondary).
        t.colors.button = c(SEC_BG);
        t.colors.button_foreground = c(TEXT);
        t.colors.button_hover = c(SEC_HOVER);
        t.colors.button_active = c(SEC_ACTIVE);
        t.colors.button_secondary = c(SEC_BG);
        t.colors.button_secondary_foreground = c(TEXT);
        t.colors.button_secondary_hover = c(SEC_HOVER);
        t.colors.button_secondary_active = c(SEC_ACTIVE);

        // Акценты: primary синий (кнопка «Собрать и скопировать», активная вкладка),
        // success зелёный («Применить изменения»).
        t.colors.primary = c(PRIMARY);
        t.colors.primary_foreground = c(WHITE);
        t.colors.primary_hover = c(PRIMARY_HOVER);
        t.colors.primary_active = c(PRIMARY);
        t.colors.button_primary = c(PRIMARY);
        t.colors.button_primary_foreground = c(WHITE);
        t.colors.button_primary_hover = c(PRIMARY_HOVER);
        t.colors.button_primary_active = c(PRIMARY);
        t.colors.success = c(SUCCESS);
        t.colors.success_foreground = c(WHITE);
        t.colors.success_hover = c(SUCCESS_HOVER);
        t.colors.success_active = c(SUCCESS);
        t.colors.button_success = c(SUCCESS);
        t.colors.button_success_foreground = c(WHITE);
        t.colors.button_success_hover = c(SUCCESS_HOVER);
        t.colors.button_success_active = c(SUCCESS);
        t.colors.accent = c(PRIMARY);
        t.colors.accent_foreground = c(WHITE);
        t.colors.selection = c(PRIMARY);
        t.colors.ring = c(PRIMARY);
        t.colors.link = c(PRIMARY);
        t.colors.link_hover = c(PRIMARY_HOVER);
        t.colors.link_active = c(PRIMARY);

        // Шапка/вкладки/скроллбар.
        t.colors.title_bar = c(WINDOW_BG);
        t.colors.title_bar_border = c(BORDER);
        t.colors.tab_bar = c(WINDOW_BG);
        t.colors.tab = c(WINDOW_BG);
        t.colors.tab_active = c(CARD_BG);
        t.colors.tab_foreground = c(TEXT);
        t.colors.tab_active_foreground = c(TEXT);
        t.colors.scrollbar = c(FIELD_BG);
        t.colors.scrollbar_thumb = c(SCROLL_THUMB);
        t.colors.scrollbar_thumb_hover = c(SCROLL_THUMB_HOVER);

        // Шрифт: edit детектит смену font_family и сам пере-резолвит дефолтные
        // шрифты окна (resolve_default_font).
        t.font_family = SharedString::from(FONT_UI);
        t.font_size = px(FONT_SIZE);
        t.mono_font_family = SharedString::from(FONT_MONO);
        t.mono_font_size = px(FONT_SIZE);
    });
}
