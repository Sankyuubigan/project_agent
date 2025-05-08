# clipboard_logic.py
import os
import tkinter as tk
from pathlib import Path
import pyperclip

# Импортируем зависимости
from treeview_logic import (
    generate_project_structure_text, tree_item_data, tree_item_paths,
    CHECKED_TAG, FILE_TAG, BINARY_TAG, LARGE_FILE_TAG, ERROR_TAG, TOO_MANY_TOKENS_TAG,
    DISABLED_LOOK_TAGS
)
from file_processing import resource_path

def copy_project_files(project_dir_entry, tree_widget, log_widget_ref, include_structure, include_instructions):
    """Собирает структуру, выбранные файлы и control_doc, копирует в буфер."""
    project_dir_str = project_dir_entry.get().strip()
    if not project_dir_str or not os.path.isdir(project_dir_str):
        if log_widget_ref: log_widget_ref.insert(tk.END, "Ошибка: Неверная директория проекта\n"); return

    final_text_parts = []
    base_path = Path(project_dir_str).resolve()

    structure_generated_and_included = False
    # 1. Структура проекта
    if include_structure:
        structure_text = generate_project_structure_text(tree_widget, project_dir_str, log_widget_ref)
        if structure_text and not structure_text.startswith("Структура не сгенерирована"):
            final_text_parts.append(structure_text)
            final_text_parts.append("---\nСодержимое выбранных файлов:\n---\n\n")
            structure_generated_and_included = True
        elif log_widget_ref:
            log_widget_ref.insert(tk.END, "Информация: Структура проекта не будет включена (не сгенерирована или нет выбранных элементов).\n")
    elif log_widget_ref:
        log_widget_ref.insert(tk.END, "Информация: Структура проекта не будет включена (опция отключена).\n")


    # 2. Содержимое выбранных и активных файлов
    markdown_blocks = []
    copied_count = 0
    all_ids = []
    def _collect_ids(parent=""):
        for child in tree_widget.get_children(parent): all_ids.append(child); _collect_ids(child)
    _collect_ids()

    for item_id in all_ids:
        if item_id not in tree_item_data or not tree_item_data[item_id].get('is_file'): continue

        tags = set(tree_widget.item(item_id, 'tags'))
        if CHECKED_TAG in tags and not DISABLED_LOOK_TAGS.intersection(tags):
            path_str = tree_item_paths.get(item_id)
            if not path_str: continue
            path_obj = Path(path_str)
            rel_path = tree_item_data[item_id].get('rel_path', path_obj.name)
            try:
                with open(path_obj, 'r', encoding='utf-8') as f: content = f.read()
                lang = path_obj.suffix[1:].lower() if path_obj.suffix else ""
                markdown_blocks.append(f"<<<FILE: {rel_path}>>>\n```{lang}\n{content}\n```\n<<<END_FILE>>>")
                copied_count += 1
            except UnicodeDecodeError: markdown_blocks.append(f"<<<FILE: {rel_path}>>>\n```\n[binary file or read error]\n```\n<<<END_FILE>>>")
            except Exception as e: markdown_blocks.append(f"<<<FILE: {rel_path}>>>\n```\n[error reading file: {e}]\n```\n<<<END_FILE>>>")

    if markdown_blocks:
        final_text_parts.append("\n\n".join(markdown_blocks))

    # 3. change_control_doc.md
    doc_name = "change_control_doc.md"; doc_content = ""; doc_error = None
    instructions_included = False
    if include_instructions:
        try:
            doc_path = resource_path(doc_name)
            if os.path.isfile(doc_path):
                with open(doc_path, 'r', encoding='utf-8') as f: doc_content = f.read()
            # else: doc_error = f"'{doc_name}' не найден."
        except Exception as e: doc_error = f"Ошибка чтения '{doc_name}': {e}"

        if doc_content:
            if final_text_parts and (markdown_blocks or structure_generated_and_included):
                 final_text_parts.append("\n\n---\nДополнительная информация:\n---\n\n")
            elif not final_text_parts : # Если это первая часть текста (структура и файлы не добавлены)
                pass # Не нужен разделитель
            final_text_parts.append(doc_content)
            instructions_included = True
        elif log_widget_ref:
            if doc_error: log_widget_ref.insert(tk.END, f"Информация: Инструкции ('{doc_name}') не включены. {doc_error}\n")
            else: log_widget_ref.insert(tk.END, f"Информация: Инструкции ('{doc_name}') не включены (файл не найден или пуст).\n")
    elif log_widget_ref:
        log_widget_ref.insert(tk.END, f"Информация: Инструкции ('{doc_name}') не будут включены (опция отключена).\n")


    # Финальная сборка и копирование
    final_text = "".join(final_text_parts).strip()
    if not final_text:
        if log_widget_ref: log_widget_ref.insert(tk.END, "Нет данных для копирования.\n"); return
    try:
        pyperclip.copy(final_text)
        msg_parts = []
        if structure_generated_and_included: msg_parts.append("структура")
        if copied_count > 0: msg_parts.append(f"{copied_count} файл(ов)")
        if instructions_included: msg_parts.append(f"инструкции ('{doc_name}')")
        
        if msg_parts:
            msg = f"{', '.join(msg_parts).capitalize()} скопировано!"
        elif copied_count == 0 and not structure_generated_and_included and not instructions_included :
            msg = "Нет данных для копирования (файлы не выбраны, опции структуры/инструкций отключены или пусты)."
        else: # Если msg_parts пуст, но что-то могло быть скопировано (например, пустой файл или только пустая структура)
             msg = "Данные скопированы (проверьте настройки и выбор файлов)."


        if log_widget_ref: log_widget_ref.insert(tk.END, msg + "\n")
    except pyperclip.PyperclipException as e: # Более конкретное исключение для pyperclip
        if log_widget_ref: log_widget_ref.insert(tk.END, f"Ошибка копирования pyperclip: {e}\nВозможно, буфер обмена недоступен или используется другой программой.\nПопробуйте установить xclip или xsel в Linux, если они отсутствуют.\n")
    except Exception as e:
        if log_widget_ref: log_widget_ref.insert(tk.END, f"Ошибка копирования: {e}\nДАННЫЕ:\n{final_text}\n")
    if log_widget_ref:
        try: log_widget_ref.see(tk.END)
        except: pass
