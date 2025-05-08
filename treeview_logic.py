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

# --- Импорты и константы (без изменений) ---
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
BINARY_TAG = "binary"; LARGE_FILE_TAG = "large_file"; ERROR_TAG = "error_status"
EXCLUDED_BY_DEFAULT_TAG = "excluded_default"; TOO_MANY_TOKENS_TAG = "too_many_tokens"
DISABLED_LOOK_TAGS = {BINARY_TAG, LARGE_FILE_TAG, ERROR_TAG, EXCLUDED_BY_DEFAULT_TAG, TOO_MANY_TOKENS_TAG} # Используется для управления ВЫБОРОМ и КОПИРОВАНИЕМ СОДЕРЖИМОГО


# --- Глобальные переменные модуля (без изменений) ---
tree_item_paths = {}; tree_item_data = {}; gitignore_matcher = None
populate_thread = None; update_queue = queue.Queue()
gui_queue_processor_running = False; last_processed_dir = None
gui_update_call_counter = 0

# --- Функции фильтрации ---
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
    else:
        try:
            file_size = item_path_obj.stat().st_size
            if file_size > MAX_FILE_SIZE_BYTES:
                tags.append(LARGE_FILE_TAG); status_msg = f"> {MAX_FILE_SIZE_BYTES // (1024*1024)}MB"
            else:
                token_count_val, token_error = count_file_tokens(str(item_path_obj), log_widget_ref)
                if token_error:
                    status_msg = token_error
                    if "бинарный" in token_error: tags.append(BINARY_TAG) # Убедимся, что бинарный тег ставится и здесь
                    elif "tiktoken" in token_error: tags.append(ERROR_TAG)
                elif token_count_val is not None:
                    token_count = token_count_val
                    if token_count > MAX_TOKENS_FOR_DISPLAY:
                        tags.append(TOO_MANY_TOKENS_TAG)
                        status_msg = f"токенов > {MAX_TOKENS_FOR_DISPLAY}"
        except OSError as e:
            tags.append(ERROR_TAG); status_msg = f"ошибка доступа: {e.strerror}"
        except Exception as e:
            tags.append(ERROR_TAG); status_msg = f"ошибка: {str(e)[:30]}"

    # Тег EXCLUDED_BY_DEFAULT_TAG добавляется к файлам, которые обычно не нужны,
    # но он не должен мешать их отображению в структуре, если они не отфильтрованы ранее.
    # Он влияет на то, будет ли файл выбран по умолчанию.
    # Эта проверка должна идти ПОСЛЕ определения основных тегов (binary, large_file, error),
    # так как эти теги могут быть причиной, по которой файл исключается по умолчанию.
    # Однако, для структуры это не так важно, как для выбора.
    # Мы уже имеем DISABLED_LOOK_TAGS, который включает EXCLUDED_BY_DEFAULT_TAG,
    # и он используется для управления ВЫБОРОМ.
    # Здесь мы просто добавляем тег, если он подходит под паттерн.
    # Проверка `not DISABLED_LOOK_TAGS.intersection(tags)` здесь не нужна,
    # так как мы хотим добавить этот тег независимо от других "отключающих" тегов.
    for pattern in EXCLUDED_BY_DEFAULT_PATTERNS:
        if fnmatch.fnmatch(item_name, pattern):
            if EXCLUDED_BY_DEFAULT_TAG not in tags: # Добавляем, только если его еще нет
                tags.append(EXCLUDED_BY_DEFAULT_TAG)
            if not status_msg: status_msg = "исключен по умолчанию" # Обновляем статус, если он пуст
            break
    return tags, status_msg, token_count


# --- Логика Treeview ---
def _update_tree_item_display(tree, item_id, item_name, token_count, status_msg, is_dir):
    display_name = item_name
    if token_count is not None and token_count > 0:
        display_name += f" ({token_count} токенов)"
    elif not is_dir and status_msg: # Статус показываем для файлов, если нет токенов или статус важен
        display_name += f" [{status_msg}]"
    elif is_dir and status_msg: # Для папок тоже можем показать статус, если он есть (например, ошибка доступа)
        display_name += f" [{status_msg}]"
    tree.item(item_id, text=display_name)


