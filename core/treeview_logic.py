# treeview_logic.py
import os
import tkinter as tk
from tkinter import ttk
from pathlib import Path
import threading
import queue

from core.fs_scanner_utils import DISABLED_LOOK_TAGS_UI
from core.treeview_scanner import scan_directory_and_populate_queue
from core.treeview_constants import (
    CHECKED_TAG, UNCHECKED_TAG, TRISTATE_TAG,
    CHECK_CHAR, UNCHECK_CHAR, TRISTATE_CHAR,
    TOO_MANY_TOKENS_TAG_UI
)

tree_item_paths = {}
tree_item_data = {}
populate_thread = None
update_queue = queue.Queue()
gui_queue_processor_running = False
last_processed_dir_path_str = None

def _update_item_display(tree, item_id):
    """Обновляет отображение элемента: текст в основной колонке и чекбокс во второй."""
    if not tree.exists(item_id) or item_id not in tree_item_data:
        return

    data = tree_item_data[item_id]
    tags = set(tree.item(item_id, 'tags'))
    
    if CHECKED_TAG in tags:
        check_char = CHECK_CHAR
    elif TRISTATE_TAG in tags:
        check_char = TRISTATE_CHAR
    else:
        check_char = UNCHECK_CHAR

    base_name = data.get('name_only', '')
    token_count = data.get('tokens')
    status_msg = data.get('status_msg', '')
    is_dir = data.get('is_dir', False)

    token_str = ""
    if token_count is not None and token_count > 0:
        if is_dir or (not is_dir and TOO_MANY_TOKENS_TAG_UI not in tags):
             token_str = f" ({token_count:,} токенов)".replace(",", " ")

    status_str = f" [{status_msg}]" if status_msg else ""
    display_text = f"{base_name}{token_str}{status_str}"

    tree.item(item_id, text=display_text)
    tree.set(item_id, 'checkbox', check_char)


def _process_tree_updates(tree, progress_bar, progress_label, log_widget_ref):
    global gui_queue_processor_running
    if not gui_queue_processor_running or not tree.winfo_exists():
        gui_queue_processor_running = False
        return

    items_processed = 0
    while items_processed < 200:
        if update_queue.empty():
            break
        
        action, data = update_queue.get()
        items_processed += 1
        
        if not tree.winfo_exists():
            gui_queue_processor_running = False
            return

        if action == "clear_tree":
            for item in tree.get_children(""): tree.delete(item)
            tree_item_paths.clear(); tree_item_data.clear()
        elif action == "progress_start":
            if progress_bar.winfo_exists(): progress_bar.grid(); progress_bar.start(10)
            if progress_label.winfo_exists(): progress_label.grid(); progress_label.config(text="Сканирование...")
        elif action == "progress_step":
            if progress_label.winfo_exists(): progress_label.config(text=f"Обработка: {data[:45]}...")
        elif action == "add_node":
            parent_id, item_id, tags, abs_path, node_data = data
            if (parent_id == "" or tree.exists(parent_id)) and not tree.exists(item_id):
                tree_item_paths[item_id] = abs_path
                tree_item_data[item_id] = node_data
                tree.insert(parent_id, tk.END, iid=item_id, open=False, tags=tags)
                _update_item_display(tree, item_id)
        elif action == "update_node_data":
            item_id, tokens, status = data
            if tree.exists(item_id) and item_id in tree_item_data:
                tree_item_data[item_id]['tokens'] = tokens
                tree_item_data[item_id]['status_msg'] = status
                _update_item_display(tree, item_id)
        elif action == "log_message":
            if log_widget_ref and log_widget_ref.winfo_exists():
                msg, tags = (data, ()) if isinstance(data, str) else data
                log_widget_ref.insert(tk.END, f"{msg}\n", tags)
        elif action == "finished":
            if progress_bar.winfo_exists(): progress_bar.stop(); progress_bar.grid_remove()
            if progress_label.winfo_exists(): progress_label.grid_remove()
            
            tokens_label = getattr(tree, 'selected_tokens_label_ref', None)
            if tree.get_children(""): set_all_tree_check_state(tree, True, tokens_label)
            
            if log_widget_ref and log_widget_ref.winfo_exists():
                log_widget_ref.insert(tk.END, "Заполнение дерева завершено.\n", ('info',)); log_widget_ref.see(tk.END)
            
            gui_queue_processor_running = False
            return

    if gui_queue_processor_running:
        tree.after(30, lambda: _process_tree_updates(tree, progress_bar, progress_label, log_widget_ref))

