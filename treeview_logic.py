# treeview_logic.py
import os
import tkinter as tk
from tkinter import ttk
from pathlib import Path
import fnmatch
import threading
import queue
import sys
import time
import traceback
import stat

from file_processing import (
    BINARY_EXTENSIONS, MAX_FILE_SIZE_BYTES, MAX_TOKENS_FOR_DISPLAY,
    count_file_tokens, resource_path
)
try: from gitignore_parser import parse_gitignore
except ImportError: parse_gitignore = None

GLOBAL_IGNORED_DIRS = {'.git', '__pycache__', '.vscode', '.idea', 'node_modules', 'venv', '.env', 'build', 'dist', 'out', 'target', '.pytest_cache', '.mypy_cache', '.tox', '*.egg-info'}
GLOBAL_IGNORED_FILES = {'.DS_Store', 'Thumbs.db', 'desktop.ini', '*.pyc', '*.pyo', '*.pyd', '*.so', '*.dll', '*.log', '*.tmp', '*.bak', '*.swp', '*.swo', '.coverage'}
EXCLUDED_BY_DEFAULT_PATTERNS = {'poetry.lock', 'pnpm-lock.yaml', 'yarn.lock', 'package-lock.json', 'Pipfile.lock', '.eslintcache', '*.min.js', '*.min.css', 'LICENSE', 'LICENSE.*', 'COPYING', 'NOTICE', '*.ipynb_checkpoints*', 'go.sum'}
CHECKED_TAG = "checked"; FOLDER_TAG = "folder"; FILE_TAG = "file"
PARTIALLY_CHECKED_TAG = "partially_checked" # Новый тег для частичного выделения
BINARY_TAG = "binary"; LARGE_FILE_TAG = "large_file"; ERROR_TAG = "error_status"
EXCLUDED_BY_DEFAULT_TAG = "excluded_default"; TOO_MANY_TOKENS_TAG = "too_many_tokens"
DISABLED_LOOK_TAGS = {BINARY_TAG, LARGE_FILE_TAG, ERROR_TAG, EXCLUDED_BY_DEFAULT_TAG, TOO_MANY_TOKENS_TAG}


tree_item_paths = {}; tree_item_data = {}; gitignore_matcher = None
populate_thread = None; update_queue = queue.Queue()
gui_queue_processor_running = False; last_processed_dir = None
gui_update_call_counter = 0

def should_fully_exclude(item_path_obj: Path, root_dir_obj: Path, item_name: str, is_dir: bool):
    if item_name in GLOBAL_IGNORED_DIRS and is_dir: return True
    if item_name in GLOBAL_IGNORED_FILES and not is_dir: return True
    if gitignore_matcher and gitignore_matcher(item_path_obj): return True
    return False

def get_item_status_tags_and_info(item_path_obj: Path, item_name: str, is_dir: bool, log_widget_ref):
    tags = [FOLDER_TAG if is_dir else FILE_TAG]
    status_msg = ""
    token_count = 0 

    if is_dir:
        return tags, "", 0 

    if item_path_obj.suffix.lower() in BINARY_EXTENSIONS:
        tags.append(BINARY_TAG); status_msg = "бинарный"
        token_count = None 
    else:
        try:
            file_size = item_path_obj.stat().st_size
            if file_size > MAX_FILE_SIZE_BYTES:
                tags.append(LARGE_FILE_TAG); status_msg = f"> {MAX_FILE_SIZE_BYTES // (1024*1024)}MB"
                token_count = None 
            else:
                token_count_val, token_error = count_file_tokens(str(item_path_obj), log_widget_ref)
                
                if token_error:
                    status_msg = token_error
                    token_count = None 
                    if "бинарный" in token_error and BINARY_TAG not in tags: tags.append(BINARY_TAG)
                    elif "tiktoken" in token_error and ERROR_TAG not in tags: tags.append(ERROR_TAG)
                elif token_count_val is not None: 
                    token_count = token_count_val
                    if token_count > MAX_TOKENS_FOR_DISPLAY:
                        tags.append(TOO_MANY_TOKENS_TAG)
                        formatted_max_tokens = f"{MAX_TOKENS_FOR_DISPLAY:,}".replace(",", " ")
                        if not status_msg : status_msg = f"токенов > {formatted_max_tokens}"
                        else: status_msg += f", токенов > {formatted_max_tokens}"
        except OSError as e:
            tags.append(ERROR_TAG); status_msg = f"ошибка доступа: {e.strerror}"
            token_count = None
        except Exception as e: 
            tags.append(ERROR_TAG); status_msg = f"ошибка: {str(e)[:30]}"
            token_count = None

    for pattern in EXCLUDED_BY_DEFAULT_PATTERNS:
        if fnmatch.fnmatch(item_name, pattern):
            if EXCLUDED_BY_DEFAULT_TAG not in tags:
                tags.append(EXCLUDED_BY_DEFAULT_TAG)
            if not status_msg: status_msg = "исключен по умолчанию"
            break
    return tags, status_msg, token_count