def _process_tree_updates(tree, progress_bar, progress_label, log_widget_ref):
    global populate_thread, update_queue, gui_queue_processor_running, gui_update_call_counter
    if not gui_queue_processor_running:
        return

    gui_update_call_counter += 1
    items_processed_this_cycle = 0; max_items = 100
    log_prefix_gui = "LOG_GUI: "

    try:
        while items_processed_this_cycle < max_items:
            try:
                action, data = update_queue.get_nowait()
                items_processed_this_cycle += 1
                try:
                    if action == "clear_tree":
                        if log_widget_ref: log_widget_ref.insert(tk.END, log_prefix_gui + "Очистка дерева...\n")
                        for item in tree.get_children(""): tree.delete(item)
                        tree_item_paths.clear(); tree_item_data.clear()
                    elif action == "progress_start":
                        if log_widget_ref: log_widget_ref.insert(tk.END, f"{log_prefix_gui}Начало прогресса (макс={data})...\n")
                        progress_bar.config(mode='indeterminate', maximum=100); progress_bar.start(10)
                        progress_label.config(text="Сканирование..."); progress_bar.grid(); progress_label.grid()
                    elif action == "progress_step":
                        label_text = data if isinstance(data, str) else '???'
                        progress_label.config(text=f"{label_text[:45]}...")
                    elif action == "add_node":
                        parent_id, item_id, text_from_thread, tags_tuple, path_str, data_dict = data
                        if (parent_id == "" or tree.exists(parent_id)) and not tree.exists(item_id):
                             # Используем _update_tree_item_display для консистентного форматирования имени
                             # Передаем данные из data_dict, так как text_from_thread может быть просто именем
                             current_display_name = data_dict['name_only']
                             current_tokens = data_dict.get('tokens',0)
                             current_status = data_dict.get('status_msg','')
                             is_item_dir = data_dict.get('is_dir', False)

                             # Формируем имя так, как оно должно быть при первоначальном добавлении
                             if current_tokens > 0:
                                 current_display_name += f" ({current_tokens} токенов)"
                             elif not is_item_dir and current_status:
                                 current_display_name += f" [{current_status}]"
                             elif is_item_dir and current_status: # Статус для папок (например, ошибка доступа)
                                 current_display_name += f" [{current_status}]"

                             tree.insert(parent_id, tk.END, iid=item_id, text=current_display_name, open=False, tags=tags_tuple)
                             tree_item_paths[item_id] = path_str
                             tree_item_data[item_id] = data_dict
                    elif action == "update_node_text":
                         item_id, name, tokens, status, is_dir_flag = data
                         if tree.exists(item_id):
                             _update_tree_item_display(tree, item_id, name, tokens, status, is_dir_flag)
                             if item_id in tree_item_data:
                                 tree_item_data[item_id]['tokens'] = tokens
                                 tree_item_data[item_id]['status_msg'] = status
                    elif action == "finished":
                        if log_widget_ref: log_widget_ref.insert(tk.END, f"{log_prefix_gui}Сигнал 'finished' получен.\n")
                        progress_bar.stop(); progress_bar.config(mode='determinate', value=0)
                        if progress_bar.winfo_ismapped(): progress_bar.grid_remove()
                        if progress_label.winfo_ismapped(): progress_label.grid_remove()
                        root_items = tree.get_children("")
                        if root_items: set_check_state_recursive(tree, root_items[0], True)
                        if log_widget_ref: log_widget_ref.insert(tk.END, f"{log_prefix_gui}Заполнение дерева завершено.\n"); log_widget_ref.see(tk.END)
                        gui_queue_processor_running = False
                        return
                    elif action == "log_message":
                         if log_widget_ref: log_widget_ref.insert(tk.END, f"{data}\n")
                except tk.TclError as e:
                    if log_widget_ref: log_widget_ref.insert(tk.END, f"{log_prefix_gui}TclError ({action}): {e}\n")
                except Exception as e:
                    if log_widget_ref: log_widget_ref.insert(tk.END, f"{log_prefix_gui}Ошибка ({action}): {e}\n{traceback.format_exc()}\n")
            except queue.Empty:
                break
    finally:
        if gui_queue_processor_running:
            tree.after(50, lambda: _process_tree_updates(tree, progress_bar, progress_label, log_widget_ref))
        else:
            if progress_bar.winfo_ismapped():
                 progress_bar.stop()
                 progress_bar.grid_remove()
                 progress_label.grid_remove()
            if log_widget_ref: log_widget_ref.insert(tk.END, f"{log_prefix_gui}Обработчик очереди остановлен (по флагу).\n")


