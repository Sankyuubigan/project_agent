# core/project_structure_utils.py
import os
from pathlib import Path
import tkinter as tk

from core.fs_scanner_utils import should_exclude_item

LINE_VERTICAL = "│   "
LINE_INTERSECTION = "├── "
LINE_CORNER = "└── "
LINE_EMPTY = "    "

def _generate_structure_recursive_for_full(
    current_dir_to_scan: Path,
    project_root_obj: Path,
    prefix_str: str,
    gitignore_matcher_func,
    log_widget_ref
):
    """Recursively generates tree structure strings."""
    lines = []
    
    if not (os.access(str(current_dir_to_scan), os.R_OK) and os.access(str(current_dir_to_scan), os.X_OK)):
        return lines 

    # This will crash on permission errors for iterdir().
    all_items = list(current_dir_to_scan.iterdir())
    items_in_current_dir = []
    for item_obj in all_items:
        if not should_exclude_item(item_obj.resolve(), item_obj.name, item_obj.is_dir(), gitignore_matcher_func):
            items_in_current_dir.append(item_obj)
    
    items_in_current_dir.sort(key=lambda x: (not x.is_dir(), x.name.lower()))

    num_items = len(items_in_current_dir)
    for i, item_obj in enumerate(items_in_current_dir):
        is_last_item = (i == num_items - 1)
        entry_line = prefix_str
        if is_last_item:
            entry_line += LINE_CORNER
            new_prefix_for_children = prefix_str + LINE_EMPTY
        else:
            entry_line += LINE_INTERSECTION
            new_prefix_for_children = prefix_str + LINE_VERTICAL
        
        entry_line += item_obj.name

        if item_obj.is_dir():
            entry_line += "/"
            lines.append(entry_line)
            lines.extend(_generate_structure_recursive_for_full(
                item_obj, project_root_obj, new_prefix_for_children,
                gitignore_matcher_func, log_widget_ref
            ))
        else: 
            lines.append(entry_line)
            
    return lines

def generate_full_project_structure(project_dir_path_str: str, log_widget_ref, gitignore_matcher_func):
    """Generates a text tree of the entire project."""
    if not project_dir_path_str:
        if log_widget_ref and log_widget_ref.winfo_exists():
            log_widget_ref.insert(tk.END, "Структура (полная): Путь к директории проекта не указан.\n", ('warning',))
        return "Структура не сгенерирована: путь к проекту не указан."
        
    project_root_obj = Path(project_dir_path_str)
    if not project_root_obj.is_dir():
        if log_widget_ref and log_widget_ref.winfo_exists():
            log_widget_ref.insert(tk.END, f"Структура (полная): Директория '{project_dir_path_str}' не найдена.\n", ('warning',))
        return "Структура не сгенерирована: неверная корневая директория."
    
    project_root_obj = project_root_obj.resolve()
    structure_lines = [project_root_obj.name] 

    if os.access(str(project_root_obj), os.R_OK) and os.access(str(project_root_obj), os.X_OK):
        structure_lines.extend(_generate_structure_recursive_for_full(
            project_root_obj, project_root_obj, "", gitignore_matcher_func, log_widget_ref
        ))
    elif log_widget_ref and log_widget_ref.winfo_exists():
        log_widget_ref.insert(tk.END, f"Структура (полная): Нет доступа к корневой директории '{project_root_obj}'.\n", ('warning',))

    if len(structure_lines) <= 1:
        if log_widget_ref and log_widget_ref.winfo_exists():
            log_widget_ref.insert(tk.END, "Структура (полная): Не найдено элементов для отображения.\n", ('warning',))
        return "Структура не сгенерирована: нет элементов для отображения."
        
    return "<file_map>\n" + "\n".join(structure_lines) + "\n</file_map>"