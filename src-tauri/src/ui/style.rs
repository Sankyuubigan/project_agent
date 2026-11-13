//! Визуальный стиль: системные шрифты Windows (Segoe UI / Consolas), палитры
//! тёмной/светлой темы (в духе GitHub Dark / VS Code) и настольные метрики.
//!
//! Из коробки egui рендерит встроенным Ubuntu-Light с крупными отступами и
//! почти чёрными полями ввода — на Windows выглядит чужеродно. Шрифты грузим
//! рантаймом из C:\Windows\Fonts (поставлять шрифт с приложением нельзя по
//! лицензии, читать установленный системный — можно). Настоящего жирного
//! начертания у egui нет (`RichText::strong` — просто более яркий цвет).

use tauri_plugin_egui::egui;

// — Акценты (общие для обеих тем, текст на них всегда белый) —
/// Главная зелёная кнопка («Применить изменения»).
pub const GREEN: egui::Color32 = egui::Color32::from_rgb(35, 134, 54);
pub const GREEN_HOVER: egui::Color32 = egui::Color32::from_rgb(46, 160, 67);
/// Голубая акцентная кнопка («Собрать и скопировать», «Скопировать текст»).
pub const BLUE: egui::Color32 = egui::Color32::from_rgb(31, 111, 235);
pub const BLUE_HOVER: egui::Color32 = egui::Color32::from_rgb(56, 139, 253);
/// Тёплый акцент контурной кнопки («Откатить последний патч»).
pub const AMBER: egui::Color32 = egui::Color32::from_rgb(210, 153, 34);

/// Толщина тонких рамок. Явный f32: голый литерал `1.0` триггерит
/// future-incompat lint float_literal_f32_fallback (Rust #154024).
const STROKE_WIDTH: f32 = 1.0;

/// Шрифты: системные Segoe UI (текст) + Consolas (пути/код). Встроенные
/// остаются фолбэком (эмодзи и редкие глифы вроде ☑/▣). Вызывается один раз.
pub fn apply_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    install_font(&mut fonts, "segoe_ui", r"C:\Windows\Fonts\segoeui.ttf");
    install_font(&mut fonts, "consolas", r"C:\Windows\Fonts\consola.ttf");

    // Первым в семье — системный шрифт; встроенные остаются фолбэком.
    if let Some(list) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        list.insert(0, "segoe_ui".to_string());
    }
    if let Some(list) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
        list.insert(0, "consolas".to_string());
    }
    ctx.set_fonts(fonts);
}

/// Тема и метрики: при старте и при переключении ☀/🌙. Шрифты не трогает —
/// за них отвечает [`apply_fonts`].
pub fn apply_theme(ctx: &egui::Context, dark: bool) {
    let mut style = (*ctx.style()).clone();
    // 14px: тонкие шрифты на 12–13px мылят, на 14px штрих достаточно жирный.
    style.text_styles = [
        (egui::TextStyle::Heading, egui::FontId::proportional(19.0)),
        (egui::TextStyle::Body, egui::FontId::proportional(14.0)),
        (egui::TextStyle::Button, egui::FontId::proportional(14.0)),
        (egui::TextStyle::Small, egui::FontId::proportional(12.0)),
        (egui::TextStyle::Monospace, egui::FontId::proportional(14.0)),
    ]
    .into();
    // «Воздух»: кнопки нормальной высоты, элементы не слипаются.
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 6.0);
    style.spacing.indent = 18.0;
    // Скроллбары постоянно видимы (floating прячутся, как на телефоне).
    // Хэндл egui красит по состоянию виджета (scroll_area.rs:1345→1396): в
    // покое это inactive.bg_fill/fg_stroke. В тёмной теме bg_fill (#282C34)
    // тёмный и виден на треке #121419; в светлой bg_fill = WHITE (кнопки по
    // ТЗ) и трек = extreme_bg тоже WHITE — хэндл исчез бы, поэтому там он
    // красится fg_stroke (цвет текста, тёмный на белом).
    style.spacing.scroll.floating = false;
    style.spacing.scroll.foreground_color = !dark;
    style.spacing.scroll.bar_width = 8.0;
    style.visuals = visuals(dark);
    ctx.set_style(style);
}

/// Рамка-карточка смыслового блока; тема берётся из текущего стиля.
pub fn card_frame(ctx: &egui::Context) -> egui::Frame {
    let v = &ctx.style().visuals;
    egui::Frame::new()
        .fill(v.window_fill)
        .stroke(v.widgets.noninteractive.bg_stroke)
        .corner_radius(6)
        .inner_margin(12)
        .outer_margin(6)
}

/// Цветная кнопка с белым текстом; в выключенном виде приглушается.
pub fn accent_button(
    ui: &mut egui::Ui,
    text: &str,
    enabled: bool,
    fill: egui::Color32,
    hover: egui::Color32,
) -> bool {
    ui.scope(|ui| {
        let v = &mut ui.style_mut().visuals;
        // Кнопка красится по weak_bg_fill (button.rs: fill.unwrap_or(weak)),
        // bg_fill нужен чекбоксам/слайдерам — задаём оба.
        v.widgets.inactive.bg_fill = fill;
        v.widgets.inactive.weak_bg_fill = fill;
        v.widgets.hovered.bg_fill = hover;
        v.widgets.hovered.weak_bg_fill = hover;
        v.widgets.active.bg_fill = hover;
        v.widgets.active.weak_bg_fill = hover;
        // Выключенный вид тускнеет сам: add_enabled(false) множит непрозрачность
        // пейнтера на visuals.disabled_alpha(), отдельного disabled-стейта в
        // egui 0.32 нет (Widgets: noninteractive/inactive/hovered/active/open).
        let label = egui::RichText::new(text).color(egui::Color32::WHITE);
        ui.add_enabled(enabled, egui::Button::new(label)).clicked()
    })
    .inner
}