def populate_file_tree_threaded(dir_path, tree, log_widget, p_bar, p_label, force_rescan=False):
    global populate_thread, gui_queue_processor_running, last_processed_dir_path_str
    
    if populate_thread and populate_thread.is_alive():
        if log_widget.winfo_exists(): log_widget.insert(tk.END, "Процесс заполнения уже запущен...\n", ('info',))
        return

    norm_path = str(Path(dir_path).resolve()) if dir_path and Path(dir_path).is_dir() else None

    if not force_rescan and norm_path and norm_path == last_processed_dir_path_str and tree.get_children(""):
        if log_widget.winfo_exists(): log_widget.insert(tk.END, f"Директория '{Path(dir_path).name}' уже отображена.\n", ('info',))
        return

    while not update_queue.empty(): update_queue.get()

    update_queue.put(("clear_tree", None))
    last_processed_dir_path_str = norm_path

    if not norm_path:
        msg = f"Ошибка: '{dir_path}' не директория." if dir_path else "Выберите директорию."
        msg_data = {'name_only': msg, 'is_dir': False, 'is_file': False, 'status_msg': '', 'tokens': 0}
        update_queue.put(("add_node", ("", "msg_node_invalid", ('message', UNCHECKED_TAG), "", msg_data)))
        update_queue.put(("finished", None))

    if not gui_queue_processor_running:
        gui_queue_processor_running = True
        tree.after_idle(lambda: _process_tree_updates(tree, p_bar, p_label, log_widget))

    if norm_path:
        populate_thread = threading.Thread(target=scan_directory_and_populate_queue, args=(norm_path, update_queue, log_widget), daemon=True)
        populate_thread.start()

def update_selected_tokens_display(tree, label_widget):
    if not label_widget or not label_widget.winfo_exists(): return
    total_tokens = 0
    
    items_to_check = list(tree.get_children(""))
    while items_to_check:
        item_id = items_to_check.pop(0)
        if not tree.exists(item_id) or item_id not in tree_item_data: continue
        
        tags = set(tree.item(item_id, 'tags'))
        data = tree_item_data[item_id]
        
        if data.get('is_file') and CHECKED_TAG in tags and not DISABLED_LOOK_TAGS_UI.intersection(tags):
            tokens = data.get('tokens')
            if isinstance(tokens, (int, float)) and tokens > 0:
                total_tokens += tokens
        
        items_to_check.extend(tree.get_children(item_id))
            
    if label_widget.winfo_exists():
        label_widget.config(text=f"Выделено токенов: {int(total_tokens):,}".replace(",", " "))

def on_tree_click(event, tree, tokens_label):
    # --- ФИНАЛЬНОЕ ИСПРАВЛЕНИЕ: Самый простой и надежный метод ---
    # 1. Определяем, по какой колонке был клик.
    column_id = tree.identify_column(event.x)

    # 2. Если клик был НЕ по нашей колонке с чекбоксами ('#1'), то ничего не делаем.
    #    Это автоматически игнорирует клики по основной колонке ('#0') с именами и треугольниками.
    if column_id != '#1':
        return

    # 3. Если мы здесь, значит, клик был точно в колонке чекбоксов. Получаем ID строки.
    row_id = tree.identify_row(event.y)
    if not row_id:
        return # Клик был в пустом месте колонки

    # 4. Запускаем стандартную логику выделения.
    tags = set(tree.item(row_id, 'tags'))
    if DISABLED_LOOK_TAGS_UI.intersection(tags):
        return
        
    is_checked = UNCHECKED_TAG in tags
    
    set_check_state_recursive(tree, row_id, is_checked)
    _update_parent_check_state_recursive(tree, row_id)
    update_selected_tokens_display(tree, tokens_label)
    # --- КОНЕЦ ИСПРАВЛЕНИЯ ---