def _update_tree_item_display(tree, item_id, item_name, token_count, status_msg, is_dir):
    display_name = item_name
    if is_dir and token_count is not None and token_count > 0:
        formatted_tokens = f"{token_count:,}".replace(",", " ")
        display_name += f" ({formatted_tokens} токенов)"
    elif not is_dir and token_count is not None and token_count > 0 and TOO_MANY_TOKENS_TAG not in tree.item(item_id, 'tags'):
        formatted_tokens = f"{token_count:,}".replace(",", " ")
        display_name += f" ({formatted_tokens} токенов)"
    
    if status_msg:
        if not ( (is_dir or (not is_dir and TOO_MANY_TOKENS_TAG not in tree.item(item_id, 'tags'))) and \
                 token_count is not None and token_count > 0 and "токенов" in status_msg.lower()):
            display_name += f" [{status_msg}]"
        elif token_count is None and "токенов" in status_msg.lower() : 
            display_name += f" [{status_msg}]"
    try:
        if tree.exists(item_id):
            tree.item(item_id, text=display_name)
    except tk.TclError:
        pass 


def update_selected_tokens_display(tree, label_widget): 
    if not label_widget or not hasattr(label_widget, 'config'):
        return
    total_selected_tokens = 0
    all_ids = []
    def _collect_all_ids_recursive(parent_id=""):
        try: children = tree.get_children(parent_id)
        except tk.TclError: children = []
        for child_id in children:
            if tree.exists(child_id):
                all_ids.append(child_id)
                _collect_all_ids_recursive(child_id)
    try: _collect_all_ids_recursive()
    except tk.TclError: return 

    for item_id in all_ids:
        if not tree.exists(item_id) or item_id not in tree_item_data:
            continue
        try:
            item_tags = set(tree.item(item_id, 'tags'))
            item_data = tree_item_data[item_id]
            # Токены суммируются, если элемент выбран (CHECKED_TAG) ИЛИ частично выбран (PARTIALLY_CHECKED_TAG для папок)
            # и при этом он является файлом и не отключен по другим причинам.
            # Папки сами по себе не добавляют токены в этот счетчик, их токены - это сумма файлов.
            if (CHECKED_TAG in item_tags or PARTIALLY_CHECKED_TAG in item_tags) and \
               not DISABLED_LOOK_TAGS.intersection(item_tags) and \
               item_data.get('is_file'):
                tokens = item_data.get('tokens', 0) 
                if isinstance(tokens, (int, float)) and tokens > 0:
                    total_selected_tokens += tokens
        except tk.TclError: continue 
            
    try:
        formatted_tokens = f"{total_selected_tokens:,}".replace(",", " ")
        label_widget.config(text=f"Выделено токенов: {formatted_tokens}")
    except tk.TclError: pass 
    except Exception: pass


