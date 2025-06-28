# core/treeview_scanner.py
import os
import threading
from pathlib import Path

from core.fs_scanner_utils import (
    should_exclude_item, get_item_status_info,
    DISABLED_LOOK_TAGS_UI, TOO_MANY_TOKENS_STATUS_TAG,
    ERROR_STATUS_TAG, BINARY_STATUS_TAG
)
from core.vendor.gitignore_parser import Matcher
from core.treeview_constants import CHECKED_TAG, UNCHECKED_TAG
from core.file_processing import count_file_tokens, MAX_TOKENS_FOR_DISPLAY

def _populate_recursive_scan(
    cur_dir_obj: Path,
    parent_id_str: str,
    root_dir_obj: Path,
    update_queue,
    log_widget_ref,
    gitignore_matcher_func
):
    update_queue.put(("progress_step", cur_dir_obj.name))

    try:
        if not (os.access(str(cur_dir_obj), os.R_OK) and os.access(str(cur_dir_obj), os.X_OK)):
            raise PermissionError(f"Отказ в доступе к '{cur_dir_obj.name}'")
        
        items = sorted(cur_dir_obj.iterdir(), key=lambda x: (not x.is_dir(), x.name.lower()))
    except (PermissionError, OSError) as e:
        perm_err_msg = str(e)
        update_queue.put(("log_message", (f"LOG_REC_SCAN: ПРЕДУПРЕЖДЕНИЕ: {perm_err_msg}", ('warning',))))
        update_queue.put(("update_node_data", (parent_id_str, 0, "ошибка доступа")))
        return

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
            'rel_path': rel_path, 'tokens': file_tokens,
            'status_msg': status_msg
        }
        update_queue.put(("add_node", (parent_id_str, item_id_str, tuple(status_tags), str(item_path_obj), data_dict)))

        if is_dir:
            _populate_recursive_scan(
                item_path_obj, item_id_str, root_dir_obj, update_queue, log_widget_ref, gitignore_matcher_func
            )

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

    _populate_recursive_scan(
        root_dir_obj, root_id, root_dir_obj, update_queue, log_widget_ref, local_gitignore_matcher
    )
    
    update_queue.put(("finished", "initial_scan"))

def token_calculation_worker(item_ids_to_process, update_queue, log_widget_ref):
    """
    Worker thread function to calculate tokens for a given list of file item IDs.
    """
    from core.treeview_logic import tree_item_paths 

    update_queue.put(("progress_start", None))
    update_queue.put(("log_message", ("Начат подсчет токенов для выбранных файлов...", ('info',))))
    
    processed_count = 0
    total_count = len(item_ids_to_process)

    for item_id in item_ids_to_process:
        processed_count += 1
        file_path_str = tree_item_paths.get(item_id)
        if not file_path_str:
            continue

        file_path_obj = Path(file_path_str)
        update_queue.put(("progress_step", f"({processed_count}/{total_count}) {file_path_obj.name}"))

        token_val, token_err_msg = count_file_tokens(file_path_str, log_widget_ref)
        
        new_status_msg = ""
        new_tags_to_add = set()

        if token_err_msg:
            new_status_msg = token_err_msg
            if "бинарный" in token_err_msg:
                new_tags_to_add.add(BINARY_STATUS_TAG)
            else:
                new_tags_to_add.add(ERROR_STATUS_TAG)
        elif token_val is not None:
            if token_val > MAX_TOKENS_FOR_DISPLAY:
                new_tags_to_add.add(TOO_MANY_TOKENS_TAG_UI)
                formatted_max = f"{MAX_TOKENS_FOR_DISPLAY:,}".replace(",", " ")
                new_status_msg = f"токенов > {formatted_max}"

        update_queue.put(("update_node_after_token_count", (item_id, token_val, new_status_msg, new_tags_to_add)))

    update_queue.put(("log_message", ("Подсчет токенов для файлов завершен. Обновление папок...", ('info',))))
    update_queue.put(("recalculate_folder_tokens", None))
    update_queue.put(("finished", "token_count"))