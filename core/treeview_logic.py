# treeview_logic.py
import os
import tkinter as tk
from tkinter import ttk
from pathlib import Path
import threading
import queue
import sys
import time
import traceback

from core.fs_scanner_utils import (
    should_exclude_item, get_item_status_info,
    BINARY_STATUS_TAG, LARGE_FILE_STATUS_TAG, ERROR_STATUS_TAG,
    EXCLUDED_BY_DEFAULT_STATUS_TAG, TOO_MANY_TOKENS_STATUS_TAG
)

try:
    # ИЗМЕНЕНО: Импортируем Matcher для ручного создания
    from gitignore_parser import parse_gitignore, Matcher
except ImportError:
    parse_gitignore = None
    Matcher = None # Добавляем Matcher сюда

CHECKED_TAG = "checked"
FOLDER_TAG = "folder_tv" 
FILE_TAG = "file_tv"     
PARTIALLY_CHECKED_TAG = "partially_checked"

BINARY_TAG_UI = BINARY_STATUS_TAG 
LARGE_FILE_TAG_UI = LARGE_FILE_STATUS_TAG
ERROR_TAG_UI = ERROR_STATUS_TAG
EXCLUDED_BY_DEFAULT_TAG_UI = EXCLUDED_BY_DEFAULT_STATUS_TAG
TOO_MANY_TOKENS_TAG_UI = TOO_MANY_TOKENS_STATUS_TAG

DISABLED_LOOK_TAGS_UI = {
    BINARY_TAG_UI, LARGE_FILE_TAG_UI, ERROR_TAG_UI,
    EXCLUDED_BY_DEFAULT_TAG_UI, TOO_MANY_TOKENS_TAG_UI
}

# Символы для отрисовки дерева (те же, что и в project_structure_utils)
LINE_VERTICAL_TV = "│   "
LINE_INTERSECTION_TV = "├── "
LINE_CORNER_TV = "└── "
LINE_EMPTY_TV = "    "


tree_item_paths = {} 
tree_item_data = {}  
gitignore_matcher = None 
populate_thread = None
update_queue = queue.Queue()
gui_queue_processor_running = False
last_processed_dir_path_str = None 
gui_update_call_counter = 0


def _map_status_tags_to_ui_tags(fs_status_tags_list):
    ui_tags_set = set()
    if BINARY_STATUS_TAG in fs_status_tags_list:
        ui_tags_set.add(BINARY_TAG_UI)
    if LARGE_FILE_STATUS_TAG in fs_status_tags_list:
        ui_tags_set.add(LARGE_FILE_TAG_UI)
    if ERROR_STATUS_TAG in fs_status_tags_list:
        ui_tags_set.add(ERROR_TAG_UI)
    if EXCLUDED_BY_DEFAULT_STATUS_TAG in fs_status_tags_list:
        ui_tags_set.add(EXCLUDED_BY_DEFAULT_TAG_UI)
    if TOO_MANY_TOKENS_STATUS_TAG in fs_status_tags_list:
        ui_tags_set.add(TOO_MANY_TOKENS_TAG_UI)
    return ui_tags_set


def _get_node_info_for_treeview(item_path_obj: Path, item_name: str, is_dir: bool, log_widget_ref):
    fs_status_tags, fs_status_msg, fs_token_count = get_item_status_info(
        item_path_obj, item_name, is_dir, log_widget_ref
    )
    current_ui_tags = _map_status_tags_to_ui_tags(fs_status_tags)
    if is_dir:
        current_ui_tags.add(FOLDER_TAG)
    else:
        current_ui_tags.add(FILE_TAG)
    return current_ui_tags, fs_status_msg, fs_token_count


