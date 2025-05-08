# apply_changes_gui.py

import os
import sys
import tkinter as tk
from tkinter import ttk, scrolledtext, messagebox  # Добавил messagebox на всякий случай
from pathlib import Path
import json
import traceback  # Для вывода ошибок

print("DEBUG: Standard imports done.")

# Импортируем функции из наших модулей
try:
    from patching import process_input
    from treeview_logic import (
        populate_file_tree_threaded, toggle_check, set_all_tree_check_state,
        CHECKED_TAG, BINARY_TAG, LARGE_FILE_TAG, ERROR_TAG, EXCLUDED_BY_DEFAULT_TAG,
        TOO_MANY_TOKENS_TAG, FOLDER_TAG, FILE_TAG
    )
    from gui_utils import (
        create_context_menu, copy_logs, clear_input_field, select_project_dir
    )
    from clipboard_logic import copy_project_files

    print("DEBUG: Module imports successful.")
except ImportError as import_err:
    error_message = f"CRITICAL ERROR: Failed to import modules: {import_err}\n\n{traceback.format_exc()}"
    print(error_message)
    try:  # Пробуем показать ошибку в GUI
        err_root = tk.Tk();
        err_root.withdraw()
        messagebox.showerror("Import Error", error_message)
    except Exception as e:
        print(f"Could not show error in messagebox: {e}")
    sys.exit(1)
except Exception as general_import_err:  # Ловим и другие ошибки импорта
    error_message = f"CRITICAL ERROR during imports: {general_import_err}\n\n{traceback.format_exc()}"
    print(error_message)
    try:  # Пробуем показать ошибку в GUI
        err_root = tk.Tk();
        err_root.withdraw()
        messagebox.showerror("Import Error", error_message)
    except:
        pass
    sys.exit(1)

