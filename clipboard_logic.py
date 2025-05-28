# clipboard_logic.py
import os
import sys 
import tkinter as tk
from pathlib import Path
import pyperclip

from treeview_logic import (
    generate_project_structure_text, tree_item_data, tree_item_paths,
    CHECKED_TAG, FILE_TAG, BINARY_TAG, LARGE_FILE_TAG, ERROR_TAG, TOO_MANY_TOKENS_TAG,
    DISABLED_LOOK_TAGS
)
from file_processing import resource_path # Теперь resource_path снова важен

INSTRUCTION_FILE_NAMES = {
    "Markdown": "markdown_method.md",
    "Git": "git_method.md",
    "Diff-Match-Patch": "diffmatchpatch_method.md",
    "JSON": "json_method.md"
}
INSTRUCTIONS_SUBDIR_NAME = "doc" 

def copy_project_files(project_dir_entry, tree_widget, log_widget_ref, 
                       include_structure_var, include_instructions_var, apply_method_var):
    project_dir_str = project_dir_entry.get().strip()
    final_text_parts = []
    
    include_instructions = include_instructions_var.get()
    apply_method = apply_method_var.get()

    doc_content = ""; doc_error_msg = None; instructions_included_flag = False
    selected_instruction_file_name = INSTRUCTION_FILE_NAMES.get(apply_method)

    if include_instructions and selected_instruction_file_name:
        # Формируем относительный путь внутри пакета/ресурсов
        relative_path_to_instruction = Path(INSTRUCTIONS_SUBDIR_NAME) / selected_instruction_file_name
        
        try:
            # Пытаемся получить абсолютный путь к ресурсу через resource_path
            instruction_file_path_resolved = resource_path(str(relative_path_to_instruction))
            
            if log_widget_ref:
                log_widget_ref.insert(tk.END, f"Поиск файла инструкции через resource_path: '{relative_path_to_instruction}' -> '{instruction_file_path_resolved}'\n", ('info',))

            if os.path.isfile(instruction_file_path_resolved):
                with open(instruction_file_path_resolved, 'r', encoding='utf-8') as f: 
                    doc_content = f.read()
                if log_widget_ref: 
                    log_widget_ref.insert(tk.END, f"Инструкция '{selected_instruction_file_name}' успешно загружена.\n", ('success',))
            else:
                doc_error_msg = f"Файл инструкции '{selected_instruction_file_name}' не найден по пути ресурса: '{instruction_file_path_resolved}'."
        except Exception as e: 
            doc_error_msg = f"Ошибка доступа к файлу инструкции '{selected_instruction_file_name}': {e}"
        
        if doc_content:
            final_text_parts.append(doc_content)
            final_text_parts.append("\n\n---\n\n") 
            instructions_included_flag = True
        elif log_widget_ref:
            if doc_error_msg: 
                 log_widget_ref.insert(tk.END, f"Информация: Инструкции ('{selected_instruction_file_name}') не включены. {doc_error_msg}\n", ('warning',))
            else: # Ошибки не было, но контент пуст
                 log_widget_ref.insert(tk.END, f"Информация: Инструкции ('{selected_instruction_file_name}') не включены (файл пуст по пути ресурса).\n", ('warning',))
    
    elif include_instructions and not selected_instruction_file_name:
         if log_widget_ref: log_widget_ref.insert(tk.END, f"Информация: Имя файла инструкции не определено для метода '{apply_method}'.\n", ('warning',))
    elif not include_instructions and log_widget_ref:
        log_widget_ref.insert(tk.END, f"Информация: Инструкции не будут включены (опция отключена).\n", ('info',))

    # ... (остальная часть функции без изменений: структура, файлы, копирование) ...
    # Важно: project_dir_str используется для generate_project_structure_text и для путей к файлам проекта,
    # но НЕ для файлов инструкций, если они в сборке.

    structure_generated_and_included = False
    include_structure = include_structure_var.get()
    if include_structure:
        if not project_dir_str or not os.path.isdir(project_dir_str):
            if log_widget_ref: log_widget_ref.insert(tk.END, "Информация: Структура проекта не будет включена (директория проекта не указана или неверна).\n", ('info',))
        else:
            structure_text = generate_project_structure_text(tree_widget, project_dir_str, log_widget_ref)
            if structure_text and not structure_text.startswith("Структура не сгенерирована"):
                final_text_parts.append(structure_text)
                final_text_parts.append("\n\n---\nСодержимое выбранных файлов:\n---\n\n")
                structure_generated_and_included = True
            elif log_widget_ref:
                log_widget_ref.insert(tk.END, "Информация: Структура проекта не будет включена (не сгенерирована или нет выбранных элементов).\n", ('info',))
    elif log_widget_ref:
        log_widget_ref.insert(tk.END, "Информация: Структура проекта не будет включена (опция отключена).\n", ('info',))

    markdown_blocks = []
    copied_count = 0
    all_ids = []
    def _collect_ids(parent=""):
        try: 
            for child in tree_widget.get_children(parent): all_ids.append(child); _collect_ids(child)
        except tk.TclError:
            if log_widget_ref: log_widget_ref.insert(tk.END, "Предупреждение: Ошибка доступа к дереву файлов при сборе ID (возможно, виджет удален).\n", ('warning',))
            return 
    _collect_ids()

    for item_id in all_ids:
        if item_id not in tree_item_data or not tree_item_data[item_id].get('is_file'): continue
        try: 
            tags = set(tree_widget.item(item_id, 'tags'))
        except tk.TclError:
            if log_widget_ref: log_widget_ref.insert(tk.END, f"Предупреждение: Ошибка доступа к тегам элемента {item_id} (возможно, виджет удален).\n", ('warning',))
            continue 

        if CHECKED_TAG in tags and not DISABLED_LOOK_TAGS.intersection(tags):
            path_str = tree_item_paths.get(item_id) # Это абсолютный путь к файлу на диске
            if not path_str: continue
            path_obj = Path(path_str)
            rel_path_for_display = tree_item_data[item_id].get('rel_path', path_obj.name)
            try:
                with open(path_obj, 'r', encoding='utf-8') as f: content = f.read()
                lang = path_obj.suffix[1:].lower() if path_obj.suffix else ""
                markdown_blocks.append(f"<<<FILE: {rel_path_for_display}>>>\n```{lang}\n{content}\n```\n<<<END_FILE>>>")
                copied_count += 1
            except UnicodeDecodeError: markdown_blocks.append(f"<<<FILE: {rel_path_for_display}>>>\n```\n[binary file or read error]\n```\n<<<END_FILE>>>")
            except Exception as e: markdown_blocks.append(f"<<<FILE: {rel_path_for_display}>>>\n```\n[error reading file: {e}]\n```\n<<<END_FILE>>>")

    if markdown_blocks:
        final_text_parts.append("\n\n".join(markdown_blocks))
    
    final_text = "".join(final_text_parts).strip()
    if not final_text:
        if log_widget_ref: log_widget_ref.insert(tk.END, "Нет данных для копирования.\n", ('info',)); return
    try:
        pyperclip.copy(final_text)
        msg_parts = []
        if instructions_included_flag and selected_instruction_file_name: msg_parts.append(f"инструкции ('{selected_instruction_file_name}')")
        if structure_generated_and_included: msg_parts.append("структура")
        if copied_count > 0: msg_parts.append(f"{copied_count} файл(ов)")
        
        if msg_parts:
            msg = f"{', '.join(msg_parts).capitalize()} скопировано!"
        elif copied_count == 0 and not structure_generated_and_included and not instructions_included_flag:
            msg = "Нет данных для копирования (файлы не выбраны, опции структуры/инструкций отключены или пусты)."
        else: 
             msg = "Данные скопированы (проверьте настройки и выбор файлов)."

        if log_widget_ref: log_widget_ref.insert(tk.END, msg + "\n", ('success',))
    except pyperclip.PyperclipException as e: 
        if log_widget_ref: log_widget_ref.insert(tk.END, f"Ошибка копирования pyperclip: {e}\nВозможно, буфер обмена недоступен или используется другой программой.\nПопробуйте установить xclip или xsel в Linux, если они отсутствуют.\n", ('error',))
    except Exception as e:
        if log_widget_ref: log_widget_ref.insert(tk.END, f"Ошибка копирования: {e}\nДАННЫЕ (первые 500 символов):\n{final_text[:500]}\n", ('error',)) 
    if log_widget_ref:
        try: log_widget_ref.see(tk.END)
        except: pass