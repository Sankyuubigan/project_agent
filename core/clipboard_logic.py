# clipboard_logic.py
import os
import sys 
import tkinter as tk
from pathlib import Path

try:
    import pyperclip
except ImportError:
    pyperclip = None

from core.treeview_logic import (
    generate_project_structure_text, 
    tree_item_data, tree_item_paths
)
from core.treeview_constants import CHECKED_TAG, TRISTATE_TAG
from core.fs_scanner_utils import DISABLED_LOOK_TAGS_UI
from core.file_processing import resource_path 
from core.project_structure_utils import generate_full_project_structure 
from core.vendor.gitignore_parser import Matcher

INSTRUCTION_FILE_NAMES = {
    "Markdown": "markdown_method.md",
    "Git": "git_method.md",
    "Diff-Match-Patch": "diffmatchpatch_method.md",
    "JSON": "json_method.md"
}
INSTRUCTIONS_SUBDIR_NAME = "doc" 

def copy_project_files(
    project_dir_entry_widget, 
    tree_widget, 
    log_widget_ref, 
    include_structure_var,      
    structure_type_var,         
    include_instructions_var,   
    apply_method_var            
):
    project_dir_str = project_dir_entry_widget.get().strip() 
    final_text_parts_list = [] 
    
    should_include_instructions = include_instructions_var.get()
    current_apply_method = apply_method_var.get()

    instruction_content_str = ""
    instruction_error_message = None
    instructions_were_included = False
    
    selected_instruction_file_name_str = INSTRUCTION_FILE_NAMES.get(current_apply_method)

    if should_include_instructions and selected_instruction_file_name_str:
        relative_path_to_instruction_file = Path(INSTRUCTIONS_SUBDIR_NAME) / selected_instruction_file_name_str
        
        resolved_path_candidate = resource_path(str(relative_path_to_instruction_file))
        if os.path.isfile(resolved_path_candidate):
            with open(resolved_path_candidate, 'r', encoding='utf-8') as f_instr: 
                instruction_content_str = f_instr.read()
            if log_widget_ref and log_widget_ref.winfo_exists(): 
                log_widget_ref.insert(tk.END, f"Инструкция '{selected_instruction_file_name_str}' успешно загружена.\n", ('success',))
        else:
            instruction_error_message = f"Файл инструкции '{selected_instruction_file_name_str}' не найден по пути: '{resolved_path_candidate}'."

        if instruction_content_str: 
            final_text_parts_list.append(instruction_content_str)
            final_text_parts_list.append("\n\n---\n\n") 
            instructions_were_included = True
        elif log_widget_ref and log_widget_ref.winfo_exists(): 
            if instruction_error_message: 
                 log_widget_ref.insert(tk.END, f"Информация: Инструкции ('{selected_instruction_file_name_str}') не включены. {instruction_error_message}\n", ('warning',))
            else: 
                 log_widget_ref.insert(tk.END, f"Информация: Инструкции ('{selected_instruction_file_name_str}') не включены (файл пуст или не найден).\n", ('warning',))
    
    elif should_include_instructions and not selected_instruction_file_name_str: 
         if log_widget_ref and log_widget_ref.winfo_exists(): log_widget_ref.insert(tk.END, f"Информация: Имя файла инструкции не определено для метода '{current_apply_method}'.\n", ('warning',))
    elif not should_include_instructions and log_widget_ref and log_widget_ref.winfo_exists(): 
        log_widget_ref.insert(tk.END, "Информация: Включение инструкций отключено пользователем.\n", ('info',))

    structure_text_output = ""
    structure_was_generated_and_included = False
    if include_structure_var.get(): 
        if not project_dir_str or not os.path.isdir(project_dir_str): 
            if log_widget_ref and log_widget_ref.winfo_exists(): log_widget_ref.insert(tk.END, "Информация: Структура проекта не будет включена (директория не указана).\n", ('info',))
        else:
            selected_structure_type = structure_type_var.get() 
            if selected_structure_type == "selected":
                if log_widget_ref and log_widget_ref.winfo_exists(): log_widget_ref.insert(tk.END, "Генерация структуры проекта (только выделенные)...\n", ('info',))
                structure_text_output = generate_project_structure_text(tree_widget, project_dir_str, log_widget_ref)
            elif selected_structure_type == "all":
                if log_widget_ref and log_widget_ref.winfo_exists(): log_widget_ref.insert(tk.END, "Генерация структуры проекта (все файлы)...\n", ('info',))
                
                current_gitignore_matcher = None
                project_root_path = Path(project_dir_str)
                path_to_gitignore_in_project = project_root_path / ".gitignore"
                if path_to_gitignore_in_project.is_file():
                    with path_to_gitignore_in_project.open('r', encoding='utf-8') as f_gi:
                        gi_lines = f_gi.readlines()
                    base_dir_str = str(path_to_gitignore_in_project.parent.resolve())
                    current_gitignore_matcher = Matcher(gi_lines, base_dir_str)
                    if log_widget_ref and log_widget_ref.winfo_exists():
                        log_widget_ref.insert(tk.END, f"Структура (полная): Используется .gitignore из '{path_to_gitignore_in_project}'.\n", ('info',))
                elif log_widget_ref and log_widget_ref.winfo_exists():
                    log_widget_ref.insert(tk.END, f"Структура (полная): Файл .gitignore не найден в '{project_dir_str}'.\n", ('info',))

                structure_text_output = generate_full_project_structure(project_dir_str, log_widget_ref, current_gitignore_matcher)
            
            if structure_text_output and not structure_text_output.startswith("Структура не сгенерирована"):
                final_text_parts_list.append(structure_text_output)
                final_text_parts_list.append("\n\n---\nСодержимое выбранных файлов:\n---\n\n")
                structure_was_generated_and_included = True
            elif log_widget_ref and log_widget_ref.winfo_exists(): 
                log_widget_ref.insert(tk.END, "Информация: Структура проекта не была сгенерирована или пуста.\n", ('info',))
    elif log_widget_ref and log_widget_ref.winfo_exists(): 
        log_widget_ref.insert(tk.END, "Информация: Включение структуры проекта отключено пользователем.\n", ('info',))

    file_blocks = []
    num_files_copied = 0
    
    all_item_ids_in_tree = [] 
    def _collect_all_tree_ids_safe(parent_id_str=""): 
        if tree_widget.exists(parent_id_str) or parent_id_str == "":
            for child_id_str in tree_widget.get_children(parent_id_str):
                if tree_widget.exists(child_id_str): 
                    all_item_ids_in_tree.append(child_id_str)
                    _collect_all_tree_ids_safe(child_id_str)
    
    _collect_all_tree_ids_safe() 

    for item_id_str_from_tree in all_item_ids_in_tree:
        if item_id_str_from_tree not in tree_item_data or not tree_item_data[item_id_str_from_tree].get('is_file'):
            continue 
        
        if not tree_widget.exists(item_id_str_from_tree):
            continue
            
        current_item_tags_set = set(tree_widget.item(item_id_str_from_tree, 'tags'))

        if CHECKED_TAG in current_item_tags_set and not DISABLED_LOOK_TAGS_UI.intersection(current_item_tags_set):
            abs_file_path_str = tree_item_paths.get(item_id_str_from_tree) 
            if not abs_file_path_str: continue 

            file_path_obj = Path(abs_file_path_str)
            relative_path_for_display = tree_item_data[item_id_str_from_tree].get('rel_path', file_path_obj.name)
            
            with open(file_path_obj, 'r', encoding='utf-8') as f_content:
                file_content_str = f_content.read()
            
            file_blocks.append(f"<<<FILE: {relative_path_for_display}>>>\n{file_content_str}\n<<<END_FILE>>>")
            num_files_copied += 1

    if file_blocks: 
        final_text_parts_list.append("\n\n".join(file_blocks))
    
    final_text_to_copy = "".join(final_text_parts_list).strip()
    
    if not final_text_to_copy: 
        if log_widget_ref and log_widget_ref.winfo_exists(): log_widget_ref.insert(tk.END, "Нет данных для копирования.\n", ('info',));
        return 

    if not pyperclip:
        if log_widget_ref and log_widget_ref.winfo_exists():
            log_widget_ref.insert(tk.END, "Ошибка: библиотека pyperclip не найдена. Копирование невозможно.\n", ('error',))
        return

    pyperclip.copy(final_text_to_copy)
    
    success_message_parts = []
    if instructions_were_included and selected_instruction_file_name_str:
        success_message_parts.append(f"инструкции ('{selected_instruction_file_name_str}')")
    if structure_was_generated_and_included:
        success_message_parts.append("структура проекта")
    if num_files_copied > 0:
        files_word = "файл" if num_files_copied == 1 else "файла" if 1 < num_files_copied < 5 else "файлов"
        success_message_parts.append(f"{num_files_copied} {files_word}")
    
    final_success_message = ""
    if success_message_parts:
        final_success_message = f"{', '.join(success_message_parts).capitalize()} скопировано в буфер обмена!"
    else: 
         final_success_message = "Данные скопированы в буфер обмена."

    if log_widget_ref and log_widget_ref.winfo_exists():
        log_widget_ref.insert(tk.END, final_success_message + "\n", ('success',))
        log_widget_ref.see(tk.END)