def populate_file_tree_threaded(directory_path_str, tree, log_widget_ref, progress_bar_ref, progress_label_ref):
    global populate_thread, tree_item_paths, tree_item_data, gitignore_matcher, gui_queue_processor_running, last_processed_dir, gui_update_call_counter
    log_prefix_main = "LOG: "

    if populate_thread and populate_thread.is_alive():
        if log_widget_ref: log_widget_ref.insert(tk.END, f"{log_prefix_main}Процесс заполнения уже запущен.\n"); return

    current_normalized_path = ""; valid_path = False
    if directory_path_str and os.path.isdir(directory_path_str):
        try: current_normalized_path = str(Path(directory_path_str).resolve()); valid_path = True
        except: pass

    if valid_path and current_normalized_path == last_processed_dir and tree.get_children(""):
        if log_widget_ref: log_widget_ref.insert(tk.END, f"{log_prefix_main}Эта директория ({Path(directory_path_str).name}) уже отображена.\n"); return

    while not update_queue.empty():
        try: update_queue.get_nowait()
        except queue.Empty: break
    if log_widget_ref: log_widget_ref.insert(tk.END, f"{log_prefix_main}Очередь очищена.\n")

    tree_item_paths.clear(); tree_item_data.clear(); gitignore_matcher = None
    last_processed_dir = current_normalized_path
    gui_update_call_counter = 0
    update_queue.put(("clear_tree", None))

    if not valid_path:
        update_queue.put(("add_node", ("", "msg_node", "Выберите корректную директорию", ('message',), "", {'name_only': "Сообщение", 'is_dir': False, 'is_file': False, 'status_msg': '', 'tokens': 0})))
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
             if log_widget_ref: log_widget_ref.insert(tk.END, f"LOG_ERROR: Не удалось запустить поток: {thread_start_err}\n")
             update_queue.put(("log_message", f"LOG_ERROR: Не удалось запустить поток: {thread_start_err}"))
             update_queue.put(("finished", None))


def _populate_file_tree_actual(directory_path_str, log_widget_ref):
    global gitignore_matcher, update_queue
    thread_name = threading.current_thread().name
    log_prefix = f"LOG_THREAD ({thread_name}): "
    try:
        update_queue.put(("log_message", f"{log_prefix}Поток стартовал для '{directory_path_str}'."))
        root_dir_obj = Path(directory_path_str).resolve()
        update_queue.put(("log_message", f"{log_prefix}Путь разрешен: {root_dir_obj}"))

        update_queue.put(("log_message", f"{log_prefix}Загрузка .gitignore..."))
        gitignore_matcher = None
        if parse_gitignore:
            gitignore_file = root_dir_obj / ".gitignore"
            if gitignore_file.is_file():
                try: 
                    gitignore_matcher = parse_gitignore(gitignore_file)
                    update_queue.put(("log_message", f"{log_prefix}.gitignore загружен."))
                except Exception as e: 
                    update_queue.put(("log_message", f"{log_prefix}WARN: Ошибка .gitignore: {e}"))
        else: 
            update_queue.put(("log_message", f"{log_prefix}WARN: gitignore-parser не найден."))

        update_queue.put(("progress_start", 1))
        dir_name = root_dir_obj.name; root_node_id = str(root_dir_obj)
        root_data = {'tokens': 0, 'status_msg': "", 'is_dir': True, 'is_file': False, 'rel_path': "", 'name_only': dir_name}
        # Передаем dir_name как текст для add_node, форматирование произойдет в GUI потоке
        update_queue.put(("add_node", ("", root_node_id, dir_name, (FOLDER_TAG, CHECKED_TAG), str(root_dir_obj), root_data)))
        
        folder_tokens = _populate_recursive_threaded(root_dir_obj, root_node_id, root_dir_obj, log_widget_ref)

        if not isinstance(folder_tokens, (int, float)): folder_tokens = 0
        if root_node_id in tree_item_data: # Убедимся, что корневой узел еще существует в данных
            tree_item_data[root_node_id]['tokens'] = folder_tokens
        # Обновляем текст корневого узла с посчитанными токенами
        update_queue.put(("update_node_text", (root_node_id, dir_name, folder_tokens, "", True)))

    except Exception as e:
        error_msg = f"Критическая ошибка в потоке {thread_name}: {str(e)}"
        update_queue.put(("log_message", f"LOG_THREAD_ERROR: {error_msg}\n{traceback.format_exc()}"))
        try: update_queue.put(("add_node", ("", f"error_root_{thread_name}", error_msg, ('error',), directory_path_str, {'name_only': "Ошибка потока", 'is_dir': False, 'is_file': False, 'status_msg':'', 'tokens':0})))
        except: pass
    finally:
        update_queue.put(("log_message", f"{log_prefix}Завершение работы потока."))
        update_queue.put(("finished", None))