def _process_tree_updates(tree, progress_bar, progress_label, log_widget_ref):
    global populate_thread, update_queue, gui_queue_processor_running, gui_update_call_counter
    if not gui_queue_processor_running:
        return

    gui_update_call_counter += 1
    items_processed_this_cycle = 0; max_items = 150 # Немного увеличил для ускорения
    log_prefix_gui = "LOG_GUI: "

    try:
        while items_processed_this_cycle < max_items:
            try:
                action, data = update_queue.get_nowait()
                items_processed_this_cycle += 1
                item_name_for_log = "N/A" 
                if action == "add_node" and isinstance(data, tuple) and len(data) > 2:
                    item_name_for_log = data[2] 
                elif action == "update_node_text" and isinstance(data, tuple) and len(data) > 1:
                    item_name_for_log = data[1] 
                
                try:
                    if action == "clear_tree":
                        if log_widget_ref: log_widget_ref.insert(tk.END, log_prefix_gui + "Очистка дерева...\n")
                        for item in tree.get_children(""): tree.delete(item)
                        tree_item_paths.clear(); tree_item_data.clear()
                    elif action == "progress_start":
                        if log_widget_ref: log_widget_ref.insert(tk.END, f"{log_prefix_gui}Начало прогресса...\n")
                        progress_bar.config(mode='indeterminate', maximum=100); progress_bar.start(10)
                        progress_label.config(text="Сканирование..."); progress_bar.grid(); progress_label.grid()
                    elif action == "progress_step":
                        label_text = data if isinstance(data, str) else '???'
                        progress_label.config(text=f"{label_text[:45]}...")
                    elif action == "add_node":
                        parent_id, item_id, text_from_thread_name_only, tags_tuple, path_str, data_dict = data
                        if (parent_id == "" or tree.exists(parent_id)) and not tree.exists(item_id):
                             tree.insert(parent_id, tk.END, iid=item_id, text=text_from_thread_name_only, open=False, tags=tags_tuple)
                             tree_item_paths[item_id] = path_str
                             tree_item_data[item_id] = data_dict 
                             _update_tree_item_display(tree, item_id,
                                                       data_dict['name_only'],
                                                       data_dict.get('tokens'), 
                                                       data_dict.get('status_msg',''),
                                                       data_dict.get('is_dir', False))
                    elif action == "update_node_text": 
                         item_id, name, tokens, status, is_dir_flag = data
                         # if log_widget_ref: # Убираем избыточный лог, оставим только для ошибок
                         #     log_widget_ref.insert(tk.END, f"{log_prefix_gui}Обновление узла '{name}': токены={tokens}, статус='{status}', папка={is_dir_flag}\n")
                         if tree.exists(item_id):
                             _update_tree_item_display(tree, item_id, name, tokens, status, is_dir_flag)
                             if item_id in tree_item_data: 
                                 tree_item_data[item_id]['tokens'] = tokens
                                 tree_item_data[item_id]['status_msg'] = status
                             # else: # Этот лог теперь не нужен, т.к. проблема была не здесь
                             #    if log_widget_ref: log_widget_ref.insert(tk.END, f"{log_prefix_gui}ПРЕДУПРЕЖДЕНИЕ: Узел '{name}' (ID: {Path(item_id).name}) не найден в tree_item_data при попытке update_node_text.\n")
                    elif action == "finished":
                        if log_widget_ref: log_widget_ref.insert(tk.END, f"{log_prefix_gui}Сигнал 'finished' получен.\n")
                        progress_bar.stop(); progress_bar.config(mode='determinate', value=0)
                        if progress_bar.winfo_ismapped(): progress_bar.grid_remove()
                        if progress_label.winfo_ismapped(): progress_label.grid_remove()
                        
                        label_ref_for_finish = getattr(tree, 'selected_tokens_label_ref', None)
                        root_items = tree.get_children("")
                        if root_items:
                            set_all_tree_check_state(tree, True, label_ref_for_finish) # Это обновит и счетчик токенов
                        elif label_ref_for_finish: 
                            update_selected_tokens_display(tree, label_ref_for_finish)
                            
                        if log_widget_ref: log_widget_ref.insert(tk.END, f"{log_prefix_gui}Заполнение дерева завершено.\n"); log_widget_ref.see(tk.END)
                        gui_queue_processor_running = False
                        return 
                    elif action == "log_message":
                         if log_widget_ref:
                            try:
                                log_widget_ref.insert(tk.END, f"{data}\n")
                            except tk.TclError: pass 
                except tk.TclError as e:
                    if log_widget_ref: log_widget_ref.insert(tk.END, f"{log_prefix_gui}TclError ({action}, item: '{item_name_for_log}'): {e}\n", ('error',))
                except Exception as e:
                    if log_widget_ref: log_widget_ref.insert(tk.END, f"{log_prefix_gui}Ошибка ({action}, item: '{item_name_for_log}'): {e}\n{traceback.format_exc()}\n", ('error',))
            except queue.Empty:
                break 
    finally:
        if gui_queue_processor_running: 
            tree.after(50, lambda: _process_tree_updates(tree, progress_bar, progress_label, log_widget_ref))
        else: 
            if progress_bar.winfo_ismapped(): 
                 progress_bar.stop(); progress_bar.grid_remove(); progress_label.grid_remove()
            # if log_widget_ref: log_widget_ref.insert(tk.END, f"{log_prefix_gui}Обработчик очереди остановлен (по флагу).\n") # Убрано
            label_ref_final = getattr(tree, 'selected_tokens_label_ref', None)
            if label_ref_final:
                update_selected_tokens_display(tree, label_ref_final)


