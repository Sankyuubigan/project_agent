# gui_utils.py
import tkinter as tk
from tkinter import scrolledtext, filedialog

try:
    import pyperclip
except ImportError:
    pyperclip = None

from core.treeview_logic import populate_file_tree_threaded

def create_context_menu(widget):
    """Creates a standard context-menu for a widget."""
    menu = tk.Menu(widget, tearoff=0)
    is_text = isinstance(widget, (tk.Text, scrolledtext.ScrolledText))
    is_entry = isinstance(widget, tk.Entry)

    if is_text or is_entry:
        menu.add_command(label="Вырезать", command=lambda: widget.event_generate("<<Cut>>"))
        menu.add_command(label="Копировать", command=lambda: widget.event_generate("<<Copy>>"))
        menu.add_command(label="Вставить", command=lambda: widget.event_generate("<<Paste>>"))
        menu.add_separator()
        menu.add_command(label="Выделить все", command=lambda: widget.event_generate("<<SelectAll>>"))

    def show_context_menu(event):
        if not widget.winfo_exists(): return
        
        is_editable = widget.cget("state") != "disabled"
        
        has_selection = False
        if is_text:
            if widget.tag_ranges(tk.SEL):
                has_selection = True
        elif is_entry:
            if widget.select_present():
                has_selection = True

        can_paste = False
        if pyperclip:
            paste_content = pyperclip.paste()
            if paste_content:
                can_paste = True

        if is_text or is_entry:
            menu.entryconfigure("Вырезать", state="normal" if has_selection and is_editable else "disabled")
            menu.entryconfigure("Копировать", state="normal" if has_selection else "disabled")
            menu.entryconfigure("Вставить", state="normal" if can_paste and is_editable else "disabled")
            menu.entryconfigure("Выделить все", state="normal")

        if menu.index('end') is not None:
            menu.tk_popup(event.x_root, event.y_root)

    widget.bind("<Button-3>", show_context_menu, add="+")
    if is_text:
        widget.bind("<<SelectAll>>", lambda e: widget.tag_add(tk.SEL, "1.0", tk.END))
    elif is_entry:
         widget.bind("<<SelectAll>>", lambda e: widget.select_range(0, tk.END))


def copy_logs(log_widget):
    """Copies text from the log widget to the clipboard."""
    if not log_widget or not log_widget.winfo_exists(): return
    
    log_text = log_widget.get("1.0", tk.END).strip()
    if log_text:
        if pyperclip:
            pyperclip.copy(log_text)
            log_widget.insert(tk.END, "\nЛоги скопированы!\n", ('success',))
        else:
            log_widget.insert(tk.END, "\nОшибка: библиотека pyperclip не найдена. Копирование невозможно.\n", ('error',))
    else:
        log_widget.insert(tk.END, "\nЛоги пусты.\n", ('info',))
    
    if log_widget.winfo_exists():
        log_widget.see(tk.END)

def clear_logs(log_widget):
    """Clears the log widget."""
    if log_widget and log_widget.winfo_exists():
        log_widget.delete("1.0", tk.END)
        log_widget.insert(tk.END, "Лог очищен.\n", ('info',))

def clear_input_field(input_text_widget, log_widget):
    """Clears the text input field."""
    if input_text_widget and hasattr(input_text_widget, 'winfo_exists') and input_text_widget.winfo_exists():
        input_text_widget.delete("1.0", tk.END)
    if log_widget and log_widget.winfo_exists():
        log_widget.insert(tk.END, "Поле ввода очищено.\n", ('info',))

def select_project_dir(entry_widget, tree_widget, log_widget_ref, progress_bar_ref, progress_label_ref):
    """Opens a directory selection dialog and populates the tree."""
    dir_path = filedialog.askdirectory()
    if dir_path:
        if entry_widget and entry_widget.winfo_exists():
             entry_widget.delete(0, tk.END)
             entry_widget.insert(0, dir_path)
        if log_widget_ref and log_widget_ref.winfo_exists():
             log_widget_ref.insert(tk.END, f"Выбрана директория (принудительное обновление): {dir_path}\n", ('info',))
        
        populate_file_tree_threaded(dir_path, tree_widget, log_widget_ref, progress_bar_ref, progress_label_ref, force_rescan=True)