def _update_tree_item_display(tree, item_id, item_name, token_count, status_msg, is_dir):
    display_name = item_name
    current_item_ui_tags = set()
    if tree.exists(item_id):
        try:
            current_item_ui_tags = set(tree.item(item_id, 'tags'))
        except tk.TclError: 
            return

    if is_dir and token_count is not None and token_count > 0:
        formatted_tokens = f"{token_count:,}".replace(",", " ")
        display_name += f" ({formatted_tokens} токенов)"
    elif not is_dir and token_count is not None and token_count > 0:
        if TOO_MANY_TOKENS_TAG_UI not in current_item_ui_tags: 
            formatted_tokens = f"{token_count:,}".replace(",", " ")
            display_name += f" ({formatted_tokens} токенов)"
    
    if status_msg:
        show_status_in_brackets = True
        if token_count is not None and token_count > 0 and "токенов" in status_msg.lower():
            if is_dir: 
                show_status_in_brackets = False
            elif not is_dir and TOO_MANY_TOKENS_TAG_UI not in current_item_ui_tags: 
                show_status_in_brackets = False
        
        if show_status_in_brackets:
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
    all_ids_in_tree = [] 
    
    def _collect_all_ids_recursive_safe(parent_id=""):
        children_ids = []
        if tree.exists(parent_id) or parent_id == "": 
            try:
                children_ids = tree.get_children(parent_id)
            except tk.TclError: 
                return 
        
        for child_id in children_ids:
            if tree.exists(child_id): 
                all_ids_in_tree.append(child_id)
                _collect_all_ids_recursive_safe(child_id)
    
    try:
        _collect_all_ids_recursive_safe()
    except tk.TclError: 
        return

    for item_id in all_ids_in_tree:
        if not tree.exists(item_id) or item_id not in tree_item_data:
            continue
            
        try:
            item_tags_set = set(tree.item(item_id, 'tags'))
            current_item_data = tree_item_data[item_id] 
            
            if (CHECKED_TAG in item_tags_set or PARTIALLY_CHECKED_TAG in item_tags_set) and \
               not DISABLED_LOOK_TAGS_UI.intersection(item_tags_set) and \
               current_item_data.get('is_file', False): 
                
                tokens_for_item = current_item_data.get('tokens') 
                if isinstance(tokens_for_item, (int, float)) and tokens_for_item > 0:
                    total_selected_tokens += tokens_for_item
        except tk.TclError: 
            continue 
            
    try:
        formatted_tokens_str = f"{int(total_selected_tokens):,}".replace(",", " ")
        label_widget.config(text=f"Выделено токенов: {formatted_tokens_str}")
    except tk.TclError: pass 
    except Exception: pass 


