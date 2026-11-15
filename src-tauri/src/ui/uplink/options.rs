//! Панель опций Uplink (egui): чекбоксы опций промпта. Вынесено из `uplink.rs`
//! (лимит файла 500 строк, правило владельца).

use std::path::Path;

use tauri_plugin_egui::egui;

use agent_core::tree::prompt::{CHANGES_FILE, PLAN_FILE, InstructionSlot};

use super::UplinkState;

impl UplinkState {
    pub(super) fn draw_options(&mut self, ui: &mut egui::Ui, base: &Path) {
        ui.checkbox(&mut self.include_selected, "Включить выбранные файлы");
        // Группа от «Включить структуру»: структура + радио + обе инструкции
        // (паритет с рамкой GPUI-ряда опций).
        ui.group(|ui| {
            ui.checkbox(&mut self.structure_checked, "Включить структуру");
            ui.add_enabled_ui(self.structure_checked, |ui| {
                ui.horizontal(|ui| {
                    ui.weak("по:");
                    ui.radio_value(&mut self.structure_all, false, "отмеченным");
                    ui.radio_value(&mut self.structure_all, true, "всем файлам");
                });
            });
            Self::draw_instruction_slot(
                ui,
                base,
                &mut self.changes,
                CHANGES_FILE,
                "Включить инструкцию выдачи изменений",
                "инструкция изменений",
            );
            Self::draw_instruction_slot(
                ui,
                base,
                &mut self.plan,
                PLAN_FILE,
                "Включить инструкцию режима плана",
                "инструкция плана",
            );
        });
    }

    /// Чекбокс + строка-хинт одной инструкции (общий для обоих слотов).
    /// Включение — (пере)читает md с диска; ошибка видима, не молчит.
    fn draw_instruction_slot(
        ui: &mut egui::Ui,
        base: &Path,
        slot: &mut InstructionSlot,
        file: &str,
        label: &str,
        hint_label: &str,
    ) {
        let before = slot.checked;
        ui.checkbox(&mut slot.checked, label);
        if slot.checked && !before {
            slot.refresh(base, file);
        }
        if slot.checked {
            if let Some((text, is_error)) = slot.hint(hint_label) {
                if is_error {
                    ui.label(egui::RichText::new(text).color(egui::Color32::LIGHT_RED));
                } else {
                    ui.weak(text);
                }
            }
        }
    }
}