def _populate_recursive_threaded(current_dir_obj, parent_id_str, root_dir_obj, log_widget_ref):
    total_tokens_in_folder = 0
    log_prefix_thread_rec = f"LOG_THREAD_REC ({threading.current_thread().name}): "
    update_queue.put(("progress_step", current_dir_obj.name))

    try:
        items = []
        # Проверяем, существует ли директория и можем ли мы ее прочитать
        if not current_dir_obj.is_dir():
            status_msg_for_parent = f"Ошибка: {current_dir_obj.name} не директория или нет доступа"
            update_queue.put(("log_message", f"{log_prefix_thread_rec}{status_msg_for_parent}"))
            # Обновим родительский узел, если это возможно, чтобы показать ошибку
            if parent_id_str in tree_item_data and tree_item_data[parent_id_str].get('is_dir'):
                tree_item_data[parent_id_str]['status_msg'] = status_msg_for_parent # Сохраняем статус для родителя
                update_queue.put(("update_node_text", (parent_id_str, tree_item_data[parent_id_str]['name_only'], tree_item_data[parent_id_str]['tokens'], status_msg_for_parent, True)))
            return 0 # Не можем продолжить с этой папкой

        for item in current_dir_obj.iterdir(): items.append(item)
        items.sort(key=lambda x: (not x.is_dir(), x.name.lower()))

        for item_path_obj in items:
            item_name = item_path_obj.name
            is_dir = item_path_obj.is_dir()
            item_id_str = str(item_path_obj.resolve())

            if should_fully_exclude(item_path_obj, root_dir_obj, item_name, is_dir):
                continue

            tags, status_msg, file_tokens_for_item = get_item_status_tags_and_info(item_path_obj, item_name, is_dir, log_widget_ref)
            
            # Тег CHECKED_TAG добавляется, если нет "отключающих" тегов.
            # Это определяет, будет ли элемент ВЫБРАН по умолчанию.
            # На отображение в СТРУКТУРЕ это влиять не должно.
            if not DISABLED_LOOK_TAGS.intersection(tags):
                tags.append(CHECKED_TAG)

            rel_path = str(item_path_obj.relative_to(root_dir_obj))
            data_dict = {
                'tokens': file_tokens_for_item,
                'status_msg': status_msg,
                'is_dir': is_dir,
                'is_file': not is_dir,
                'rel_path': rel_path,
                'name_only': item_name
            }
            
            # Передаем item_name как текст, форматирование (добавление токенов/статуса) будет в GUI потоке
            update_queue.put(("add_node", (parent_id_str, item_id_str, item_name, tuple(tags), str(item_path_obj), data_dict)))

            if is_dir:
                tokens_from_subdir = _populate_recursive_threaded(item_path_obj, item_id_str, root_dir_obj, log_widget_ref)
                if isinstance(tokens_from_subdir, (int,float)):
                    total_tokens_in_folder += tokens_from_subdir
                    # Обновляем данные и текст для папки с учетом накопленных токенов
                    if item_id_str in tree_item_data: # Убедимся, что узел еще существует
                        tree_item_data[item_id_str]['tokens'] = tokens_from_subdir
                        # Статус папки может быть установлен, если была ошибка доступа к ней самой
                        current_folder_status = tree_item_data[item_id_str].get('status_msg', "")
                        update_queue.put(("update_node_text", (item_id_str, item_name, tokens_from_subdir, current_folder_status, True)))
            elif file_tokens_for_item is not None and CHECKED_TAG in tags and not DISABLED_LOOK_TAGS.intersection(tags):
                # Суммируем токены только для ВЫБРАННЫХ и АКТИВНЫХ файлов
                total_tokens_in_folder += file_tokens_for_item
        
    except PermissionError:
        perm_error_msg = f"Отказ в доступе к {current_dir_obj.name}"
        update_queue.put(("log_message", f"{log_prefix_thread_rec}WARN: {perm_error_msg}"))
        # Обновляем родительский узел (саму папку current_dir_obj) с сообщением об ошибке
        # parent_id_str здесь это ID самой папки current_dir_obj, если это не корень
        # Но мы обновляем сам узел current_dir_obj, если он был добавлен
        item_id_of_current_dir = str(current_dir_obj.resolve())
        if item_id_of_current_dir in tree_item_data:
            tree_item_data[item_id_of_current_dir]['status_msg'] = perm_error_msg
            update_queue.put(("update_node_text", (item_id_of_current_dir, current_dir_obj.name, 0, perm_error_msg, True)))
        # Не добавляем фиктивный узел ошибки внутрь, а помечаем саму папку
    except Exception as e:
        gen_error_msg = f"Ошибка при обработке {current_dir_obj.name}: {e}"
        update_queue.put(("log_message", f"{log_prefix_thread_rec}{gen_error_msg}\n{traceback.format_exc()}"))
        item_id_of_current_dir = str(current_dir_obj.resolve())
        if item_id_of_current_dir in tree_item_data:
            tree_item_data[item_id_of_current_dir]['status_msg'] = str(e)[:50] # Краткое сообщение об ошибке
            update_queue.put(("update_node_text", (item_id_of_current_dir, current_dir_obj.name, 0, str(e)[:50], True)))
    
    return total_tokens_in_folder

