# main.py
import os
import sys
from pathlib import Path
import json
import tkinter as tk
from tkinter import ttk, scrolledtext, messagebox
from datetime import datetime

if __name__ == "__main__":
    current_file_path = Path(__file__).resolve()
    core_dir = current_file_path.parent
    project_root = core_dir.parent
    if str(project_root) not in sys.path:
        sys.path.insert(0, str(project_root))

from core.patching import process_input 
from core.treeview_logic import (
    populate_file_tree_threaded, on_tree_click, set_all_tree_check_state,
    update_selected_tokens_display, calculate_tokens_for_selected_threaded
)
from core.treeview_constants import (
    CHECKED_TAG, TRISTATE_TAG,
    BINARY_TAG_UI, LARGE_FILE_TAG_UI, ERROR_TAG_UI, EXCLUDED_BY_DEFAULT_TAG_UI,
    TOO_MANY_TOKENS_TAG_UI
)
from core.gui_utils import (
    create_context_menu, copy_logs, clear_input_field, select_project_dir, clear_logs
)
from core.clipboard_logic import copy_project_files
from core.file_processing import resource_path, initialize_tokenizer
from core.ui_components import LineNumberedText

APP_VERSION = datetime.now().strftime("%y.%m.%d")

root = tk.Tk()
root.title(f"Project Agent v{APP_VERSION}") 
root.geometry("1100x850") 

style = ttk.Style()
if 'clam' in style.theme_names(): 
    style.theme_use('clam')
# Отключаем стандартную подсветку выделения, чтобы работала наша кастомная
style.map("Treeview", 
          background=[('selected', style.lookup("Treeview", "background"))], 
          foreground=[('selected', style.lookup("Treeview", "foreground"))])

icon_path = resource_path("app_icon.ico")
if os.path.exists(icon_path): 
    root.iconbitmap(icon_path)

top_controls_frame = tk.Frame(root)
top_controls_frame.pack(side=tk.TOP, fill=tk.X, pady=(10, 5), padx=10)

dir_selection_frame = tk.Frame(top_controls_frame)
dir_selection_frame.pack(side=tk.TOP, fill=tk.X)
tk.Label(dir_selection_frame, text="Директория проекта:").pack(side=tk.LEFT, padx=(0, 5))
project_dir_entry = tk.Entry(dir_selection_frame, width=70) 
project_dir_entry.pack(side=tk.LEFT, fill=tk.X, expand=True)
browse_button = tk.Button(dir_selection_frame, text="Обзор...") 
browse_button.pack(side=tk.LEFT, padx=(5, 0))
create_context_menu(project_dir_entry) 

progress_frame = tk.Frame(top_controls_frame) 
progress_frame.pack(side=tk.TOP, fill=tk.X, pady=(5, 0))
progress_status_label = tk.Label(progress_frame, text="", anchor="w", width=50) 
progress_status_label.grid(row=0, column=0, sticky="ew", padx=(0, 5))
progress_bar = ttk.Progressbar(progress_frame, orient="horizontal", length=300, mode="determinate") 
progress_bar.grid(row=0, column=1, sticky="ew")
progress_frame.columnconfigure(1, weight=1) 
progress_bar.grid_remove()
progress_status_label.grid_remove()

main_frame = tk.Frame(root)
main_frame.pack(pady=(0, 10), padx=10, fill=tk.BOTH, expand=True)

left_frame = tk.Frame(main_frame)
left_frame.pack(side=tk.LEFT, fill=tk.BOTH, expand=True, padx=(0, 10))

input_area_frame = tk.Frame(left_frame) 
input_area_frame.pack(fill=tk.BOTH, expand=True)
tk.Label(input_area_frame, text="Поле ввода (Markdown, Diff, или JSON):").pack(anchor=tk.W)

input_text_widget = LineNumberedText(
    input_area_frame, wrap=tk.WORD, 
    relief=tk.SUNKEN, borderwidth=1
)
input_text_widget.pack(fill=tk.BOTH, expand=True)
create_context_menu(input_text_widget.text)
input_text_widget.text.bind("<Control-a>", lambda e: input_text_widget.text.event_generate("<<SelectAll>>"))

settings_and_apply_frame = tk.Frame(left_frame) 
settings_and_apply_frame.pack(fill=tk.X, pady=(10, 0))

apply_method_controls_frame = tk.Frame(settings_and_apply_frame) 
apply_method_controls_frame.pack(fill=tk.X)
tk.Label(apply_method_controls_frame, text="Метод применения:").pack(side=tk.LEFT, padx=(0, 5))
apply_method_var = tk.StringVar(value="Markdown") 
apply_method_options = ["Markdown", "Git", "Diff-Match-Patch", "JSON"]
apply_method_menu = ttk.OptionMenu(
    apply_method_controls_frame, apply_method_var, apply_method_var.get(), *apply_method_options
)
apply_method_menu.pack(side=tk.LEFT, padx=(0, 15))