def _process_tree_updates(tree, progress_bar, progress_label, log_widget_ref):
    global populate_thread, update_queue, gui_queue_processor_running, gui_update_call_counter
    if not gui_queue_processor_running: 
        return

    gui_update_call_counter += 1
    items_processed_this_cycle = 0
    max_items_per_cycle = 150 
    log_prefix_gui = "LOG_GUI: " 

    try:
        while items_processed_this_cycle < max_items_per_cycle:
            try:
                action, data = update_queue.get_nowait()
                items_processed_this_cycle += 1
                item_name_for_log_debug = "N/A" 
                if action == "add_node" and isinstance(data, tuple) and len(data) > 2:
                    item_name_for_log_debug = data[2] 
                elif action == "update_node_text" and isinstance(data, tuple) and len(data) > 1:
                    item_name_for_log_debug = data[1] 
                
                try:
                    if action == "clear_tree":
                        if log_widget_ref: log_widget_ref.insert(tk.END, log_prefix_gui + "Очистка дерева...\n", ('info',))
                        for item in tree.get_children(""): tree.delete(item)
                        tree_item_paths.clear(); tree_item_data.clear()
                    elif action == "progress_start":
                        if log_widget_ref: log_widget_ref.insert(tk.END, f"{log_prefix_gui}Начало прогресса сканирования...\n", ('info',))
                        progress_bar.config(mode='indeterminate', maximum=100); progress_bar.start(10) 
                        progress_label.config(text="Сканирование директории...")
                        if not progress_bar.winfo_ismapped(): progress_bar.grid()
                        if not progress_label.winfo_ismapped(): progress_label.grid()
                    elif action == "progress_step":
                        label_text_from_data = data if isinstance(data, str) else '???'
                        progress_label.config(text=f"Обработка: {label_text_from_data[:45]}...")
                    elif action == "add_node":
                        parent_id_str, item_id_str, text_for_node, ui_tags_tuple, abs_path_str, node_data_dict = data
                        if (parent_id_str == "" or tree.exists(parent_id_str)) and not tree.exists(item_id_str):
                             tree.insert(parent_id_str, tk.END, iid=item_id_str, text=text_for_node, open=False, tags=ui_tags_tuple)
                             tree_item_paths[item_id_str] = abs_path_str; tree_item_data[item_id_str] = node_data_dict
                             _update_tree_item_display(tree, item_id_str, node_data_dict['name_only'],
                                                       node_data_dict.get('tokens'),  node_data_dict.get('status_msg',''),
                                                       node_data_dict.get('is_dir', False))
                    elif action == "update_node_text": 
                         item_id_to_update, name_for_display, tokens_val, status_str, is_dir_flag = data
                         if tree.exists(item_id_to_update):
                             _update_tree_item_display(tree, item_id_to_update, name_for_display, tokens_val, status_str, is_dir_flag)
                             if item_id_to_update in tree_item_data: 
                                 tree_item_data[item_id_to_update]['tokens'] = tokens_val
                                 tree_item_data[item_id_to_update]['status_msg'] = status_str
                    elif action == "finished":
                        if log_widget_ref: log_widget_ref.insert(tk.END, f"{log_prefix_gui}Сигнал 'finished' (завершение сканирования) получен.\n", ('info',))
                        progress_bar.stop(); progress_bar.config(mode='determinate', value=0) 
                        if progress_bar.winfo_ismapped(): progress_bar.grid_remove()
                        if progress_label.winfo_ismapped(): progress_label.grid_remove()
                        selected_tokens_label_widget = getattr(tree, 'selected_tokens_label_ref', None)
                        root_item_ids = tree.get_children("")
                        if root_item_ids: set_all_tree_check_state(tree, True, selected_tokens_label_widget) 
                        elif selected_tokens_label_widget: update_selected_tokens_display(tree, selected_tokens_label_widget)
                        if log_widget_ref:
                            log_widget_ref.insert(tk.END, f"{log_prefix_gui}Заполнение дерева завершено.\n", ('info',))
                            log_widget_ref.see(tk.END) 
                        gui_queue_processor_running = False; return 
                    elif action == "log_message": 
                         if log_widget_ref:
                            log_message_content, log_tags_tuple = ("", ())
                            if isinstance(data, tuple) and len(data) == 2: log_message_content, log_tags_tuple = data
                            elif isinstance(data, str): log_message_content = data
                            else: log_message_content = str(data) 
                            try: log_widget_ref.insert(tk.END, f"{log_message_content}\n", log_tags_tuple)
                            except tk.TclError: pass 
                except tk.TclError as e: 
                    if log_widget_ref: log_widget_ref.insert(tk.END, f"{log_prefix_gui}TclError (действие: {action}, элемент: '{item_name_for_log_debug}'): {e}\n", ('error',))
                except Exception as e: 
                    if log_widget_ref: log_widget_ref.insert(tk.END, f"{log_prefix_gui}Ошибка обработки действия '{action}' (элемент: '{item_name_for_log_debug}'): {e}\n{traceback.format_exc()}\n", ('error',))
            except queue.Empty: break 
    finally:
        if gui_queue_processor_running: 
            tree.after(50, lambda: _process_tree_updates(tree, progress_bar, progress_label, log_widget_ref))
        else: 
            if progress_bar.winfo_ismapped(): progress_bar.stop(); progress_bar.grid_remove()
            if progress_label.winfo_ismapped(): progress_label.grid_remove()
            final_selected_tokens_label = getattr(tree, 'selected_tokens_label_ref', None)
            if final_selected_tokens_label: update_selected_tokens_display(tree, final_selected_tokens_label)