# --- Создание GUI ---
# Обернем создание GUI в try-except
try:
    print("DEBUG: Creating root window...")
    root = tk.Tk();
    root.title("Apply Project Changes");
    root.geometry("1100x850")
    style = ttk.Style();
    style.map("Treeview", background=[('selected', '#E0E0E0')], foreground=[('selected', 'black')])
    print("DEBUG: Root window created.")

    # --- Верхняя часть: Выбор директории и Прогресс-бар ---
    top_controls_frame = tk.Frame(root);
    top_controls_frame.pack(side=tk.TOP, fill=tk.X, pady=(10, 5), padx=10)
    dir_selection_frame = tk.Frame(top_controls_frame);
    dir_selection_frame.pack(side=tk.TOP, fill=tk.X)
    tk.Label(dir_selection_frame, text="Project Directory:").pack(side=tk.LEFT, padx=(0, 5))
    project_dir_entry = tk.Entry(dir_selection_frame, width=70);
    project_dir_entry.pack(side=tk.LEFT, fill=tk.X, expand=True)
    browse_button = tk.Button(dir_selection_frame, text="Browse...")  # command привяжем позже
    browse_button.pack(side=tk.LEFT, padx=(5, 0))
    create_context_menu(project_dir_entry)

    progress_frame = tk.Frame(top_controls_frame);
    progress_frame.pack(side=tk.TOP, fill=tk.X, pady=(5, 0))
    progress_status_label = tk.Label(progress_frame, text="", anchor="w", width=50);
    progress_status_label.grid(row=0, column=0, sticky="ew", padx=(0, 5))
    progress_bar = ttk.Progressbar(progress_frame, orient="horizontal", length=300, mode="determinate");
    progress_bar.grid(row=0, column=1, sticky="ew")
    progress_frame.columnconfigure(1, weight=1);
    progress_bar.grid_remove();
    progress_status_label.grid_remove()
    print("DEBUG: Top controls created.")

    # --- Основная рама ---
    main_frame = tk.Frame(root);
    main_frame.pack(pady=(0, 10), padx=10, fill=tk.BOTH, expand=True)

    # --- Левая часть ---
    left_frame = tk.Frame(main_frame);
    left_frame.pack(side=tk.LEFT, fill=tk.BOTH, expand=True, padx=(0, 10))
    input_area_frame = tk.Frame(left_frame);
    input_area_frame.pack(fill=tk.BOTH, expand=True)
    tk.Label(input_area_frame, text="Input (Markdown, Diff, or JSON):").pack(anchor=tk.W)
    input_text_widget = scrolledtext.ScrolledText(input_area_frame, height=15, width=50, wrap=tk.WORD, relief=tk.SUNKEN,
                                                  borderwidth=1);
    input_text_widget.pack(fill=tk.BOTH, expand=True)
    create_context_menu(input_text_widget);
    input_text_widget.bind("<Control-a>", lambda e: input_text_widget.tag_add(tk.SEL, "1.0", tk.END))

    settings_frame = tk.Frame(left_frame);
    settings_frame.pack(fill=tk.X, pady=(10, 0))
    apply_method_frame = tk.Frame(settings_frame);
    apply_method_frame.pack(fill=tk.X)
    tk.Label(apply_method_frame, text="Apply Method:").pack(side=tk.LEFT, padx=(0, 5))
    apply_method_var = tk.StringVar(value="Markdown");
    apply_method_options = ["Markdown", "Git", "Diff-Match-Patch", "JSON"]
    apply_method_menu = ttk.OptionMenu(apply_method_frame, apply_method_var, apply_method_var.get(),
                                       *apply_method_options);
    apply_method_menu.pack(side=tk.LEFT, padx=(0, 15))
    apply_button = tk.Button(apply_method_frame, text="Apply Changes",
                             command=lambda: process_input(input_text_widget.get("1.0", tk.END),
                                                           project_dir_entry.get(), log_widget, apply_method_var.get()),
                             width=15, height=1, bg="#90EE90");
    apply_button.pack(side=tk.LEFT, padx=5)
    clear_button = tk.Button(apply_method_frame, text="Clear Input",
                             command=lambda: clear_input_field(input_text_widget, log_widget));
    clear_button.pack(side=tk.LEFT, padx=5)
    print("DEBUG: Left panel created.")

    # --- Правая часть ---
    right_frame = tk.Frame(main_frame, width=500);
    right_frame.pack(side=tk.RIGHT, fill=tk.BOTH, expand=False);
    right_frame.pack_propagate(False)
    tk.Label(right_frame, text="Select Files/Folders to Copy:").pack(anchor=tk.W, padx=5)
    tree_view_frame = tk.Frame(right_frame);
    tree_view_frame.pack(fill=tk.BOTH, expand=True, pady=(0, 5), padx=5)
    tree_scrollbar_y = ttk.Scrollbar(tree_view_frame, orient=tk.VERTICAL);
    tree_scrollbar_x = ttk.Scrollbar(tree_view_frame, orient=tk.HORIZONTAL)
    file_tree = ttk.Treeview(tree_view_frame, yscrollcommand=tree_scrollbar_y.set, xscrollcommand=tree_scrollbar_x.set,
                             selectmode="none");
    tree_scrollbar_y.pack(side=tk.RIGHT, fill=tk.Y);
    tree_scrollbar_x.pack(side=tk.BOTTOM, fill=tk.X)
    file_tree.pack(fill=tk.BOTH, expand=True);
    tree_scrollbar_y.config(command=file_tree.yview);
    tree_scrollbar_x.config(command=file_tree.xview)

    # Настройка тегов
    print("DEBUG: Configuring tree tags...")
    file_tree.tag_configure(CHECKED_TAG, background='#A0D2EB');
    file_tree.tag_configure('message', foreground='grey');
    file_tree.tag_configure('error', foreground='red', font=('TkDefaultFont', 9, 'italic'))
    file_tree.tag_configure(BINARY_TAG, foreground='#777777', font=('TkDefaultFont', 9, 'italic'));
    file_tree.tag_configure(LARGE_FILE_TAG, foreground='#CC7A00', font=('TkDefaultFont', 9, 'italic'))
    file_tree.tag_configure(EXCLUDED_BY_DEFAULT_TAG, foreground='#006400', font=('TkDefaultFont', 9, 'italic'));
    file_tree.tag_configure(TOO_MANY_TOKENS_TAG, foreground='#8A2BE2', font=('TkDefaultFont', 9, 'italic'))
    file_tree.tag_configure(ERROR_TAG, foreground='red', font=('TkDefaultFont', 9, 'italic'))
    print("DEBUG: Tree tags configured.")

    file_tree.bind("<Button-1>", lambda event: toggle_check(event, file_tree))
    tree_buttons_frame = tk.Frame(right_frame);
    tree_buttons_frame.pack(fill=tk.X, padx=5)
    select_all_button = tk.Button(tree_buttons_frame, text="Select All",
                                  command=lambda: set_all_tree_check_state(file_tree, True));
    select_all_button.pack(side=tk.LEFT, padx=(0, 5))
    deselect_all_button = tk.Button(tree_buttons_frame, text="Deselect All",
                                    command=lambda: set_all_tree_check_state(file_tree, False));
    deselect_all_button.pack(side=tk.LEFT)

    copy_options_frame = tk.Frame(right_frame)
    copy_options_frame.pack(fill=tk.X, pady=(5, 0), padx=5)

    include_structure_var = tk.BooleanVar(value=True)
    structure_checkbox = tk.Checkbutton(copy_options_frame, text="Include project structure",
                                        variable=include_structure_var)
    structure_checkbox.pack(side=tk.LEFT, anchor=tk.W)

    include_instructions_var = tk.BooleanVar(value=True)
    instructions_checkbox = tk.Checkbutton(copy_options_frame, text="Include instructions (doc)",
                                           variable=include_instructions_var)
    instructions_checkbox.pack(side=tk.LEFT, padx=(10, 0), anchor=tk.W)

    copy_button = tk.Button(right_frame, text="Copy Selected to Clipboard",
                            command=lambda: copy_project_files(project_dir_entry,
                                                               file_tree,
                                                               log_widget,
                                                               include_structure_var.get(),
                                                               include_instructions_var.get()),
                            height=2, bg="#AED6F1")
    copy_button.pack(fill=tk.X, pady=(5, 5), padx=5)
    print("DEBUG: Right panel created.")

    # --- Нижняя часть: Лог ---
    log_frame = tk.Frame(root);
    log_frame.pack(pady=(0, 10), padx=10, fill=tk.BOTH, expand=True)
    tk.Label(log_frame, text="Log:").pack(anchor=tk.W)
    log_widget = scrolledtext.ScrolledText(log_frame, height=8, width=80, wrap=tk.WORD, relief=tk.SUNKEN, borderwidth=1,
                                           state='normal');
    log_widget.pack(fill=tk.BOTH, expand=True)
    create_context_menu(log_widget)
    copy_log_button = tk.Button(log_frame, text="Copy Logs", command=lambda: copy_logs(log_widget));
    copy_log_button.pack(pady=(5, 0), anchor=tk.E, padx=(0, 5))
    print("DEBUG: Log panel created.")

    file_tree.log_widget_ref = log_widget
    print("DEBUG: file_tree.log_widget_ref assigned.")

    browse_button.config(command=lambda: select_project_dir(project_dir_entry, file_tree, log_widget, progress_bar,
                                                            progress_status_label))
    print("DEBUG: Browse button command configured.")


    def start_populate(dir_to_populate):
        print(f"PRINT_DEBUG_MAIN: start_populate called for '{dir_to_populate}'")
        try:
            populate_file_tree_threaded(dir_to_populate, file_tree, log_widget, progress_bar, progress_status_label)
            print(f"PRINT_DEBUG_MAIN: populate_file_tree_threaded scheduled/started for '{dir_to_populate}'")
        except Exception as e:
            error_msg_start = f"CRITICAL ERROR: Exception during scheduling/starting populate_file_tree_threaded:\n{traceback.format_exc()}"
            print(error_msg_start)
            try:
                log_widget.insert(tk.END, error_msg_start + "\n")
            except:
                pass


    config_file = Path("app_config.json")
    print(f"DEBUG: Checking config file: {config_file}")
    if config_file.exists():
        print(f"DEBUG: Config file exists. Loading...")
        last_dir = None
        try:
            with open(config_file, 'r', encoding='utf-8') as f:
                config = json.load(f)
            last_dir = config.get("last_project_dir")
            print(f"DEBUG: Last directory from config: {last_dir}")
            if last_dir and os.path.isdir(last_dir):
                project_dir_entry.insert(0, last_dir)
                print(f"DEBUG: Scheduling start_populate for {last_dir} using root.after")
                root.after(100, start_populate, last_dir)
            else:
                print(f"DEBUG: Scheduling start_populate for None (last_dir invalid or not set)")
                if not file_tree.get_children(""):  # ИСПРАВЛЕНО: tree -> file_tree
                    root.after(100, lambda: populate_file_tree_threaded(None, file_tree, log_widget, progress_bar,
                                                                        progress_status_label))
        except Exception as e:
            print(f"ERROR: Could not load config: {e}")
            if not file_tree.get_children(""):  # ИСПРАВЛЕНО: tree -> file_tree
                root.after(100, lambda: populate_file_tree_threaded(None, file_tree, log_widget, progress_bar,
                                                                    progress_status_label))
    else:
        print(f"DEBUG: Config file does not exist. Scheduling populate for None (to show message).")
        if not file_tree.get_children(""):  # ИСПРАВЛЕНО: tree -> file_tree
            root.after(100, lambda: populate_file_tree_threaded(None, file_tree, log_widget, progress_bar,
                                                                progress_status_label))


    def on_closing():
        last_dir = project_dir_entry.get();
        config = {"last_project_dir": last_dir if os.path.isdir(last_dir) else ""}
        try:
            with open(config_file, 'w', encoding='utf-8') as f:
                json.dump(config, f, indent=4)
        except Exception as e:
            print(f"Warning: Could not save config: {e}")
        root.destroy()


    root.protocol("WM_DELETE_WINDOW", on_closing)

    try:
        import tiktoken; print("DEBUG: tiktoken found")
    except ImportError:
        print("WARNING: tiktoken not found"); log_widget.insert(tk.END, "ПРЕДУПРЕЖДЕНИЕ: tiktoken не найден...\n",
                                                                ('error',))
    try:
        import gitignore_parser; print("DEBUG: gitignore-parser found")
    except ImportError:
        print("WARNING: gitignore-parser not found"); log_widget.insert(tk.END,
                                                                        "ПРЕДУПРЕЖДЕНИЕ: gitignore-parser не найден...\n",
                                                                        ('error',))

    print("DEBUG: Starting mainloop...")
    root.mainloop()
    print("DEBUG: Mainloop finished.")

except Exception as gui_creation_error:
    error_message = f"CRITICAL ERROR during GUI creation:\n{gui_creation_error}\n\n{traceback.format_exc()}"
    print(error_message)
    try:
        err_root = tk.Tk();
        err_root.withdraw()
        messagebox.showerror("GUI Creation Error", error_message)
    except:
        pass
    sys.exit(1)