apply_changes_button = tk.Button(
    apply_method_controls_frame, text="Применить изменения",
    command=lambda: process_input(
        input_text_widget.get("1.0", tk.END), 
        project_dir_entry.get(),             
        log_widget,                          
        apply_method_var.get()               
    ),
    width=18, height=1, bg="#90EE90" 
)
apply_changes_button.pack(side=tk.LEFT, padx=5)

clear_input_button = tk.Button(
    apply_method_controls_frame, text="Очистить ввод",
    command=lambda: clear_input_field(input_text_widget, log_widget)
)
clear_input_button.pack(side=tk.LEFT, padx=5)

right_frame = tk.Frame(main_frame, width=500) 
right_frame.pack(side=tk.RIGHT, fill=tk.BOTH, expand=False)
right_frame.pack_propagate(False) 

tk.Label(right_frame, text="Выберите файлы/папки для копирования:").pack(anchor=tk.W, padx=5)

tree_view_container_frame = tk.Frame(right_frame) 
tree_view_container_frame.pack(fill=tk.BOTH, expand=True, pady=(0, 5), padx=5)
tree_scrollbar_y = ttk.Scrollbar(tree_view_container_frame, orient=tk.VERTICAL)
tree_scrollbar_x = ttk.Scrollbar(tree_view_container_frame, orient=tk.HORIZONTAL)

file_tree = ttk.Treeview(
    tree_view_container_frame, 
    columns=('checkbox',),
    yscrollcommand=tree_scrollbar_y.set, 
    xscrollcommand=tree_scrollbar_x.set,
    selectmode="none" 
)
tree_scrollbar_y.pack(side=tk.RIGHT, fill=tk.Y)
tree_scrollbar_x.pack(side=tk.BOTTOM, fill=tk.X)
file_tree.pack(fill=tk.BOTH, expand=True)
tree_scrollbar_y.config(command=file_tree.yview)
tree_scrollbar_x.config(command=file_tree.xview)

file_tree.column('#0', width=350, stretch=tk.YES) 
file_tree.column('checkbox', width=40, stretch=tk.NO, anchor='center')
file_tree.heading('checkbox', text='')

# --- ИЗМЕНЕНИЕ: Возвращаем подсветку цветом ---
file_tree.tag_configure(CHECKED_TAG, background='#A0D2EB') 
file_tree.tag_configure(TRISTATE_TAG, background='#C8E0F4') 
# --- КОНЕЦ ИЗМЕНЕНИЯ ---

file_tree.tag_configure('message', foreground='grey') 
file_tree.tag_configure(ERROR_TAG_UI, foreground='red')
file_tree.tag_configure(BINARY_TAG_UI, foreground='#777777')
file_tree.tag_configure(LARGE_FILE_TAG_UI, foreground='#CC7A00') 
file_tree.tag_configure(EXCLUDED_BY_DEFAULT_TAG_UI, foreground='#006400') 
file_tree.tag_configure(TOO_MANY_TOKENS_TAG_UI, foreground='#8A2BE2') 

selected_tokens_label = tk.Label(right_frame, text="Выделено токенов: 0", anchor=tk.W)
selected_tokens_label.pack(fill=tk.X, pady=(0, 5), padx=5)
file_tree.selected_tokens_label_ref = selected_tokens_label 

file_tree.bind("<Button-1>", lambda event: on_tree_click(event, file_tree, selected_tokens_label))

tree_buttons_frame = tk.Frame(right_frame) 
tree_buttons_frame.pack(fill=tk.X, padx=5)
select_all_button = tk.Button(
    tree_buttons_frame, text="Выбрать все",
    command=lambda: set_all_tree_check_state(file_tree, True, selected_tokens_label)
)
select_all_button.pack(side=tk.LEFT, padx=(0, 5))
deselect_all_button = tk.Button(
    tree_buttons_frame, text="Снять все",
    command=lambda: set_all_tree_check_state(file_tree, False, selected_tokens_label)
)
deselect_all_button.pack(side=tk.LEFT)

calculate_tokens_button = tk.Button(
    tree_buttons_frame, text="Посчитать токены",
    command=lambda: calculate_tokens_for_selected_threaded(
        file_tree, log_widget, progress_bar, progress_status_label
    )
)
calculate_tokens_button.pack(side=tk.LEFT, padx=(5, 0))

copy_options_frame = tk.Frame(right_frame)
copy_options_frame.pack(fill=tk.X, pady=(5,0), padx=5)

include_structure_var = tk.BooleanVar(value=True)
structure_checkbox = tk.Checkbutton(
    copy_options_frame, text="Включить структуру проекта", variable=include_structure_var
)
structure_checkbox.grid(row=0, column=0, sticky=tk.W, columnspan=2) 