def populate_file_tree_threaded( dir_path_str_or_none, tree, log_widget_ref,  p_bar_ref, p_label_ref, force_rescan=False ):
    global populate_thread, tree_item_paths, tree_item_data, gitignore_matcher, gui_queue_processor_running, last_processed_dir_path_str, gui_update_call_counter
    log_prefix_main_populate = "LOG_POPULATE: "
    if populate_thread and populate_thread.is_alive():
        if log_widget_ref: log_widget_ref.insert(tk.END, f"{log_prefix_main_populate}Процесс заполнения уже запущен...\n", ('info',)); return
    valid_path, norm_path_str = False, ""
    if dir_path_str_or_none:
        try:
            res_path = Path(dir_path_str_or_none).resolve()
            if res_path.is_dir(): norm_path_str, valid_path = str(res_path), True
        except Exception: pass
    if not force_rescan and valid_path and norm_path_str == last_processed_dir_path_str and tree.get_children(""):
        if log_widget_ref: log_widget_ref.insert(tk.END, f"{log_prefix_main_populate}Директория '{Path(dir_path_str_or_none).name}' уже отображена.\n", ('info',)); return
    while not update_queue.empty():
        try: update_queue.get_nowait()
        except queue.Empty: break
    tree_item_paths.clear(); tree_item_data.clear(); gitignore_matcher = None
    last_processed_dir_path_str = norm_path_str if valid_path else None 
    gui_update_call_counter = 0; update_queue.put(("clear_tree", None)) 
    if not valid_path:
        msg = f"Ошибка: '{dir_path_str_or_none}' не директория." if dir_path_str_or_none else "Выберите директорию."
        update_queue.put(("add_node", ("", "msg_node_invalid", msg, ('message',), "", {'name_only': msg, 'is_dir': False, 'is_file': False, 'status_msg': '', 'tokens': 0, 'ui_tags': {'message'}})))
        update_queue.put(("finished", None)) 
    if not gui_queue_processor_running:
        gui_queue_processor_running = True
        tree.after_idle(lambda: _process_tree_updates(tree, p_bar_ref, p_label_ref, log_widget_ref))
    if valid_path: 
        populate_thread = threading.Thread(target=_populate_file_tree_actual_scan, args=(norm_path_str, log_widget_ref), daemon=True, name=f"PopulateTree-{time.strftime('%H%M%S')}")
        try: populate_thread.start()
        except RuntimeError as e:
             if log_widget_ref: log_widget_ref.insert(tk.END, f"{log_prefix_main_populate}КРИТ.ОШИБКА: Не удалось запустить поток: {e}\n", ('error',))
             update_queue.put(("log_message", (f"{log_prefix_main_populate}КРИТ.ОШИБКА: Запуск потока не удался: {e}", ('error',)))); update_queue.put(("finished", None)) 