def set_check_state_recursive(tree, item_id, is_checked):
    if not tree.exists(item_id): return
    
    tags = set(tree.item(item_id, 'tags'))
    tags.discard(CHECKED_TAG); tags.discard(UNCHECKED_TAG); tags.discard(TRISTATE_TAG)
    
    if not DISABLED_LOOK_TAGS_UI.intersection(tags):
        tags.add(CHECKED_TAG if is_checked else UNCHECKED_TAG)
    
    tree.item(item_id, tags=tuple(tags))
    _update_item_display(tree, item_id)
    
    for child_id in tree.get_children(item_id):
        set_check_state_recursive(tree, child_id, is_checked)

def _update_parent_check_state_recursive(tree, item_id):
    parent_id = tree.parent(item_id)
    if not parent_id or not tree.exists(parent_id): return

    children = tree.get_children(parent_id)
    if not children: return

    checked_count, unchecked_count, active_count = 0, 0, 0
    for child_id in children:
        if not tree.exists(child_id): continue
        tags = set(tree.item(child_id, 'tags'))
        if not DISABLED_LOOK_TAGS_UI.intersection(tags):
            active_count += 1
            if CHECKED_TAG in tags: checked_count += 1
            elif UNCHECKED_TAG in tags: unchecked_count += 1

    parent_tags = set(tree.item(parent_id, 'tags'))
    parent_tags.discard(CHECKED_TAG); parent_tags.discard(UNCHECKED_TAG); parent_tags.discard(TRISTATE_TAG)

    if active_count > 0:
        if checked_count == active_count:
            parent_tags.add(CHECKED_TAG)
        elif unchecked_count == active_count:
            parent_tags.add(UNCHECKED_TAG)
        else:
            parent_tags.add(TRISTATE_TAG)
    
    tree.item(parent_id, tags=tuple(parent_tags))
    _update_item_display(tree, parent_id)
    _update_parent_check_state_recursive(tree, parent_id)

def set_all_tree_check_state(tree, is_checked, tokens_label):
    for item_id in tree.get_children(""):
        set_check_state_recursive(tree, item_id, is_checked)
    update_selected_tokens_display(tree, tokens_label)

def generate_project_structure_text(tree, root_dir_path, log_widget):
    if not root_dir_path or not Path(root_dir_path).is_dir():
        return "Структура не сгенерирована: неверная корневая директория."

    root_path = Path(root_dir_path).resolve()
    structure_lines = [root_path.name]
    
    root_id = None
    for item_id in tree.get_children(""):
        if tree_item_paths.get(item_id) == str(root_path):
            root_id = item_id; break
    
    if not root_id: return "Структура не сгенерирована: не найден корень."

    def _generate_recursive(parent_id, prefix):
        children_ids = []
        if tree.exists(parent_id):
            for child_id in tree.get_children(parent_id):
                if tree.exists(child_id) and child_id in tree_item_data:
                    tags = set(tree.item(child_id, 'tags'))
                    data = tree_item_data[child_id]
                    is_dir_sel = data.get('is_dir') and (CHECKED_TAG in tags or TRISTATE_TAG in tags)
                    is_file_sel = data.get('is_file') and CHECKED_TAG in tags and not DISABLED_LOOK_TAGS_UI.intersection(tags)
                    if is_dir_sel or is_file_sel: children_ids.append(child_id)
            
            children_ids.sort(key=lambda cid: (not tree_item_data[cid]['is_dir'], tree_item_data[cid]['name_only'].lower()))

        for i, child_id in enumerate(children_ids):
            data = tree_item_data[child_id]
            line = prefix + ("└── " if i == len(children_ids) - 1 else "├── ") + data['name_only']
            if data['is_dir']: line += "/"
            structure_lines.append(line)
            
            if data['is_dir']:
                new_prefix = prefix + ("    " if i == len(children_ids) - 1 else "│   ")
                _generate_recursive(child_id, new_prefix)

    if CHECKED_TAG in tree.item(root_id, 'tags') or TRISTATE_TAG in tree.item(root_id, 'tags'):
        _generate_recursive(root_id, "")
    
    return "<file_map>\n" + "\n".join(structure_lines) + "\n</file_map>" if len(structure_lines) > 1 else "Структура не сгенерирована: нет выбранных элементов."