def populate_file_tree_threaded(directory_path_str, tree, log_widget_ref, progress_bar_ref, progress_label_ref):
    global populate_thread, tree_item_paths, tree_item_data, gitignore_matcher, gui_queue_processor_running, last_processed_dir, gui_update_call_counter
    log_prefix_main = "LOG_POPULATE: "

    if populate_thread and populate_thread.is_alive():
        if log_widget_ref: log_widget_ref.insert(tk.END, f"{log_prefix_main}Процесс заполнения уже запущен. Ожидание...\n");
        return

    current_normalized_path = ""; valid_path = False
    if directory_path_str and os.path.isdir(directory_path_str):
        try: current_normalized_path = str(Path(directory_path_str).resolve()); valid_path = True
        except: pass

    if valid_path and current_normalized_path == last_processed_dir and tree.get_children(""):
        if log_widget_ref: log_widget_ref.insert(tk.END, f"{log_prefix_main}Директория '{Path(directory_path_str).name}' уже отображена.\n"); return

    while not update_queue.empty():
        try: update_queue.get_nowait()
        except queue.Empty: break
    # if log_widget_ref: log_widget_ref.insert(tk.END, f"{log_prefix_main}Очередь GUI очищена.\n") # Убрано

    tree_item_paths.clear(); tree_item_data.clear(); gitignore_matcher = None
    last_processed_dir = current_normalized_path if valid_path else None 
    gui_update_call_counter = 0
    update_queue.put(("clear_tree", None)) 

    if not valid_path:
        if directory_path_str is not None: 
             msg_text = f"Ошибка: '{directory_path_str}' не директория."
        else: 
             msg_text = "Выберите корректную директорию проекта."
        update_queue.put(("add_node", ("", "msg_node", msg_text, ('message',), "", {'name_only': msg_text, 'is_dir': False, 'is_file': False, 'status_msg': '', 'tokens': 0})))
        update_queue.put(("finished", None)) 
    
    if not gui_queue_processor_running:
        gui_queue_processor_running = True
        tree.after_idle(lambda: _process_tree_updates(tree, progress_bar_ref, progress_label_ref, log_widget_ref))

    if valid_path: 
        populate_thread = threading.Thread(target=_populate_file_tree_actual,
                                           args=(directory_path_str, log_widget_ref),
                                           daemon=True, name=f"PopulateTree-{time.strftime('%H%M%S')}")
        try:
            populate_thread.start()
        except Exception as thread_start_err:
             if log_widget_ref: log_widget_ref.insert(tk.END, f"{log_prefix_main}КРИТ.ОШИБКА: Не удалось запустить поток: {thread_start_err}\n", ('error',))
             update_queue.put(("log_message", f"{log_prefix_main}КРИТ.ОШИБКА: Не удалось запустить поток: {thread_start_err}"))
             update_queue.put(("finished", None)) 