def _populate_file_tree_actual_scan(abs_dir_path_str, log_widget_ref):
    global gitignore_matcher, update_queue 
    thread_name, log_prefix = threading.current_thread().name, f"LOG_THREAD ({threading.current_thread().name}): " 
    try:
        root_dir_obj = Path(abs_dir_path_str); gitignore_matcher = None
        # ИЗМЕНЕНО: Используем Matcher и читаем файл в UTF-8, чтобы избежать ошибок кодировки
        if Matcher: # Проверяем, что Matcher был успешно импортирован
            gi_file = root_dir_obj / ".gitignore"
            if gi_file.is_file():
                try:
                    # Читаем файл с явным указанием кодировки UTF-8
                    with gi_file.open(mode='r', encoding='utf-8') as f:
                        lines = f.readlines()
                    # Создаем Matcher вручную
                    gitignore_matcher = Matcher(lines, str(root_dir_obj))
                    update_queue.put(("log_message", (f"{log_prefix}Используется .gitignore: {gi_file}", ('info',))))
                except Exception as e:
                    update_queue.put(("log_message", (f"{log_prefix}ПРЕДУПРЕЖДЕНИЕ: Ошибка разбора .gitignore ({gi_file}): {e}", ('warning',))))

        update_queue.put(("progress_start", None)); root_name, root_id = root_dir_obj.name, str(root_dir_obj)
        root_ui_tags, root_status, _ = _get_node_info_for_treeview(root_dir_obj, root_name, True, log_widget_ref)
        root_ui_tags.add(CHECKED_TAG) 
        root_data = {'name_only': root_name, 'is_dir': True, 'is_file': False, 'rel_path': "", 'tokens': 0, 'status_msg': root_status, 'ui_tags': root_ui_tags }
        update_queue.put(("add_node", ("", root_id, root_name, tuple(root_ui_tags), str(root_dir_obj), root_data)))
        tokens_root = _populate_recursive_scan_and_count_tokens(root_dir_obj, root_id, root_dir_obj, log_widget_ref)
        if isinstance(tokens_root, (int, float)) and tokens_root >= 0: update_queue.put(("update_node_text", (root_id, root_name, tokens_root, root_status, True))) 
        else: update_queue.put(("log_message", (f"{log_prefix}ПРЕДУПРЕЖДЕНИЕ: Для '{root_name}' токены не были числом: {tokens_root}.", ('warning',)))); update_queue.put(("update_node_text", (root_id, root_name, 0, "ошибка подсчета токенов", True)))
    except Exception as e:
        err_msg = f"Крит. ошибка в потоке {thread_name} при обработке '{abs_dir_path_str}': {e}"
        update_queue.put(("log_message", (f"LOG_THREAD_ERROR: {err_msg}\n{traceback.format_exc()}", ('error',))))
        try: update_queue.put(("add_node", ("", f"error_root_{thread_name}", err_msg[:100], ('error',), abs_dir_path_str, {'name_only': "Ошибка потока", 'is_dir': False, 'is_file': False, 'status_msg':err_msg[:100], 'tokens':0, 'ui_tags': {'error'}})))
        except: pass
    finally: update_queue.put(("finished", None)) 

def _populate_recursive_scan_and_count_tokens( cur_dir_obj, parent_id_str, root_dir_obj, log_widget_ref ):
    total_tokens_folder, cur_dir_name_log = 0, cur_dir_obj.name
    update_queue.put(("progress_step", cur_dir_obj.name)) 
    if not (os.access(str(cur_dir_obj), os.R_OK) and os.access(str(cur_dir_obj), os.X_OK)):
        perm_err_msg = f"Отказ в доступе к '{cur_dir_name_log}'"
        update_queue.put(("log_message", (f"LOG_REC_SCAN: ПРЕДУПРЕЖДЕНИЕ: {perm_err_msg}", ('warning',))))
        update_queue.put(("update_node_text", (parent_id_str, cur_dir_obj.name, 0, perm_err_msg, True))); return 0 
    try:
        items = []; [items.append(item) for item in cur_dir_obj.iterdir()]; items.sort(key=lambda x: (not x.is_dir(), x.name.lower())) 
        for item_path_obj in items:
            item_name, is_dir, item_id_str = item_path_obj.name, item_path_obj.is_dir(), str(item_path_obj) 
            if should_exclude_item(item_path_obj, item_name, is_dir, gitignore_matcher): continue
            ui_tags, status_msg, file_tokens = _get_node_info_for_treeview(item_path_obj, item_name, is_dir, log_widget_ref)
            if not DISABLED_LOOK_TAGS_UI.intersection(ui_tags): ui_tags.add(CHECKED_TAG)
            rel_path = str(item_path_obj.relative_to(root_dir_obj)) if item_path_obj.is_relative_to(root_dir_obj) else item_name
            data_dict = {'name_only': item_name, 'is_dir': is_dir, 'is_file': not is_dir, 'rel_path': rel_path, 'tokens': file_tokens if not is_dir else 0, 'status_msg': status_msg, 'ui_tags': ui_tags }
            update_queue.put(("add_node", (parent_id_str, item_id_str, item_name, tuple(ui_tags), str(item_path_obj), data_dict)))
            if is_dir:
                tokens_subdir = _populate_recursive_scan_and_count_tokens(item_path_obj, item_id_str, root_dir_obj, log_widget_ref)
                if isinstance(tokens_subdir, (int,float)) and tokens_subdir >= 0: total_tokens_folder += tokens_subdir; update_queue.put(("update_node_text", (item_id_str, item_name, tokens_subdir, data_dict.get('status_msg',''), True)))
            elif file_tokens is not None and isinstance(file_tokens, (int, float)) and file_tokens >=0: total_tokens_folder += file_tokens
    except OSError as e:
        iter_err = f"Ошибка чтения директории '{cur_dir_name_log}': {e.strerror}"
        update_queue.put(("log_message", (f"LOG_REC_SCAN: {iter_err}", ('error',)))); update_queue.put(("update_node_text", (parent_id_str, cur_dir_obj.name, 0, iter_err, True)))
    except Exception as e:
        gen_err = f"Неожиданная ошибка при обработке '{cur_dir_name_log}': {e}"
        update_queue.put(("log_message", (f"LOG_REC_SCAN: {gen_err}\n{traceback.format_exc()}", ('error',)))); update_queue.put(("update_node_text", (parent_id_str, cur_dir_obj.name, 0, str(e)[:50], True)))
    return total_tokens_folder

