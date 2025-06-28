# core/ui_components.py
import tkinter as tk
from tkinter import scrolledtext

class LineNumberedText(tk.Frame):
    """
    A composite widget that combines a ScrolledText widget with a line number display.
    """
    def __init__(self, master, *args, **kwargs):
        tk.Frame.__init__(self, master)

        self.linenumbers = tk.Text(self, width=4, padx=4, takefocus=0, border=0,
                                   background='#f1f1f1', foreground='#999999',
                                   state='disabled')
        self.text = scrolledtext.ScrolledText(self, *args, **kwargs)
        
        self.linenumbers.pack(side="left", fill="y")
        self.text.pack(side="right", fill="both", expand=True)
        
        # --- ИСПРАВЛЕНИЕ: Надежная синхронизация прокрутки ---
        # Команда для ползунка прокрутки теперь единый метод _on_scroll
        self.text.vbar.config(command=self._on_scroll)
        # Команда для прокрутки самого текстового поля (колесо мыши) - метод _on_text_yscroll
        self.text.config(yscrollcommand=self._on_text_yscroll)
        
        # Вызов перерисовки при изменении текста или размера виджета
        self.text.bind("<<Modified>>", self._on_text_modified)
        self.text.bind("<Configure>", self._on_text_modified)
        
        self._is_modified_scheduled = False
        self.text.edit_modified(False)

    def _on_scroll(self, *args):
        """
        Единая команда для ползунка прокрутки. Применяет yview к обоим виджетам,
        сохраняя их синхронизацию при перетаскивании ползунка.
        """
        self.text.yview(*args)
        self.linenumbers.yview(*args)

    def _on_text_yscroll(self, first, last):
        """
        Вызывается при прокрутке текстового поля (например, колесом мыши).
        Обновляет положение ползунка и синхронизирует номера строк.
        """
        self.text.vbar.set(first, last)
        # Напрямую перемещаем номера строк в ту же позицию, что и текст.
        self.linenumbers.yview_moveto(first)

    def _on_text_modified(self, event=None):
        """
        Планирует перерисовку номеров строк, чтобы избежать избыточных обновлений.
        Вызывается при изменении текста или размера виджета (<Configure>).
        """
        if not self._is_modified_scheduled:
            self._is_modified_scheduled = True
            # Используем after_idle, чтобы дождаться, пока менеджер компоновки завершит работу.
            self.after_idle(self._redraw_line_numbers)
        
        # Сбрасываем флаг модификации после того, как мы обработали изменение.
        if self.text.edit_modified():
            self.text.edit_modified(False)

    def _redraw_line_numbers(self):
        """Перерисовывает номера строк и обеспечивает финальную синхронизацию по Y."""
        if not self.winfo_exists(): return
        self._is_modified_scheduled = False
        
        self.linenumbers.config(state='normal')
        self.linenumbers.delete("1.0", "end")
        
        # Получаем часть с номером строки из индекса.
        line_count_str = self.text.index("end-1c").split('.')[0]
        
        if line_count_str and line_count_str.isdigit():
            line_count = int(line_count_str)
            if line_count > 0:
                line_numbers_string = "\n".join(str(i) for i in range(1, line_count + 1))
                self.linenumbers.insert("1.0", line_numbers_string)
        
        self.linenumbers.config(state='disabled')
        
        # --- КРИТИЧЕСКОЕ ИСПРАВЛЕНИЕ ---
        # После перерисовки принудительно устанавливаем Y-позицию номеров строк,
        # чтобы она совпадала с текстовым полем. Это исправляет рассинхронизацию
        # после переноса строк из-за изменения размера окна.
        yview_result = self.text.yview()
        if yview_result:
            self.linenumbers.yview_moveto(yview_result[0])

    # --- Public Methods ---
    def get(self, *args, **kwargs):
        return self.text.get(*args, **kwargs)

    def insert(self, *args, **kwargs):
        result = self.text.insert(*args, **kwargs)
        self._on_text_modified()
        return result

    def delete(self, *args, **kwargs):
        result = self.text.delete(*args, **kwargs)
        self._on_text_modified()
        return result
    
    def tag_add(self, *args, **kwargs):
        return self.text.tag_add(*args, **kwargs)
        
    def bind(self, *args, **kwargs):
        self.text.bind(*args, **kwargs)