def _populate_file_tree_actual(directory_path_str, log_widget_ref):
    global gitignore_matcher, update_queue
    thread_name = threading.current_thread().name
    log_prefix = f"LOG_THREAD ({thread_name}): " # Оставим для отладки потока, если что
    try:
        # update_queue.put(("log_message", f"{log_prefix}Поток стартовал для '{directory_path_str}'.")) # Убрано
        root_dir_obj = Path(directory_path_str).resolve()
        # update_queue.put(("log_message", f"{log_prefix}Путь разрешен: {root_dir_obj}")) # Убрано

        # update_queue.put(("log_message", f"{log_prefix}Загрузка .gitignore...")) # Убрано
        gitignore_matcher = None
        if parse_gitignore:
            gitignore_file = root_dir_obj / ".gitignore"
            if gitignore_file.is_file():
                try: 
                    gitignore_matcher = parse_gitignore(gitignore_file)
                    # update_queue.put(("log_message", f"{log_prefix}.gitignore загружен.")) # Убрано
                except Exception as e: 
                    update_queue.put(("log_message", f"{log_prefix}ПРЕДУПРЕЖДЕНИЕ: Ошибка разбора .gitignore: {e}"))
            # else: # Убираем сообщение, если .gitignore не найден - это нормальная ситуация
            #     update_queue.put(("log_message", f"{log_prefix}Файл .gitignore не найден в {root_dir_obj}."))
        # else:  # Это уже логируется в GUI при старте
        #     update_queue.put(("log_message", f"{log_prefix}ПРЕДУПРЕЖДЕНИЕ: gitignore-parser не найден."))

        update_queue.put(("progress_start", 1)) 
        dir_name = root_dir_obj.name; root_node_id = str(root_dir_obj)
        
        root_data = {'tokens': 0, 'status_msg': "", 'is_dir': True, 'is_file': False, 'rel_path': "", 'name_only': dir_name}
        update_queue.put(("add_node", ("", root_node_id, dir_name, (FOLDER_TAG, CHECKED_TAG), str(root_dir_obj), root_data)))
        
        folder_tokens = _populate_recursive_threaded(root_dir_obj, root_node_id, root_dir_obj, log_widget_ref)
        # update_queue.put(("log_message", f"{log_prefix}Корневая папка '{dir_name}' получила токенов: {folder_tokens}")) # Убрано

        if isinstance(folder_tokens, (int, float)) and folder_tokens >=0:
            update_queue.put(("update_node_text", (root_node_id, dir_name, folder_tokens, "", True))) 
        else: 
            update_queue.put(("log_message", f"{log_prefix}ПРЕДУПРЕЖДЕНИЕ: Для корневой папки '{dir_name}' токены не были числом: {folder_tokens}."))
            update_queue.put(("update_node_text", (root_node_id, dir_name, 0, "ошибка подсчета токенов", True)))

    except Exception as e:
        error_msg = f"Критическая ошибка в потоке {thread_name} при обработке '{directory_path_str}': {str(e)}"
        update_queue.put(("log_message", f"LOG_THREAD_ERROR: {error_msg}\n{traceback.format_exc()}"))
        try: update_queue.put(("add_node", ("", f"error_root_{thread_name}", error_msg[:100], ('error',), directory_path_str, {'name_only': "Ошибка потока", 'is_dir': False, 'is_file': False, 'status_msg':error_msg[:100], 'tokens':0})))
        except: pass
    finally:
        # update_queue.put(("log_message", f"{log_prefix}Завершение работы потока.")) # Убрано
        update_queue.put(("finished", None)) 


def _populate_recursive_threaded(current_dir_obj, parent_id_str, root_dir_obj, log_widget_ref):
    total_tokens_in_folder = 0
    current_dir_name_for_log = current_dir_obj.name
    log_prefix_thread_rec = f"LOG_REC_THR ({threading.current_thread().name}, CWD: {current_dir_name_for_log}): "
    update_queue.put(("progress_step", current_dir_obj.name)) 

    try:
        items = []
        if not current_dir_obj.is_dir(): 
            status_msg_for_parent = f"Ошибка: '{current_dir_name_for_log}' не директория или нет доступа"
            # update_queue.put(("log_message", f"{log_prefix_thread_rec}{status_msg_for_parent}")) # Уже логируется в GUI
            update_queue.put(("update_node_text", (parent_id_str, current_dir_name_for_log, 0, status_msg_for_parent, False)))
            return 0 

        for item in current_dir_obj.iterdir(): items.append(item)
        items.sort(key=lambda x: (not x.is_dir(), x.name.lower())) 

        for item_path_obj in items:
            item_name = item_path_obj.name
            is_dir = item_path_obj.is_dir()
            item_id_str = str(item_path_obj.resolve()) 

            if should_fully_exclude(item_path_obj, root_dir_obj, item_name, is_dir):
                continue

            tags_list, status_msg, file_tokens_for_item = get_item_status_tags_and_info(item_path_obj, item_name, is_dir, log_widget_ref)
            
            if not DISABLED_LOOK_TAGS.intersection(tags_list):
                tags_list.append(CHECKED_TAG)

            rel_path = str(item_path_obj.relative_to(root_dir_obj))
            data_dict = {
                'tokens': file_tokens_for_item if not is_dir else 0, 
                'status_msg': status_msg,
                'is_dir': is_dir,
                'is_file': not is_dir,
                'rel_path': rel_path,
                'name_only': item_name
            }
            
            update_queue.put(("add_node", (parent_id_str, item_id_str, item_name, tuple(tags_list), str(item_path_obj), data_dict)))

            if is_dir:
                tokens_from_subdir = _populate_recursive_threaded(item_path_obj, item_id_str, root_dir_obj, log_widget_ref)
                
                if isinstance(tokens_from_subdir, (int,float)) and tokens_from_subdir >= 0:
                    total_tokens_in_folder += tokens_from_subdir
                    current_folder_status_for_update = data_dict.get('status_msg', "") 
                    # update_queue.put(("log_message", f"{log_prefix_thread_rec}ОТПРАВКА update_node_text для папки '{item_name}' с токенами: {tokens_from_subdir}")) # Убрано
                    update_queue.put(("update_node_text", (item_id_str, item_name, tokens_from_subdir, current_folder_status_for_update, True)))
                # else: # Логирование некорректных токенов происходит в count_file_tokens или GUI
                    # update_queue.put(("log_message", f"{log_prefix_thread_rec}ПРЕДУПРЕЖДЕНИЕ: Подпапка '{item_name}' вернула некорректные токены: {tokens_from_subdir}."))
            
            elif file_tokens_for_item is not None and isinstance(file_tokens_for_item, (int, float)) and file_tokens_for_item >=0:
                total_tokens_in_folder += file_tokens_for_item
        
    except PermissionError:
        perm_error_msg = f"Отказ в доступе к '{current_dir_name_for_log}'"
        update_queue.put(("log_message", f"{log_prefix_thread_rec}ПРЕДУПРЕЖДЕНИЕ: {perm_error_msg}"))
        item_id_of_current_dir = str(current_dir_obj.resolve())
        update_queue.put(("update_node_text", (item_id_of_current_dir, current_dir_obj.name, 0, perm_error_msg, True)))
    except Exception as e:
        gen_error_msg = f"Ошибка при обработке '{current_dir_name_for_log}': {e}"
        update_queue.put(("log_message", f"{log_prefix_thread_rec}{gen_error_msg}\n{traceback.format_exc()}"))
        item_id_of_current_dir = str(current_dir_obj.resolve())
        update_queue.put(("update_node_text", (item_id_of_current_dir, current_dir_obj.name, 0, str(e)[:50], True)))
    
    # update_queue.put(("log_message", f"{log_prefix_thread_rec}Завершение обхода для '{current_dir_name_for_log}'. Возвращено токенов: {total_tokens_in_folder}")) # Убрано
    return total_tokens_in_folder


