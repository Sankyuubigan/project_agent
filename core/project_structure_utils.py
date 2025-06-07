# core/project_structure_utils.py
import os
from pathlib import Path
import tkinter as tk # Для type hinting и tk.END

# Используем утилиты из fs_scanner_utils
from core.fs_scanner_utils import should_exclude_item # Используем абсолютный импорт

# Символы для отрисовки дерева
LINE_VERTICAL = "│   "
LINE_INTERSECTION = "├── "
LINE_CORNER = "└── "
LINE_EMPTY = "    "


def _generate_structure_recursive_for_full(
    current_dir_to_scan: Path,
    project_root_obj: Path,
    prefix_str: str, # Строка префикса с линиями от родительских уровней
    gitignore_matcher_func,
    log_widget_ref # log_widget_ref теперь обязателен для передачи
):
    """
    Рекурсивно генерирует строки для древовидной структуры с использованием псевдографики.
    """
    lines = []
    
    items_in_current_dir = []
    if not (os.access(str(current_dir_to_scan), os.R_OK) and os.access(str(current_dir_to_scan), os.X_OK)):
        return lines 

    try:
        all_items = list(current_dir_to_scan.iterdir())
        for item_obj in all_items:
            if not should_exclude_item(item_obj.resolve(), item_obj.name, item_obj.is_dir(), gitignore_matcher_func):
                items_in_current_dir.append(item_obj)
        
        items_in_current_dir.sort(key=lambda x: (not x.is_dir(), x.name.lower()))
    except OSError:
        return lines

    num_items = len(items_in_current_dir)
    for i, item_obj in enumerate(items_in_current_dir):
        item_name = item_obj.name
        is_item_dir = item_obj.is_dir()
        is_last_item = (i == num_items - 1)

        path_for_display_str = ""
        if project_root_obj == item_obj: 
            path_for_display_str = item_name
        else:
            try:
                path_for_display_str = item_obj.relative_to(current_dir_to_scan).as_posix()
                if path_for_display_str == ".": 
                    path_for_display_str = item_name
            except ValueError:
                path_for_display_str = item_name

        entry_line = prefix_str
        if is_last_item:
            entry_line += LINE_CORNER
            new_prefix_for_children = prefix_str + LINE_EMPTY
        else:
            entry_line += LINE_INTERSECTION
            new_prefix_for_children = prefix_str + LINE_VERTICAL
        
        entry_line += path_for_display_str

        if is_item_dir:
            entry_line += "/"
            lines.append(entry_line)
            lines.extend(_generate_structure_recursive_for_full(
                item_obj,
                project_root_obj, 
                new_prefix_for_children,
                gitignore_matcher_func,
                log_widget_ref
            ))
        else: 
            lines.append(entry_line)
            
    return lines

def generate_full_project_structure(project_dir_path_str: str, log_widget_ref, gitignore_matcher_func):
    """
    Генерирует текстовое древовидное представление всех файлов и папок проекта
    с использованием псевдографики.
    """
    if not project_dir_path_str:
        if log_widget_ref:
            try:
                log_widget_ref.insert(tk.END, "Структура (полная): Путь к директории проекта не указан.\n", ('warning',))
            except tk.TclError: pass
        return "Структура не сгенерирована: путь к проекту не указан."
        
    project_root_obj = Path(project_dir_path_str)
    if not project_root_obj.is_dir():
        if log_widget_ref:
            try:
                log_widget_ref.insert(tk.END, f"Структура (полная): Директория '{project_dir_path_str}' не найдена или не является директорией.\n", ('warning',))
            except tk.TclError: pass
        return "Структура не сгенерирована: неверная корневая директория."
    
    project_root_obj = project_root_obj.resolve()

    structure_lines = [project_root_obj.name] 

    if os.access(str(project_root_obj), os.R_OK) and os.access(str(project_root_obj), os.X_OK):
        structure_lines.extend(_generate_structure_recursive_for_full(
            project_root_obj,    
            project_root_obj,    
            "",                  
            gitignore_matcher_func,
            log_widget_ref
        ))
    elif log_widget_ref:
        try:
            log_widget_ref.insert(tk.END, f"Структура (полная): Нет доступа к корневой директории '{project_root_obj}'.\n", ('warning',))
        except tk.TclError: pass

    # Исправленная строка 130 (теперь 121 после удаления пустых строк)
    if len(structure_lines) <= 1 and not structure_lines[0].strip().endswith("/"): # Только имя папки, без содержимого
        has_visible_content = False
        if os.access(str(project_root_obj), os.R_OK) and os.access(str(project_root_obj), os.X_OK):
            try:
                for item in project_root_obj.iterdir():
                    if not should_exclude_item(item.resolve(), item.name, item.is_dir(), gitignore_matcher_func):
                        has_visible_content = True
                        break
            except OSError:
                pass 
        
        if not has_visible_content and not structure_lines[0].endswith("/"):
             structure_lines[0] += "/" 


    if len(structure_lines) <= 1 and not (len(structure_lines) == 1 and structure_lines[0].strip()): 
        if log_widget_ref:
            try:
                log_widget_ref.insert(tk.END, "Структура (полная): Не найдено элементов для отображения (возможно, все игнорируется или папка пуста).\n", ('warning',))
            except tk.TclError: pass
        return "Структура не сгенерирована: нет элементов для отображения."
        
    return "<file_map>\n" + "\n".join(structure_lines) + "\n</file_map>"