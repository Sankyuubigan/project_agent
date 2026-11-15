//! Вкладка «Логи»: хвост файла лога (plugin-log-collector — единственная точка
//! записи) + путь к файлу. Без WebView логи читаются напрямую из Rust.

use tauri_plugin_egui::egui;

/// Сколько последних строк показывать.
const LOG_TAIL_LINES: usize = 400;

pub struct LogView {
    tail: String,
    path: String,
    path_missing: bool,
}

impl LogView {
    pub fn new() -> Self {
        Self { tail: String::new(), path: String::new(), path_missing: false }
    }

    pub fn draw(&mut self, ctx: &egui::Context) {
        // Хвост лога читаем на каждый кадр вкладки (файл маленький, кадры редкие).
        self.tail = plugin_log_collector::logger::read_tail(LOG_TAIL_LINES);
        if self.path.is_empty() {
            self.path = plugin_log_collector::logger::log_file_path()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            self.path_missing = self.path.is_empty();
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Логи");
                ui.separator();
                if self.path_missing {
                    ui.label(
                        egui::RichText::new("файл лога не найден (лог пишется в память/консоль)")
                            .weak(),
                    );
                } else {
                    ui.monospace(&self.path);
                }
            });
            ui.separator();
            if self.tail.is_empty() {
                ui.label("— лог пуст —");
            } else {
                // Read-only просмотр: TextEdit растягивается по контенту,
                // скролл даёт внешний ScrollArea (сам TextEdit в egui не
                // скроллит; fixed-размер клипал текст без прокрутки).
                // interactive(false) запрещает редактирование/выделение.
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.tail)
                                .font(egui::TextStyle::Monospace)
                                .desired_width(ui.available_width())
                                .margin(egui::Margin::same(8))
                                .interactive(false),
                        );
                    });
            }
        });
    }
}