def generate_project_structure_text(tree, root_dir_path_str, log_widget_ref): 
    if not root_dir_path_str or not os.path.isdir(root_dir_path_str):
        if log_widget_ref: log_widget_ref.insert(tk.END, "Структура: Корневая директория не задана или не существует.\n")
        return "Структура не сгенерирована: неверная корневая директория."

    structure_lines = ["Структура проекта:"]
    root_path_obj = Path(root_dir_path_str).resolve()
    paths_in_structure = set() 

    def _generate_recursive(item_id, indent_level):
        if not tree.exists(item_id) or item_id not in tree_item_data or item_id not in tree_item_paths:
            return

        item_info = tree_item_data[item_id]
        item_path_str = tree_item_paths[item_id] 
        current_path_obj = Path(item_path_str).resolve()
        
        try:
            if current_path_obj == root_path_obj: 
                rel_path_display = Path(item_info.get('name_only', current_path_obj.name))
            elif current_path_obj.is_relative_to(root_path_obj):
                rel_path_display = Path(root_path_obj.name) / current_path_obj.relative_to(root_path_obj)
            else: 
                 rel_path_display = Path(item_info.get('name_only', current_path_obj.name))
        except ValueError: 
            rel_path_display = Path(item_info.get('name_only', current_path_obj.name)) 

        rel_path_posix = rel_path_display.as_posix()

        if rel_path_posix in paths_in_structure: return 
        paths_in_structure.add(rel_path_posix)
        
        prefix = '    ' * indent_level
        entry = f"{prefix}- {rel_path_posix}"
        
        # Для структуры важен CHECKED_TAG или PARTIALLY_CHECKED_TAG
        current_item_tags = set(tree.item(item_id, 'tags'))
        item_is_fully_or_partially_checked = bool({CHECKED_TAG, PARTIALLY_CHECKED_TAG}.intersection(current_item_tags))


        if item_info.get('is_dir'):
            entry_suffix = "/"
            # Показываем сводку, если папка не выбрана (ни полностью, ни частично)
            should_show_summary = not item_is_fully_or_partially_checked

            if should_show_summary:
                num_files = 0; num_subdirs = 0
                try:
                    direct_children_ids = tree.get_children(item_id)
                    for child_id_for_count in direct_children_ids:
                        if tree.exists(child_id_for_count) and child_id_for_count in tree_item_data:
                            if tree_item_data[child_id_for_count].get('is_dir'):
                                num_subdirs += 1
                            elif tree_item_data[child_id_for_count].get('is_file'):
                                num_files += 1
                    entry_suffix += f" (Файлов: {num_files}; папок: {num_subdirs})"
                except tk.TclError: 
                    entry_suffix += " (ошибка подсчета содержимого)"

            structure_lines.append(entry + entry_suffix)

            # Рекурсируем, только если текущая папка выбрана (полностью или частично)
            if item_is_fully_or_partially_checked: 
                try:
                    children_ids = tree.get_children(item_id)
                    sorted_children_ids = sorted(children_ids, key=lambda cid: (
                        not (tree.exists(cid) and tree_item_data.get(cid, {}).get('is_dir', False)),
                        tree_item_data.get(cid, {}).get('name_only', '').lower()
                    ))
                    for child_id in sorted_children_ids:
                        _generate_recursive(child_id, indent_level + 1)
                except tk.TclError: pass 
        
        else: 
            structure_lines.append(entry)
            
    root_ids = tree.get_children("") 
    if not root_ids:
        if log_widget_ref: log_widget_ref.insert(tk.END, "Структура: Дерево пусто.\n")
        return "Структура не сгенерирована: дерево пусто."

    sorted_root_ids = sorted(root_ids, key=lambda cid: (
        not (tree.exists(cid) and tree_item_data.get(cid, {}).get('is_dir', False)),
        tree_item_data.get(cid, {}).get('name_only', '').lower()
    ))

    for root_id in sorted_root_ids:
        _generate_recursive(root_id, 0) 

    if len(structure_lines) <= 1: 
        if log_widget_ref: log_widget_ref.insert(tk.END, "Структура: Нет элементов для отображения.\n")
        return "Структура не сгенерирована: нет элементов для отображения."
    return "\n".join(structure_lines)