def generate_project_structure_text(tree, root_dir_path_str, log_widget_ref):
    """Генерирует текстовое представление структуры ВСЕХ файлов и папок,
       которые были добавлены в дерево (т.е. прошли should_fully_exclude)."""
    if not root_dir_path_str or not os.path.isdir(root_dir_path_str):
        if log_widget_ref: log_widget_ref.insert(tk.END, "Структура: Корневая директория не задана.\n")
        return "Структура не сгенерирована: неверная корневая директория."

    structure_lines = ["Структура проекта:"]
    root_path_obj = Path(root_dir_path_str).resolve()
    
    # Используем set для хранения уже добавленных в структуру относительных путей,
    # чтобы избежать дубликатов, если один и тот же элемент как-то попал в дерево дважды (маловероятно, но для надежности).
    paths_in_structure = set()

    def _generate_recursive(item_id, indent_level):
        # Проверяем, существует ли элемент в данных дерева
        if not tree.exists(item_id) or item_id not in tree_item_data or item_id not in tree_item_paths:
            return

        item_info = tree_item_data[item_id]
        item_path_str = tree_item_paths[item_id]
        current_path_obj = Path(item_path_str).resolve()

        try:
            # Относительный путь от родителя корневой директории проекта
            # Это делает корневую папку проекта видимой в структуре как элемент верхнего уровня.
            # Если root_dir_path_str это "D:/project", а current_path_obj "D:/project/src", то rel_path будет "project/src"
            # Если root_dir_path_str это "D:/project", а current_path_obj "D:/project", то rel_path будет "project"
            if current_path_obj == root_path_obj :
                 rel_path = Path(item_info.get('name_only', current_path_obj.name))
            else:
                 rel_path = current_path_obj.relative_to(root_path_obj.parent)

        except ValueError: # Может случиться, если пути на разных дисках или другая проблема с relative_to
            rel_path = Path(item_info.get('name_only', current_path_obj.name)) # Откат к просто имени

        rel_path_posix = rel_path.as_posix()

        # Избегаем дублирования путей в структуре
        if rel_path_posix in paths_in_structure:
            return
        paths_in_structure.add(rel_path_posix)

        prefix = '    ' * indent_level
        entry = f"{prefix}- {rel_path_posix}"

        if item_info.get('is_dir'):
            structure_lines.append(entry + "/")
            children = tree.get_children(item_id)
            children = sorted(children, key=lambda cid: (
                not tree_item_data.get(cid, {}).get('is_dir', False),
                tree_item_data.get(cid, {}).get('name_only', '').lower()
            ))
            for child_id in children:
                _generate_recursive(child_id, indent_level + 1)
        else: # Это файл
            structure_lines.append(entry)

    root_ids = tree.get_children("") # Получаем ID корневых элементов дерева
    if not root_ids:
        if log_widget_ref: log_widget_ref.insert(tk.END, "Структура: Дерево пусто.\n")
        return "Структура не сгенерирована: дерево пусто."

    # Сортируем корневые элементы так же, как и дочерние
    root_ids = sorted(root_ids, key=lambda cid: (
        not tree_item_data.get(cid, {}).get('is_dir', False),
        tree_item_data.get(cid, {}).get('name_only', '').lower()
    ))

    for root_id in root_ids:
        _generate_recursive(root_id, 0)

    if len(structure_lines) <= 1: # Только заголовок "Структура проекта:"
        if log_widget_ref: log_widget_ref.insert(tk.END, "Структура: Нет элементов для отображения в структуре.\n")
        return "Структура не сгенерирована: нет элементов в дереве."
    return "\n".join(structure_lines)


