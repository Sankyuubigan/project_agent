# gui_utils.py
import tkinter as tk
from tkinter import scrolledtext, filedialog
import pyperclip

def create_context_menu(widget):
    """Создает стандартное контекстное меню."""
    menu = tk.Menu(widget, tearoff=0)
    is_text = isinstance(widget, (tk.Text, scrolledtext.ScrolledText))
    is_entry = isinstance(widget, tk.Entry)

    cmd_cut = lambda: widget.event_generate("<<Cut>>")
    cmd_copy = lambda: widget.event_generate("<<Copy>>")
    cmd_paste = lambda: widget.event_generate("<<Paste>>")
    cmd_select_all = lambda: widget.tag_add(tk.SEL, "1.0", tk.END) if is_text else (widget.select_range(0, tk.END) if is_entry else None)

    menu.add_command(label="Cut", command=cmd_cut, state="disabled")
    menu.add_command(label="Copy", command=cmd_copy, state="disabled")
    menu.add_command(label="Paste", command=cmd_paste, state="disabled")
    menu.add_separator()
    menu.add_command(label="Select All", command=cmd_select_all, state="disabled")

    def update_menu_state():
        is_editable = True
        try: is_editable = widget.cget("state") != "disabled"
        except: pass
        has_selection = False
        try:
            if widget.selection_get(): has_selection = True
        except: pass
        can_paste = False
        try:
            if pyperclip.paste(): can_paste = True
        except: pass

        cut_state = "normal" if has_selection and is_editable else "disabled"
        copy_state = "normal" if has_selection else "disabled"
        paste_state = "normal" if can_paste and is_editable else "disabled"
        select_all_state = "normal" if (is_text or is_entry) else "disabled"
        
        try: menu.entryconfigure("Cut", state=cut_state)
        except tk.TclError: pass
        try: menu.entryconfigure("Copy", state=copy_state)
        except tk.TclError: pass
        try: menu.entryconfigure("Paste", state=paste_state)
        except tk.TclError: pass
        try: menu.entryconfigure("Select All", state=select_all_state)
        except tk.TclError: pass

    def show_context_menu(event):
        update_menu_state()
        menu.tk_popup(event.x_root, event.y_root)

    widget.bind("<Button-3>", show_context_menu)


def copy_logs(log_widget):
    """Копирует текст из лога в буфер обмена."""
    if not log_widget: return 
    try:
        log_text = log_widget.get("1.0", tk.END).strip()
        if log_text:
            pyperclip.copy(log_text)
            log_widget.insert(tk.END, "\nЛоги скопированы!\n", ('success',))
        else:
            log_widget.insert(tk.END, "\nЛоги пусты.\n", ('info',))
    except Exception as e:
        try:
            log_widget.insert(tk.END, f"\nОшибка копирования логов: {e}\n", ('error',))
        except: pass 
    finally:
        try: 
            log_widget.see(tk.END)
        except: pass


def clear_input_field(input_text_widget, log_widget):
    """Очищает поле ввода текста."""
    if input_text_widget:
        input_text_widget.delete("1.0", tk.END)
    if log_widget:
        try:
            log_widget.insert(tk.END, "Поле ввода очищено.\n", ('info',))
        except: pass

def select_project_dir(entry_widget, tree_widget, log_widget_ref, progress_bar_ref, progress_label_ref):
    """Открывает диалог выбора директории и запускает заполнение дерева с принудительным обновлением."""
    from treeview_logic import populate_file_tree_threaded # Отложенный импорт
    dir_path = filedialog.askdirectory()
    if dir_path:
        if entry_widget:
             entry_widget.delete(0, tk.END); entry_widget.insert(0, dir_path)
        if log_widget_ref:
             log_widget_ref.insert(tk.END, f"Выбрана директория (принудительное обновление): {dir_path}\n")
        # Запускаем заполнение с флагом force_rescan=True
        populate_file_tree_threaded(dir_path, tree_widget, log_widget_ref, progress_bar_ref, progress_label_ref, force_rescan=True)