def toggle_check(event, tree, selected_tokens_label_ref): 
    item_id = tree.identify_row(event.y)
    if not item_id or not tree.exists(item_id): return 

    region = tree.identify("region", event.x, event.y)
    element_clicked = tree.identify_element(event.x, event.y)
    
    # log_widget_ref_debug = getattr(tree, 'log_widget_ref', None) # Убрано
    # if log_widget_ref_debug: # Убрано
    #     try:
    #         item_name_debug = tree_item_data.get(item_id, {}).get('name_only', Path(item_id).name)
    #         log_widget_ref_debug.insert(tk.END, f"DEBUG_CLICK_V2: Клик по '{item_name_debug}', region='{region}', element='{element_clicked}'.\n")
    #     except: pass

    if region == "indicator" or (isinstance(element_clicked, str) and ".indicator" in element_clicked):
        # if log_widget_ref_debug: # Убрано
            # try:
                # item_name_debug = tree_item_data.get(item_id, {}).get('name_only', Path(item_id).name)
                # log_widget_ref_debug.insert(tk.END, f"DEBUG_CLICK_V2: Клик по индикатору для '{item_name_debug}' подтвержден. Изменение чекбокса пропущено.\n")
            # except: pass
        return 

    current_tags = set(tree.item(item_id, 'tags'))
    is_disabled_by_look = bool(DISABLED_LOOK_TAGS.intersection(current_tags))

    if is_disabled_by_look:
        log_widget_ref = getattr(tree, 'log_widget_ref', None)
        if log_widget_ref:
             try: 
                 item_name = tree_item_data.get(item_id,{}).get('name_only',item_id)
                 disabled_tags_str = ', '.join(DISABLED_LOOK_TAGS.intersection(current_tags))
                 log_widget_ref.insert(tk.END, f"ИНФО: Элемент '{item_name}' не может быть выбран (статус: {disabled_tags_str}).\n")
             except: pass
        return
    
    # Если элемент был частично выбран, а мы кликаем на него, он должен стать полностью выбранным.
    # Если он был полностью выбран, он становится невыбранным.
    # Если он не был выбран, он становится полностью выбранным.
    new_state_is_checked = True
    if CHECKED_TAG in current_tags:
        new_state_is_checked = False
    # PARTIALLY_CHECKED_TAG сам по себе не переключается кликом по родительскому элементу,
    # он является результатом состояния дочерних.

    set_check_state_recursive(tree, item_id, new_state_is_checked, selected_tokens_label_ref)
    _update_parent_check_state_recursive(tree, item_id, selected_tokens_label_ref)
    
    if selected_tokens_label_ref: 
        update_selected_tokens_display(tree, selected_tokens_label_ref)