def generate_project_structure_text(tree, root_dir_path_str, log_widget_ref): 
    if not root_dir_path_str:
        if log_widget_ref: log_widget_ref.insert(tk.END, "Структура (выделенные): Корневая директория не задана.\n", ('warning',))
        return "Структура не сгенерирована: корневая директория не задана."
    resolved_root_dir = Path(root_dir_path_str)
    if not resolved_root_dir.is_dir():
        if log_widget_ref: log_widget_ref.insert(tk.END, f"Структура (выделенные): Директория '{root_dir_path_str}' не найдена.\n", ('warning',))
        return "Структура не сгенерирована: неверная корневая директория."
    resolved_root_dir = resolved_root_dir.resolve()

    structure_lines = [resolved_root_dir.name] # Начинаем с имени корневой директории
    paths_added = {str(resolved_root_dir)} # Добавляем путь к корню, чтобы избежать его повторного вывода

    def _generate_recursive_selected(parent_item_id, current_prefix_str):
        children_of_parent = []
        try:
            # Получаем только тех детей, которые выбраны или частично выбраны
            # и сортируем их для консистентного вывода
            raw_children_ids = tree.get_children(parent_item_id)
            for child_id_str in raw_children_ids:
                if tree.exists(child_id_str) and child_id_str in tree_item_data:
                    child_tags_set = set(tree.item(child_id_str, 'tags'))
                    # Папка отображается, если она CHECKED или PARTIALLY_CHECKED
                    # Файл отображается, только если он CHECKED и не DISABLED
                    is_dir_and_selected = tree_item_data[child_id_str].get('is_dir') and \
                                          bool({CHECKED_TAG, PARTIALLY_CHECKED_TAG}.intersection(child_tags_set))
                    is_file_and_selected = tree_item_data[child_id_str].get('is_file') and \
                                           CHECKED_TAG in child_tags_set and \
                                           not DISABLED_LOOK_TAGS_UI.intersection(child_tags_set)
                    
                    if is_dir_and_selected or is_file_and_selected:
                        children_of_parent.append(child_id_str)
            
            children_of_parent.sort(key=lambda cid_sort: (
                not tree_item_data.get(cid_sort, {}).get('is_dir', False),
                tree_item_data.get(cid_sort, {}).get('name_only', '').lower()
            ))
        except tk.TclError:
            return # Ошибка доступа к детям

        num_children_to_display = len(children_of_parent)
        for i, child_id_to_process in enumerate(children_of_parent):
            child_node_data = tree_item_data[child_id_to_process]
            child_display_name = child_node_data.get('name_only', 'unknown_child')
            is_last_child_in_list = (i == num_children_to_display - 1)
            
            line_connector = LINE_CORNER_TV if is_last_child_in_list else LINE_INTERSECTION_TV
            
            full_display_line = current_prefix_str + line_connector + child_display_name
            
            if child_node_data.get('is_dir'):
                full_display_line += "/"
            
            # Добавляем строку, если путь еще не был добавлен (маловероятно здесь, но для безопасности)
            # Используем rel_path для проверки уникальности в структуре
            child_rel_path = child_node_data.get('rel_path')
            if child_rel_path not in paths_added : # paths_added должен хранить rel_path
                structure_lines.append(full_display_line)
                paths_added.add(child_rel_path)
            
            if child_node_data.get('is_dir'):
                # Новый префикс для детей этой подпапки
                new_child_prefix_for_recursion = current_prefix_str + \
                                                 (LINE_EMPTY_TV if is_last_child_in_list else LINE_VERTICAL_TV)
                _generate_recursive_selected(child_id_to_process, new_child_prefix_for_recursion)

    # Находим ID корневого элемента в TreeView, соответствующего resolved_root_dir
    root_id_in_treeview = None
    for tv_root_id in tree.get_children(""): # Итерируем по элементам верхнего уровня дерева
        if tree.exists(tv_root_id) and tree_item_paths.get(tv_root_id) == str(resolved_root_dir):
            root_id_in_treeview = tv_root_id
            break
            
    if root_id_in_treeview:
        # Проверяем, выбран ли сам корневой элемент
        root_tags = set(tree.item(root_id_in_treeview, 'tags'))
        if bool({CHECKED_TAG, PARTIALLY_CHECKED_TAG}.intersection(root_tags)):
            # Если корень выбран, начинаем рекурсию для его детей
            _generate_recursive_selected(root_id_in_treeview, "") # Начальный префикс пустой
        elif log_widget_ref:
             log_widget_ref.insert(tk.END, "Структура (выделенные): Корневая директория не выбрана.\n", ('info',))
    elif log_widget_ref:
        log_widget_ref.insert(tk.END, "Структура (выделенные): Не найден корневой узел в дереве для указанного пути.\n", ('warning',))


    if len(structure_lines) <= 1 and not (len(structure_lines) == 1 and structure_lines[0].strip()):
        # Если только имя корневой папки и больше ничего (или вообще ничего)
        if log_widget_ref: log_widget_ref.insert(tk.END, "Структура (выделенные): Нет выбранных элементов для отображения.\n", ('warning',))
        return "Структура не сгенерирована: нет выбранных элементов."
        
    return "<file_map>\n" + "\n".join(structure_lines) + "\n</file_map>"