def toggle_check(event, tree):
    item_id = tree.identify_row(event.y)
    if not item_id: return

    current_tags = set(tree.item(item_id, 'tags'))
    is_disabled_by_look = bool(DISABLED_LOOK_TAGS.intersection(current_tags))

    if is_disabled_by_look:
        log_widget_ref = getattr(tree, 'log_widget_ref', None)
        if log_widget_ref:
             try: log_widget_ref.insert(tk.END, f"INFO: Элемент '{tree_item_data.get(item_id,{}).get('name_only',item_id)}' не может быть выбран из-за статуса ({', '.join(DISABLED_LOOK_TAGS.intersection(current_tags))}).\n")
             except: pass
        return

    new_state_is_checked = CHECKED_TAG not in current_tags
    set_check_state_recursive(tree, item_id, new_state_is_checked)
    _update_parent_check_state_recursive(tree, item_id)


def set_check_state_recursive(tree, item_id, state):
    # Проверяем, существует ли элемент, прежде чем работать с ним
    if not tree.exists(item_id):
        return

    current_tags = set(tree.item(item_id, 'tags'))
    is_disabled_by_look = bool(DISABLED_LOOK_TAGS.intersection(current_tags))

    if is_disabled_by_look:
        # Если элемент "отключен по виду", мы не должны его выбирать,
        # но если он был выбран, а состояние меняется на False, его нужно снять.
        if CHECKED_TAG in current_tags and not state : # Если был выбран и теперь снимаем выбор
             tree.item(item_id, tags=tuple(current_tags - {CHECKED_TAG}))
        # Если он отключен и пытаемся выбрать (state=True), ничего не делаем с его CHECKED_TAG
    else: # Элемент не "отключен по виду"
        if state: current_tags.add(CHECKED_TAG)
        else: current_tags.discard(CHECKED_TAG)
        tree.item(item_id, tags=tuple(current_tags))

    # Рекурсивно для дочерних элементов
    for child_id in tree.get_children(item_id):
        set_check_state_recursive(tree, child_id, state)


def _update_parent_check_state_recursive(tree, item_id):
    parent_id = tree.parent(item_id)
    if not parent_id: return

    # Убедимся, что родитель существует
    if not tree.exists(parent_id):
        return

    parent_tags = set(tree.item(parent_id, 'tags'))
    if DISABLED_LOOK_TAGS.intersection(parent_tags): # Если родитель "отключен по виду", его состояние выбора не меняем
        _update_parent_check_state_recursive(tree, parent_id) # Но проверяем его родителя
        return

    all_children = tree.get_children(parent_id)
    if not all_children: # Если у родителя нет детей
        parent_tags.discard(CHECKED_TAG) # Считаем его невыбранным
        tree.item(parent_id, tags=tuple(parent_tags))
        _update_parent_check_state_recursive(tree, parent_id)
        return

    all_active_children_checked = True
    any_active_child_checked = False
    has_active_children = False

    for child_id in all_children:
        if not tree.exists(child_id): continue # Пропускаем, если дочерний элемент был удален
        child_tags = set(tree.item(child_id, 'tags'))
        if not DISABLED_LOOK_TAGS.intersection(child_tags): # Это активный ребенок
            has_active_children = True
            if CHECKED_TAG not in child_tags:
                all_active_children_checked = False
            else:
                any_active_child_checked = True
            # Оптимизация: если уже ясно, что не все выбраны, но хотя бы один выбран,
            # дальнейшая проверка не изменит результат для родителя (он будет "частично" выбран).
            if not all_active_children_checked and any_active_child_checked:
                break
    
    if has_active_children:
        if all_active_children_checked or any_active_child_checked:
            parent_tags.add(CHECKED_TAG)
        else: # Ни один активный ребенок не выбран
            parent_tags.discard(CHECKED_TAG)
    else: # Нет активных детей (все дети DISABLED_LOOK или их нет)
        parent_tags.discard(CHECKED_TAG) # Родитель не может быть выбран, если нечего выбирать из активного

    tree.item(parent_id, tags=tuple(parent_tags))
    _update_parent_check_state_recursive(tree, parent_id)

def set_all_tree_check_state(tree, state):
    for item_id in tree.get_children(""):
        set_check_state_recursive(tree, item_id, state)