def set_check_state_recursive(tree, item_id, state, selected_tokens_label_ref):
    if not tree.exists(item_id): return

    current_tags = set(tree.item(item_id, 'tags'))
    is_disabled_by_look = bool(DISABLED_LOOK_TAGS.intersection(current_tags))
    
    # Снимаем предыдущие теги выделения
    current_tags.discard(CHECKED_TAG)
    current_tags.discard(PARTIALLY_CHECKED_TAG)

    if not is_disabled_by_look: 
        if state: 
            current_tags.add(CHECKED_TAG)
        # Если state is False, то оба тега (CHECKED_TAG и PARTIALLY_CHECKED_TAG) уже сняты.
    
    tree.item(item_id, tags=tuple(current_tags))

    try:
        children = tree.get_children(item_id)
        for child_id in children:
            set_check_state_recursive(tree, child_id, state, selected_tokens_label_ref)
    except tk.TclError: pass 


def _update_parent_check_state_recursive(tree, item_id, selected_tokens_label_ref):
    if not item_id or not tree.exists(item_id): return
    
    parent_id = tree.parent(item_id)
    if not parent_id or not tree.exists(parent_id): return

    parent_tags_set = set(tree.item(parent_id, 'tags'))
    if DISABLED_LOOK_TAGS.intersection(parent_tags_set):
        _update_parent_check_state_recursive(tree, parent_id, selected_tokens_label_ref)
        return

    all_children_ids = []
    try: all_children_ids = tree.get_children(parent_id)
    except tk.TclError: return 

    if not all_children_ids:
        parent_tags_set.discard(CHECKED_TAG)
        parent_tags_set.discard(PARTIALLY_CHECKED_TAG)
        tree.item(parent_id, tags=tuple(parent_tags_set))
        _update_parent_check_state_recursive(tree, parent_id, selected_tokens_label_ref)
        return

    all_active_children_fully_checked = True
    any_active_child_checked_or_partially = False # Включает и частично выбранные подпапки
    has_active_children = False

    for child_id in all_children_ids:
        if not tree.exists(child_id): continue
        child_tags = set(tree.item(child_id, 'tags'))
        if not DISABLED_LOOK_TAGS.intersection(child_tags):
            has_active_children = True
            if CHECKED_TAG not in child_tags and PARTIALLY_CHECKED_TAG not in child_tags: # Не выбран никак
                all_active_children_fully_checked = False
            if CHECKED_TAG in child_tags or PARTIALLY_CHECKED_TAG in child_tags: # Выбран полностью или частично
                any_active_child_checked_or_partially = True
            
            if CHECKED_TAG not in child_tags: # Если хотя бы один активный не выбран ПОЛНОСТЬЮ
                all_active_children_fully_checked = False

    parent_tags_set.discard(CHECKED_TAG)
    parent_tags_set.discard(PARTIALLY_CHECKED_TAG)

    if has_active_children:
        if all_active_children_fully_checked:
            parent_tags_set.add(CHECKED_TAG)
        elif any_active_child_checked_or_partially:
            parent_tags_set.add(PARTIALLY_CHECKED_TAG)
    # Если нет активных детей или ни один активный ребенок не выбран, оба тега остаются снятыми.
    
    tree.item(parent_id, tags=tuple(parent_tags_set))
    _update_parent_check_state_recursive(tree, parent_id, selected_tokens_label_ref)


def set_all_tree_check_state(tree, state, selected_tokens_label_ref):
    try:
        root_children = tree.get_children("")
        for item_id in root_children:
            set_check_state_recursive(tree, item_id, state, selected_tokens_label_ref)
        # После изменения всех состояний, обновляем родителей для всех корневых элементов
        # _update_parent_check_state_recursive вызывается рекурсивно вверх от каждого измененного элемента,
        # поэтому дополнительный проход здесь не нужен, если set_check_state_recursive вызывает _update_parent_check_state_recursive.
        # Однако, set_check_state_recursive не вызывает его, поэтому нужен.
        # Лучше, чтобы set_check_state_recursive вызывал _update_parent_check_state_recursive для item_id.
        # Но для Select All/Deselect All, проще пройтись по корневым и обновить их родителей.
        for item_id in root_children:
             if tree.exists(item_id):
                 _update_parent_check_state_recursive(tree, item_id, selected_tokens_label_ref)


    except tk.TclError: pass 
    
    if selected_tokens_label_ref:
        update_selected_tokens_display(tree, selected_tokens_label_ref)