def toggle_check(event, tree, selected_tokens_label_ref): 
    item_id_clicked = tree.identify_row(event.y)
    if not item_id_clicked or not tree.exists(item_id_clicked): return 
    region_clicked, element_identified = tree.identify("region", event.x, event.y), tree.identify_element(event.x, event.y) 
    if region_clicked == "indicator" or (isinstance(element_identified, str) and ".indicator" in element_identified): return 
    current_item_tags_set = set(tree.item(item_id_clicked, 'tags'))
    is_item_disabled_for_selection = bool(DISABLED_LOOK_TAGS_UI.intersection(current_item_tags_set))
    if is_item_disabled_for_selection:
        log_widget_for_toggle = getattr(tree, 'log_widget_ref', None)
        if log_widget_for_toggle:
             try: 
                 item_name_for_log, disabled_tags_str = tree_item_data.get(item_id_clicked,{}).get('name_only',item_id_clicked), ', '.join(DISABLED_LOOK_TAGS_UI.intersection(current_item_tags_set))
                 log_widget_for_toggle.insert(tk.END, f"ИНФО: Элемент '{item_name_for_log}' не выбран (статус: {disabled_tags_str}).\n", ('info',))
             except tk.TclError: pass 
        return 
    new_selection_state_is_checked = CHECKED_TAG not in current_item_tags_set
    set_check_state_recursive(tree, item_id_clicked, new_selection_state_is_checked, selected_tokens_label_ref)
    _update_parent_check_state_recursive(tree, item_id_clicked, selected_tokens_label_ref)
    if selected_tokens_label_ref: update_selected_tokens_display(tree, selected_tokens_label_ref)