structure_type_var = tk.StringVar(value="selected") 
radio_selected_structure = ttk.Radiobutton(
    copy_options_frame, text="Только выделенные", variable=structure_type_var, value="selected"
)
radio_selected_structure.grid(row=1, column=0, sticky=tk.W, padx=(20, 0)) 
radio_all_structure = ttk.Radiobutton(
    copy_options_frame, text="Все файлы проекта", variable=structure_type_var, value="all"
)
radio_all_structure.grid(row=1, column=1, sticky=tk.W, padx=(5,0))

def _toggle_radio_buttons_state():
    state = tk.NORMAL if include_structure_var.get() else tk.DISABLED
    if radio_selected_structure.winfo_exists(): radio_selected_structure.config(state=state)
    if radio_all_structure.winfo_exists(): radio_all_structure.config(state=state)

structure_checkbox.config(command=_toggle_radio_buttons_state)
_toggle_radio_buttons_state() 

include_instructions_var = tk.BooleanVar(value=True)
instructions_checkbox = tk.Checkbutton(
    copy_options_frame, text="Включить инструкции (doc)", variable=include_instructions_var
)
instructions_checkbox.grid(row=2, column=0, sticky=tk.W, columnspan=2, pady=(5,0)) 

copy_to_clipboard_button = tk.Button(
    right_frame, text="Копировать выбранное в буфер",
    command=lambda: copy_project_files(
        project_dir_entry, file_tree, log_widget,
        include_structure_var, structure_type_var, 
        include_instructions_var, apply_method_var 
    ),
    height=2, bg="#AED6F1" 
)
copy_to_clipboard_button.pack(fill=tk.X, pady=(10, 5), padx=5) 

log_frame = tk.Frame(root)
log_frame.pack(pady=(0, 10), padx=10, fill=tk.BOTH, expand=True)
tk.Label(log_frame, text="Лог операций:").pack(anchor=tk.W)
log_widget = scrolledtext.ScrolledText(
    log_frame, height=8, width=80, wrap=tk.WORD, 
    relief=tk.SUNKEN, borderwidth=1, state='normal' 
)
log_widget.pack(fill=tk.BOTH, expand=True)
create_context_menu(log_widget) 

log_widget.tag_config('error', foreground='red')
log_widget.tag_config('warning', foreground='orange') 
log_widget.tag_config('success', foreground='green')
log_widget.tag_config('info', foreground='blue') 

log_buttons_frame = tk.Frame(log_frame)
log_buttons_frame.pack(fill=tk.X, pady=(5, 0))

copy_log_button = tk.Button(log_buttons_frame, text="Копировать лог", command=lambda: copy_logs(log_widget))
copy_log_button.pack(side=tk.RIGHT, padx=(5, 0)) 

clear_log_button = tk.Button(log_buttons_frame, text="Очистить лог", command=lambda: clear_logs(log_widget))
clear_log_button.pack(side=tk.RIGHT)

file_tree.log_widget_ref = log_widget 

browse_button.config(command=lambda: select_project_dir(
    project_dir_entry, file_tree, log_widget, progress_bar, progress_status_label
))

def start_populating_tree(dir_to_scan, is_initial_load=False):
    populate_file_tree_threaded(
        dir_to_scan, file_tree, log_widget, 
        progress_bar, progress_status_label, 
        force_rescan=not is_initial_load 
    )

config_file_path_obj = Path(project_root) / "app_config.json"
if config_file_path_obj.is_file():
    with open(config_file_path_obj, 'r', encoding='utf-8') as f_config:
        config_data = json.load(f_config)
    loaded_last_dir = config_data.get("last_project_dir")
    
    if loaded_last_dir and Path(loaded_last_dir).is_dir():
        project_dir_entry.insert(0, loaded_last_dir)
        root.after(150, lambda: start_populating_tree(loaded_last_dir, is_initial_load=True))
    else: 
        if not file_tree.get_children(""): 
            root.after(150, lambda: start_populating_tree(None, is_initial_load=True)) 
else: 
    if not file_tree.get_children(""): 
        root.after(150, lambda: start_populating_tree(None, is_initial_load=True))

def on_window_closing():
    current_project_dir_str = project_dir_entry.get()
    config_to_save = {"last_project_dir": ""}
    if Path(current_project_dir_str).is_dir(): 
        config_to_save["last_project_dir"] = current_project_dir_str
    
    with open(config_file_path_obj, 'w', encoding='utf-8') as f_config_save:
        json.dump(config_to_save, f_config_save, indent=4)
    
    root.destroy() 

root.protocol("WM_DELETE_WINDOW", on_window_closing)

initialize_tokenizer(log_widget)

if __name__ == "__main__":
    root.mainloop()