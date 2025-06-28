# core/treeview_scanner.py
import os
import threading
from pathlib import Path

from core.fs_scanner_utils import (
    should_exclude_item, get_item_status_info,
    DISABLED_LOOK_TAGS_UI
)
from core.vendor.gitignore_parser import Matcher
# --- ИЗМЕНЕНИЕ: Импорт из нового файла констант ---
from core.treeview_constants import CHECKED_TAG, UNCHECKED_TAG

def _populate_recursive_scan_and_count_tokens(
    cur_dir_obj: Path,
    parent_id_str: str,
    root_dir_obj: Path,
    update_queue,
    log_widget_ref,
    gitignore_matcher_func
):
    total_tokens_folder = 0
    update_queue.put(("progress_step", cur_dir_obj.name))

    try:
        if not (os.access(str(cur_dir_obj), os.R_OK) and os.access(str(cur_dir_obj), os.X_OK)):
            raise PermissionError(f"Отказ в доступе к '{cur_dir_obj.name}'")
        
        items = sorted(cur_dir_obj.iterdir(), key=lambda x: (not x.is_dir(), x.name.lower()))
    except (PermissionError, OSError) as e:
        perm_err_msg = str(e)
        update_queue.put(("log_message", (f"LOG_REC_SCAN: ПРЕДУПРЕЖДЕНИЕ: {perm_err_msg}", ('warning',))))
        update_queue.put(("update_node_data", (parent_id_str, 0, "ошибка доступа")))
        return 0

    for item_path_obj in items:
        item_name, is_dir, item_id_str = item_path_obj.name, item_path_obj.is_dir(), str(item_path_obj)

        if should_exclude_item(item_path_obj, item_name, is_dir, gitignore_matcher_func):
            continue

        status_tags, status_msg, file_tokens = get_item_status_info(item_path_obj, item_name, is_dir, log_widget_ref)
        
        if not DISABLED_LOOK_TAGS_UI.intersection(status_tags):
            status_tags.add(CHECKED_TAG)
        else:
            status_tags.add(UNCHECKED_TAG)

        rel_path = str(item_path_obj.relative_to(root_dir_obj)) if root_dir_obj in item_path_obj.parents else item_name
        data_dict = {
            'name_only': item_name, 'is_dir': is_dir, 'is_file': not is_dir,
            'rel_path': rel_path, 'tokens': file_tokens if not is_dir else 0,
            'status_msg': status_msg
        }
        update_queue.put(("add_node", (parent_id_str, item_id_str, tuple(status_tags), str(item_path_obj), data_dict)))

        if is_dir:
            tokens_subdir = _populate_recursive_scan_and_count_tokens(
                item_path_obj, item_id_str, root_dir_obj, update_queue, log_widget_ref, gitignore_matcher_func
            )
            if isinstance(tokens_subdir, (int, float)) and tokens_subdir >= 0:
                total_tokens_folder += tokens_subdir
                update_queue.put(("update_node_data", (item_id_str, tokens_subdir, data_dict.get('status_msg', ''))))
        elif file_tokens is not None and isinstance(file_tokens, (int, float)) and file_tokens >= 0:
            total_tokens_folder += file_tokens

    return total_tokens_folder

def scan_directory_and_populate_queue(abs_dir_path_str, update_queue, log_widget_ref):
    root_dir_obj = Path(abs_dir_path_str)
    local_gitignore_matcher = None

    gi_file = root_dir_obj / ".gitignore"
    if gi_file.is_file():
        try:
            with gi_file.open('r', encoding='utf-8') as f:
                lines = f.readlines()
            base_dir = str(gi_file.parent.resolve())
            local_gitignore_matcher = Matcher(lines, base_dir)
        except Exception as e:
            update_queue.put(("log_message", (f"Не удалось прочитать .gitignore: {e}", ('warning',))))


    update_queue.put(("progress_start", None))
    root_name, root_id = root_dir_obj.name, str(root_dir_obj)
    root_ui_tags, root_status, _ = get_item_status_info(root_dir_obj, root_name, True, log_widget_ref)
    root_ui_tags.add(CHECKED_TAG)
    
    root_data = {
        'name_only': root_name, 'is_dir': True, 'is_file': False, 'rel_path': "",
        'tokens': 0, 'status_msg': root_status
    }
    update_queue.put(("add_node", ("", root_id, tuple(root_ui_tags), str(root_dir_obj), root_data)))

    tokens_root = _populate_recursive_scan_and_count_tokens(
        root_dir_obj, root_id, root_dir_obj, update_queue, log_widget_ref, local_gitignore_matcher
    )

    if isinstance(tokens_root, (int, float)) and tokens_root >= 0:
        update_queue.put(("update_node_data", (root_id, tokens_root, root_status)))
    
    update_queue.put(("finished", None))