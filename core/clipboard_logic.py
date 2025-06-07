# clipboard_logic.py
import os
import sys 
import tkinter as tk # Для tk.END и type hinting
from pathlib import Path
import pyperclip # Для копирования в буфер обмена

# Импортируем функции и константы из других модулей проекта
from core.treeview_logic import ( # Используем абсолютный импорт от корня пакета
    generate_project_structure_text, 
    tree_item_data, tree_item_paths,
    CHECKED_TAG, DISABLED_LOOK_TAGS_UI 
)
from core.file_processing import resource_path 
from core.project_structure_utils import generate_full_project_structure 

try:
    from gitignore_parser import parse_gitignore
except ImportError:
    parse_gitignore = None 

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
        
        absolute_instruction_file_path = ""
        try:
            resolved_path_candidate = resource_path(str(relative_path_to_instruction_file))
            if os.path.isfile(resolved_path_candidate): 
                 absolute_instruction_file_path = resolved_path_candidate
            else:
                instruction_error_message = f"Файл инструкции '{selected_instruction_file_name_str}' не найден по пути ресурса: '{resolved_path_candidate}'."
        except Exception as e_res_path: 
            instruction_error_message = f"Ошибка доступа к ресурсу инструкции '{selected_instruction_file_name_str}': {e_res_path}"

        if absolute_instruction_file_path: 
            try:
                with open(absolute_instruction_file_path, 'r', encoding='utf-8') as f_instr: 
                    instruction_content_str = f_instr.read()
                if log_widget_ref: 
                    log_widget_ref.insert(tk.END, f"Инструкция '{selected_instruction_file_name_str}' успешно загружена.\n", ('success',))
            except OSError as e_read_instr:
                instruction_error_message = f"Ошибка чтения файла инструкции '{selected_instruction_file_name_str}': {e_read_instr.strerror}"
            except Exception as e_generic_instr:
                 instruction_error_message = f"Неожиданная ошибка при загрузке инструкции '{selected_instruction_file_name_str}': {e_generic_instr}"
        
        if instruction_content_str: 
            final_text_parts_list.append(instruction_content_str)
            final_text_parts_list.append("\n\n---\n\n") 
            instructions_were_included = True
        elif log_widget_ref: 
            if instruction_error_message: 
                 log_widget_ref.insert(tk.END, f"Информация: Инструкции ('{selected_instruction_file_name_str}') не включены. {instruction_error_message}\n", ('warning',))
            else: 
                 log_widget_ref.insert(tk.END, f"Информация: Инструкции ('{selected_instruction_file_name_str}') не включены (файл пуст или не найден без явной ошибки).\n", ('warning',))
    
    elif should_include_instructions and not selected_instruction_file_name_str: 
         if log_widget_ref: log_widget_ref.insert(tk.END, f"Информация: Имя файла инструкции не определено для метода '{current_apply_method}'. Инструкции не включены.\n", ('warning',))
    elif not should_include_instructions and log_widget_ref: 
        log_widget_ref.insert(tk.END, "Информация: Включение инструкций отключено пользователем.\n", ('info',))

    structure_text_output = ""
    structure_was_generated_and_included = False
    if include_structure_var.get(): 
        if not project_dir_str or not os.path.isdir(project_dir_str): 
            if log_widget_ref: log_widget_ref.insert(tk.END, "Информация: Структура проекта не будет включена (директория проекта не указана или неверна).\n", ('info',))
        else:
            selected_structure_type = structure_type_var.get() 
            if selected_structure_type == "selected":
                if log_widget_ref: log_widget_ref.insert(tk.END, "Генерация структуры проекта (только выделенные)...\n", ('info',))
                structure_text_output = generate_project_structure_text(tree_widget, project_dir_str, log_widget_ref)
            elif selected_structure_type == "all":
                if log_widget_ref: log_widget_ref.insert(tk.END, "Генерация структуры проекта (все файлы)...\n", ('info',))
                current_gitignore_matcher = None
                if parse_gitignore: 
                    path_to_gitignore_in_project = Path(project_dir_str) / ".gitignore"
                    if path_to_gitignore_in_project.is_file():
                        try:
                            current_gitignore_matcher = parse_gitignore(str(path_to_gitignore_in_project.resolve()))
                            if log_widget_ref:
                                log_widget_ref.insert(tk.END, f"Структура (полная): Используется .gitignore из '{path_to_gitignore_in_project}'.\n", ('info',))
                        except Exception as e_parse_gi:
                            if log_widget_ref:
                                log_widget_ref.insert(tk.END, f"Структура (полная): Ошибка разбора .gitignore ('{path_to_gitignore_in_project}'): {e_parse_gi}\n", ('warning',))
                    elif log_widget_ref:
                        log_widget_ref.insert(tk.END, f"Структура (полная): Файл .gitignore не найден в '{project_dir_str}'.\n", ('info',))
                elif log_widget_ref: 
                     log_widget_ref.insert(tk.END, "Структура (полная): Библиотека gitignore-parser не найдена, .gitignore не будет использоваться.\n", ('info',))

                structure_text_output = generate_full_project_structure(project_dir_str, log_widget_ref, current_gitignore_matcher)
            
            if structure_text_output and not structure_text_output.startswith("Структура не сгенерирована"):
                final_text_parts_list.append(structure_text_output)
                final_text_parts_list.append("\n\n---\nСодержимое выбранных файлов:\n---\n\n")
                structure_was_generated_and_included = True
            elif log_widget_ref: 
                log_widget_ref.insert(tk.END, "Информация: Структура проекта не была сгенерирована или пуста.\n", ('info',))
    elif log_widget_ref: 
        log_widget_ref.insert(tk.END, "Информация: Включение структуры проекта отключено пользователем.\n", ('info',))

    markdown_blocks_for_files = []
    num_files_copied = 0
    
    all_item_ids_in_tree = [] 
    def _collect_all_tree_ids_safe(parent_id_str=""): 
        children_ids_list = []
        if tree_widget.exists(parent_id_str) or parent_id_str == "":
            try: children_ids_list = tree_widget.get_children(parent_id_str)
            except tk.TclError: return 
        
        for child_id_str in children_ids_list:
            if tree_widget.exists(child_id_str): 
                all_item_ids_in_tree.append(child_id_str)
                _collect_all_tree_ids_safe(child_id_str)
    try:
        _collect_all_tree_ids_safe() 
    except tk.TclError: 
        if log_widget_ref: log_widget_ref.insert(tk.END, "Предупреждение: Ошибка доступа к дереву файлов при сборе ID для копирования.\n", ('warning',))

    for item_id_str_from_tree in all_item_ids_in_tree:
        if item_id_str_from_tree not in tree_item_data or not tree_item_data[item_id_str_from_tree].get('is_file'):
            continue 
            
        try: 
            current_item_tags_set = set(tree_widget.item(item_id_str_from_tree, 'tags'))
        except tk.TclError: 
            if log_widget_ref: log_widget_ref.insert(tk.END, f"Предупреждение: Ошибка доступа к тегам элемента {item_id_str_from_tree} (возможно, удален).\n", ('warning',))
            continue 

        if CHECKED_TAG in current_item_tags_set and not DISABLED_LOOK_TAGS_UI.intersection(current_item_tags_set):
            abs_file_path_str = tree_item_paths.get(item_id_str_from_tree) 
            if not abs_file_path_str: continue 

            file_path_obj = Path(abs_file_path_str)
            relative_path_for_display = tree_item_data[item_id_str_from_tree].get('rel_path', file_path_obj.name)
            
            file_content_str = ""
            error_reading_file = False
            try: 
                with open(file_path_obj, 'r', encoding='utf-8') as f_content:
                    file_content_str = f_content.read()
            except UnicodeDecodeError: 
                markdown_blocks_for_files.append(f"<<<FILE: {relative_path_for_display}>>>\n```\n[бинарный файл или ошибка декодирования]\n```\n<<<END_FILE>>>")
                error_reading_file = True
            except OSError as e_read_os:
                markdown_blocks_for_files.append(f"<<<FILE: {relative_path_for_display}>>>\n```\n[ошибка чтения файла (OS): {e_read_os.strerror}]\n```\n<<<END_FILE>>>")
                error_reading_file = True
            except Exception as e_read_generic:
                markdown_blocks_for_files.append(f"<<<FILE: {relative_path_for_display}>>>\n```\n[ошибка чтения файла: {e_read_generic}]\n```\n<<<END_FILE>>>")
                error_reading_file = True

            if not error_reading_file: 
                file_extension = file_path_obj.suffix[1:].lower() if file_path_obj.suffix else ""
                markdown_blocks_for_files.append(f"<<<FILE: {relative_path_for_display}>>>\n```{file_extension}\n{file_content_str}\n```\n<<<END_FILE>>>")
                num_files_copied += 1

    if markdown_blocks_for_files: 
        final_text_parts_list.append("\n\n".join(markdown_blocks_for_files))
    
    final_text_to_copy = "".join(final_text_parts_list).strip()
    
    if not final_text_to_copy: 
        if log_widget_ref: log_widget_ref.insert(tk.END, "Нет данных для копирования (проверьте выбор файлов и опции).\n", ('info',));
        return 

    try: 
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
        elif num_files_copied == 0 and not structure_was_generated_and_included and not instructions_were_included:
            final_success_message = "Нет данных для копирования (файлы не выбраны, опции структуры/инструкций отключены или пусты)."
        else: 
             final_success_message = "Данные скопированы в буфер обмена (проверьте настройки и выбор файлов)."

        if log_widget_ref: log_widget_ref.insert(tk.END, final_success_message + "\n", ('success',))
    
    except pyperclip.PyperclipException as e_pyperclip: 
        if log_widget_ref: log_widget_ref.insert(tk.END, f"Ошибка копирования pyperclip: {e_pyperclip}\nВозможно, буфер обмена недоступен или используется другой программой.\nВ Linux: попробуйте 'sudo apt-get install xclip' или 'sudo apt-get install xsel'.\n", ('error',))
    except Exception as e_copy_generic: 
        if log_widget_ref:
            log_widget_ref.insert(tk.END, f"Неожиданная ошибка при копировании в буфер: {e_copy_generic}\nДАННЫЕ (первые 500 символов):\n{final_text_to_copy[:500]}\n", ('error',)) 
    
    if log_widget_ref: 
        try: log_widget_ref.see(tk.END)
        except tk.TclError: pass 