def set_check_state_recursive(tree, item_id_to_set, new_state_is_checked, selected_tokens_label_ref):
    if not tree.exists(item_id_to_set): return 
    current_tags_of_item = set(tree.item(item_id_to_set, 'tags'))
    is_item_disabled = bool(DISABLED_LOOK_TAGS_UI.intersection(current_tags_of_item))
    
    current_tags_of_item.discard(CHECKED_TAG)
    current_tags_of_item.discard(PARTIALLY_CHECKED_TAG)

    if not is_item_disabled: 
        if new_state_is_checked: 
            current_tags_of_item.add(CHECKED_TAG)
    
    tree.item(item_id_to_set, tags=tuple(current_tags_of_item))

    try:
        children_of_item = tree.get_children(item_id_to_set)
        for child_id in children_of_item:
            set_check_state_recursive(tree, child_id, new_state_is_checked, selected_tokens_label_ref)
    except tk.TclError: pass 


def _update_parent_check_state_recursive(tree, child_item_id, selected_tokens_label_ref):
    if not child_item_id or not tree.exists(child_item_id): return
    
    parent_id_of_child = tree.parent(child_item_id)
    if not parent_id_of_child or not tree.exists(parent_id_of_child): 
        return

    parent_current_tags_set = set(tree.item(parent_id_of_child, 'tags'))
    if DISABLED_LOOK_TAGS_UI.intersection(parent_current_tags_set):
        _update_parent_check_state_recursive(tree, parent_id_of_child, selected_tokens_label_ref)
        return

    all_children_of_parent = []
    try:
        all_children_of_parent = tree.get_children(parent_id_of_child)
    except tk.TclError: return 

    if not all_children_of_parent: 
        parent_current_tags_set.discard(CHECKED_TAG)
        parent_current_tags_set.discard(PARTIALLY_CHECKED_TAG)
        tree.item(parent_id_of_child, tags=tuple(parent_current_tags_set))
        _update_parent_check_state_recursive(tree, parent_id_of_child, selected_tokens_label_ref) 
        return

    all_active_children_are_fully_checked = True 
    any_active_child_is_selected = False       
    parent_has_any_active_children = False     

    for sibling_id in all_children_of_parent:
        if not tree.exists(sibling_id): continue 
        
        sibling_tags_set = set(tree.item(sibling_id, 'tags'))
        is_sibling_disabled = bool(DISABLED_LOOK_TAGS_UI.intersection(sibling_tags_set))

        if not is_sibling_disabled: 
            parent_has_any_active_children = True
            if CHECKED_TAG in sibling_tags_set or PARTIALLY_CHECKED_TAG in sibling_tags_set:
                any_active_child_is_selected = True
            
            if CHECKED_TAG not in sibling_tags_set: 
                all_active_children_are_fully_checked = False
        else: 
            all_active_children_are_fully_checked = False 
            
    parent_current_tags_set.discard(CHECKED_TAG)
    parent_current_tags_set.discard(PARTIALLY_CHECKED_TAG)

    if parent_has_any_active_children: 
        if all_active_children_are_fully_checked: 
            parent_current_tags_set.add(CHECKED_TAG)
        elif any_active_child_is_selected: 
            parent_current_tags_set.add(PARTIALLY_CHECKED_TAG)
    
    tree.item(parent_id_of_child, tags=tuple(parent_current_tags_set))
    _update_parent_check_state_recursive(tree, parent_id_of_child, selected_tokens_label_ref)


def set_all_tree_check_state(tree, new_state_to_set: bool, selected_tokens_label_ref):
    try:
        root_level_children_ids = tree.get_children("")
        for item_id_root_level in root_level_children_ids:
            set_check_state_recursive(tree, item_id_root_level, new_state_to_set, selected_tokens_label_ref)
        
        for item_id_root_level_for_update in root_level_children_ids:
             if tree.exists(item_id_root_level_for_update): 
                 _update_parent_check_state_recursive(tree, item_id_root_level_for_update, selected_tokens_label_ref)
    except tk.TclError: pass 
    
    if selected_tokens_label_ref:
        update_selected_tokens_display(tree, selected_tokens_label_ref)