/// Контурная кнопка: прозрачный фон, тонкая рамка; hover — оттенок `accent`.
pub fn outline_button(
    ui: &mut egui::Ui,
    text: &str,
    enabled: bool,
    accent: egui::Color32,
) -> bool {
    ui.scope(|ui| {
        let style = ui.style();
        let border = style.visuals.widgets.noninteractive.bg_stroke.color;
        let text_color = style.visuals.widgets.inactive.fg_stroke.color;
        let v = &mut ui.style_mut().visuals;
        v.widgets.inactive.bg_fill = egui::Color32::TRANSPARENT;
        v.widgets.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
        v.widgets.inactive.bg_stroke = egui::Stroke::new(STROKE_WIDTH, border);
        v.widgets.hovered.bg_fill = accent.gamma_multiply(0.15);
        v.widgets.hovered.weak_bg_fill = accent.gamma_multiply(0.15);
        v.widgets.hovered.bg_stroke = egui::Stroke::new(STROKE_WIDTH, accent);
        v.widgets.active.bg_fill = accent.gamma_multiply(0.3);
        v.widgets.active.weak_bg_fill = accent.gamma_multiply(0.3);
        v.widgets.active.bg_stroke = egui::Stroke::new(STROKE_WIDTH, accent);
        let label = egui::RichText::new(text).color(text_color);
        ui.add_enabled(enabled, egui::Button::new(label)).clicked()
    })
    .inner
}

/// Палитра одной из тем поверх встроенных visual egui.
fn visuals(dark: bool) -> egui::Visuals {
    let mut v = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    // (панель, карточка, поле, рамка, текст, второстепенный текст,
    //  вторичная кнопка: фон/hover/рамка/hover-рамка, фон полос)
    let (panel, card, field, border, text, weak, sec_bg, sec_hover, sec_s, sec_hs, faint) = if dark
    {
        (
            egui::Color32::from_rgb(24, 26, 32),   // #181A20
            egui::Color32::from_rgb(31, 34, 42),   // #1F222A
            egui::Color32::from_rgb(18, 20, 25),   // #121419
            egui::Color32::from_rgb(46, 52, 64),   // #2E3440
            egui::Color32::from_rgb(230, 237, 243), // #E6EDF3
            egui::Color32::from_rgb(139, 148, 158), // #8B949E
            egui::Color32::from_rgb(40, 44, 52),   // #282C34
            egui::Color32::from_rgb(50, 54, 63),   // #32363F
            egui::Color32::from_rgb(59, 66, 82),   // #3B4252
            egui::Color32::from_rgb(76, 86, 106),  // #4C566A
            egui::Color32::from_rgb(22, 24, 30),
        )
    } else {
        (
            egui::Color32::from_rgb(243, 244, 246), // #F3F4F6
            egui::Color32::WHITE,
            egui::Color32::WHITE,
            egui::Color32::from_rgb(209, 213, 219), // #D1D5DB
            egui::Color32::from_rgb(17, 24, 39),    // #111827
            egui::Color32::from_rgb(107, 114, 128), // #6B7280
            egui::Color32::WHITE,
            egui::Color32::from_rgb(243, 244, 246),
            egui::Color32::from_rgb(209, 213, 219),
            egui::Color32::from_rgb(156, 163, 175), // #9CA3AF
            egui::Color32::from_rgb(238, 240, 243),
        )
    };

    v.panel_fill = panel;
    v.window_fill = card;
    v.extreme_bg_color = field; // поля ввода/дерево — тёмно-графитовые, не чёрные
    v.faint_bg_color = faint;
    v.window_stroke = egui::Stroke::new(STROKE_WIDTH, border);
    v.hyperlink_color = BLUE;
    v.selection.bg_fill = BLUE;
    v.selection.stroke = egui::Stroke::new(STROKE_WIDTH, egui::Color32::WHITE);

    let s = |c: egui::Color32| egui::Stroke::new(STROKE_WIDTH, c);
    v.widgets.noninteractive.bg_stroke = s(border);
    v.widgets.noninteractive.fg_stroke = s(weak);
    v.widgets.inactive.bg_fill = sec_bg;
    v.widgets.inactive.weak_bg_fill = sec_bg;
    v.widgets.inactive.bg_stroke = s(sec_s);
    v.widgets.inactive.fg_stroke = s(text);
    v.widgets.hovered.bg_fill = sec_hover;
    v.widgets.hovered.weak_bg_fill = sec_hover;
    v.widgets.hovered.bg_stroke = s(sec_hs);
    v.widgets.hovered.fg_stroke = s(text);
    v.widgets.active.bg_fill = sec_hover;
    v.widgets.active.weak_bg_fill = sec_hover;
    v.widgets.active.bg_stroke = s(sec_hs);
    v.widgets.active.fg_stroke = s(text);
    v.widgets.open.fg_stroke = s(text);
    for w in [
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
    ] {
        w.corner_radius = egui::CornerRadius::same(4);
    }
    v
}

/// Прочитать ttf и добавить в определения; файла нет → warn и встроенный
/// фолбэк (приложение остаётся работоспособным).
fn install_font(fonts: &mut egui::FontDefinitions, name: &str, path: &str) {
    match std::fs::read(path) {
        Ok(bytes) => {
            fonts
                .font_data
                .insert(name.to_string(), egui::FontData::from_owned(bytes).into());
        }
        Err(e) => log::warn!("шрифт {path} не загружен ({e}) — будет встроенный"),
    }
}
