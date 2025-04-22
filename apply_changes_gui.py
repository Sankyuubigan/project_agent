# apply_changes_gui.py

import os
import tkinter as tk
from tkinter import ttk
from tkinter import filedialog, scrolledtext
from pathlib import Path
import pyperclip
import json
import hashlib
import subprocess
import tempfile
import diff_match_patch as dmp_module
import re

def resource_path(relative_path):
    """ Возвращает абсолютный путь к ресурсу, работает для обычного запуска и для PyInstaller """
    try:
        # PyInstaller создает временную папку и сохраняет путь в _MEIPASS
        base_path = sys._MEIPASS
    except Exception:
        # _MEIPASS не установлен, значит запускаемся в обычном режиме
        base_path = os.path.abspath(Path(__file__).parent) # Используем pathlib и os.path.abspath

    return os.path.join(base_path, relative_path)


# --- Все функции (calculate_file_hash, apply_..., parse_..., process_input, etc.) ---
# --- остаются такими же, как в предыдущей версии.                             ---
# --- Я их снова скрою для краткости, но они должны быть здесь.                  ---
# ... (весь код функций до раздела "Создание GUI") ...

# Функция для вычисления хэша файла
def calculate_file_hash(file_path):
    hasher = hashlib.sha256()
    if not os.path.isfile(file_path):
        return "not_a_file"
    try:
        with open(file_path, 'rb') as f:
            for chunk in iter(lambda: f.read(4096), b""):
                hasher.update(chunk)
        return hasher.hexdigest()
    except Exception as e:
        print(f"Error calculating hash for {file_path}: {e}")
        return "error_calculating_hash"

# Функция для применения diff через git apply
def apply_diff_patch(project_dir, diff_content, log_widget):
    """Применяет diff-патч через git apply."""
    try:
        # Проверяем наличие Git
        # Добавляем флаг для скрытия окна консоли в Windows
        startupinfo = None
        if os.name == 'nt':
            startupinfo = subprocess.STARTUPINFO()
            startupinfo.dwFlags |= subprocess.STARTF_USESHOWWINDOW
            startupinfo.wShowWindow = subprocess.SW_HIDE # Скрываем окно

        git_check = subprocess.run(["git", "--version"], capture_output=True, text=True, check=False, startupinfo=startupinfo)
        if git_check.returncode != 0:
            log_widget.insert(tk.END, f"Ошибка: Git не установлен или не найден в PATH: {git_check.stderr}\n")
            return False
        log_widget.insert(tk.END, f"Git версия: {git_check.stdout.strip()}\n")

        with tempfile.NamedTemporaryFile(mode='w', suffix='.patch', delete=False, encoding='utf-8', newline='\n') as temp_file:
            diff_content_lf = diff_content.replace('\r\n', '\n')
            temp_file.write(diff_content_lf)
            temp_file_path = temp_file.name
        log_widget.insert(tk.END, f"Создан временный файл патча: {temp_file_path}\n")

        # Проверка diff
        log_widget.insert(tk.END, f"Проверка diff: git apply --check --ignore-space-change --ignore-whitespace {temp_file_path}\n")
        check_result = subprocess.run(
            ["git", "apply", "--check", "--ignore-space-change", "--ignore-whitespace", temp_file_path],
            cwd=project_dir, capture_output=True, text=True, encoding='utf-8', check=False, startupinfo=startupinfo
        )
        if check_result.returncode != 0:
            log_widget.insert(tk.END, f"Ошибка проверки diff:\n{check_result.stderr}\n")
            try: os.unlink(temp_file_path)
            except OSError as e: log_widget.insert(tk.END, f"Не удалось удалить временный файл {temp_file_path}: {e}\n")
            return False
        log_widget.insert(tk.END, f"Diff корректен, применяем...\n")

        # Применение diff
        log_widget.insert(tk.END, f"Команда: git apply --verbose --reject --ignore-space-change --ignore-whitespace {temp_file_path}\n")
        result = subprocess.run(
            ["git", "apply", "--verbose", "--reject", "--ignore-space-change", "--ignore-whitespace", temp_file_path],
            cwd=project_dir, capture_output=True, text=True, encoding='utf-8', check=False, startupinfo=startupinfo
        )

        try: os.unlink(temp_file_path)
        except OSError as e: log_widget.insert(tk.END, f"Не удалось удалить временный файл {temp_file_path}: {e}\n")

        if result.returncode == 0:
            log_widget.insert(tk.END, f"Патч успешно применён в директории: {project_dir}\n")
            if result.stdout: log_widget.insert(tk.END, f"Вывод git apply (stdout):\n{result.stdout}\n")
            if result.stderr: log_widget.insert(tk.END, f"Вывод git apply (stderr/warnings):\n{result.stderr}\n")
            rej_files = list(Path(project_dir).rglob('*.rej'))
            if rej_files:
                log_widget.insert(tk.END, "ПРЕДУПРЕЖДЕНИЕ: Обнаружены файлы .rej, некоторые части патча не были применены:\n")
                for rej_file in rej_files: log_widget.insert(tk.END, f" - {rej_file}\n")
            return True
        else:
            log_widget.insert(tk.END, f"Ошибка при применении патча:\n{result.stderr}\n")
            if result.stdout: log_widget.insert(tk.END, f"Вывод git apply (stdout):\n{result.stdout}\n")
            rej_files = list(Path(project_dir).rglob('*.rej'))
            if rej_files:
                log_widget.insert(tk.END, "Обнаружены файлы .rej с отклонёнными изменениями:\n")
                for rej_file in rej_files: log_widget.insert(tk.END, f" - {rej_file}\n")
            return False
    except FileNotFoundError:
        log_widget.insert(tk.END, "Ошибка: Команда 'git' не найдена. Убедитесь, что Git установлен и доступен в системном PATH.\n")
        return False
    except Exception as e:
        log_widget.insert(tk.END, f"Непредвиденная ошибка при выполнении git apply: {str(e)}\n")
        if 'temp_file_path' in locals() and os.path.exists(temp_file_path):
            try: os.unlink(temp_file_path)
            except OSError as unlink_e: log_widget.insert(tk.END, f"Не удалось удалить временный файл {temp_file_path} после ошибки: {unlink_e}\n")
        return False

# Новая функция для применения diff через diff-match-patch
def apply_diff_with_dmp(project_dir, diff_content, log_widget):
    """Применяет diff-патч с использованием diff-match-patch."""
    try:
        dmp = dmp_module.diff_match_patch()
        patches = dmp.patch_fromText(diff_content)
        if not patches:
            log_widget.insert(tk.END, "Ошибка: Не удалось разобрать diff-текст с помощью diff-match-patch.\n")
            log_widget.insert(tk.END, "Попытка ручного разбора git diff формата...\n")
            return apply_git_diff_manually_with_dmp(project_dir, diff_content, log_widget)

        log_widget.insert(tk.END, "Формат diff не распознан как стандартный DMP патч. Попытка ручного разбора git diff...\n")
        return apply_git_diff_manually_with_dmp(project_dir, diff_content, log_widget)

    except Exception as e:
        log_widget.insert(tk.END, f"Ошибка при применении diff через diff-match-patch: {str(e)}\n")
        return False

def apply_git_diff_manually_with_dmp(project_dir, diff_content, log_widget):
    """
    Пытается разобрать diff в формате `git diff` и применить его
    с помощью diff-match-patch к каждому файлу.
    """
    log_widget.insert(tk.END, "Начало ручного разбора и применения git diff с помощью DMP...\n")
    project_root = Path(project_dir).resolve()
    dmp = dmp_module.diff_match_patch()
    current_file_path = None
    diff_lines_for_file = []
    all_patches = {}

    lines = diff_content.splitlines()
    i = 0
    while i < len(lines):
        line = lines[i]
        if line.startswith("diff --git"):
            if current_file_path and diff_lines_for_file:
                 all_patches[current_file_path] = "\n".join(diff_lines_for_file) + "\n"

            diff_lines_for_file = [line]
            match = re.match(r'diff --git a/(.*?) b/(.*)', line)
            if match:
                path_a = match.group(1).strip('"')
                path_b = match.group(2).strip('"')
                current_file_path = Path(path_b).as_posix()
                log_widget.insert(tk.END, f"Найден diff для файла: {current_file_path}\n")
            else:
                log_widget.insert(tk.END, f"Предупреждение: Не удалось извлечь путь из строки diff --git: {line}\n")
                current_file_path = None
        elif current_file_path:
             diff_lines_for_file.append(line)
        i += 1

    if current_file_path and diff_lines_for_file:
        all_patches[current_file_path] = "\n".join(diff_lines_for_file) + "\n"

    if not all_patches:
        log_widget.insert(tk.END, "Ошибка: Не найдено ни одного блока diff --git в предоставленном тексте.\n")
        return False

    success = True
    for rel_path_str, patch_text in all_patches.items():
        full_path = project_root / rel_path_str
        log_widget.insert(tk.END, f"Обработка файла: {rel_path_str} ({full_path})\n")

        is_new_file = "new file mode" in patch_text and "index 0000000.." in patch_text
        is_deleted_file = "deleted file mode" in patch_text and "index " in patch_text and "..0000000" in patch_text

        if not full_path.exists() and not is_new_file:
             log_widget.insert(tk.END, f"Ошибка: Файл {full_path} не найден и diff не является созданием нового файла.\n")
             success = False; continue
        if full_path.exists() and is_new_file:
             log_widget.insert(tk.END, f"Предупреждение: Diff указывает на создание нового файла, но файл {full_path} уже существует. Попытка применить патч...\n")

        if is_deleted_file:
            if full_path.is_file():
                try: os.remove(full_path)
                except OSError as e: log_widget.insert(tk.END, f"Ошибка при удалении файла {rel_path_str}: {e}\n"); success = False
                else: log_widget.insert(tk.END, f"Файл {rel_path_str} удален согласно diff.\n")
            else: log_widget.insert(tk.END, f"Предупреждение: Diff указывает на удаление файла, но файл {full_path} не найден.\n")
            continue

        original_content = ""
        if is_new_file:
            log_widget.insert(tk.END, f"Файл {rel_path_str} создается как новый...\n")
            try: full_path.parent.mkdir(parents=True, exist_ok=True)
            except OSError as e: log_widget.insert(tk.END, f"Ошибка создания директории для {rel_path_str}: {e}\n"); success = False; continue
        else:
             try:
                 with open(full_path, 'r', encoding='utf-8') as f: original_content = f.read()
             except Exception as read_e: log_widget.insert(tk.END, f"Ошибка чтения файла {rel_path_str}: {read_e}\n"); success = False; continue

        try:
            patches = dmp.patch_fromText(patch_text)
            if not patches:
                 log_widget.insert(tk.END, f"Предупреждение: Не удалось создать DMP патч из текста для файла {rel_path_str}. Применение невозможно.\n")
                 success = False; continue

            new_content, results = dmp.patch_apply(patches, original_content)

            if all(results):
                log_widget.insert(tk.END, f"Патч успешно применён к файлу: {rel_path_str}\n")
                try:
                    with open(full_path, 'w', encoding='utf-8', newline='\n') as f: f.write(new_content)
                except Exception as write_e: log_widget.insert(tk.END, f"Ошибка записи файла {rel_path_str}: {write_e}\n"); success = False
            else:
                failed_hunks = [i for i, res in enumerate(results) if not res]
                log_widget.insert(tk.END, f"Ошибка: Не удалось применить патч к файлу {rel_path_str}. Ошибки в блоках (hunks): {failed_hunks}. Результаты: {results}\n")
                success = False

        except ValueError as ve: log_widget.insert(tk.END, f"Ошибка ValueError при обработке патча для {rel_path_str}: {ve}.\n"); success = False
        except Exception as e: log_widget.insert(tk.END, f"Непредвиденная ошибка при обработке файла {rel_path_str}: {e}\n"); success = False

    log_widget.insert(tk.END, f"Ручной разбор и применение git diff завершено. Общий результат: {'Успех' if success else 'Неудача'}\n")
    return success

def apply_json_patch(file_path_str, changes, log_widget):
    """Применяет изменения из JSON к указанному файлу."""
    file_path = Path(file_path_str) # Работаем с Path объектом
    try:
        expected_hash = changes.get("expected_hash")
        file_exists = file_path.is_file()

        if expected_hash and file_exists:
            current_hash = calculate_file_hash(file_path_str)
            if current_hash not in ["not_a_file", "error_calculating_hash"] and current_hash != expected_hash:
                log_widget.insert(tk.END, f"Ошибка: Хэш файла {file_path.name} не совпадает. Ожидаемый: {expected_hash}, текущий: {current_hash}\n")
                return False
        elif expected_hash and not file_exists:
             log_widget.insert(tk.END, f"Предупреждение: Указан expected_hash, но файл {file_path.name} не существует.\n")

        lines = []
        if file_exists:
            try:
                with open(file_path, 'r', encoding='utf-8') as f: lines = f.readlines()
            except Exception as read_e: log_widget.insert(tk.END, f"Ошибка чтения файла {file_path.name}: {read_e}\n"); return False
        elif changes.get("action") != "replace_all" and not changes.get("modifications"):
             log_widget.insert(tk.END, f"Ошибка: Файл {file_path.name} не существует и нет данных для его создания.\n"); return False

        modifications = changes.get("modifications", [])
        if changes.get("action") == "replace_all":
             lines = [(line + '\n') for line in changes.get("content", "").splitlines()]
             log_widget.insert(tk.END, f"Действие 'replace_all' для файла {file_path.name}.\n")
             modifications = []

        for change in modifications:
            action = change.get("action")
            line_number = change.get("line_number")
            content = change.get("content", "")
            count = change.get("count", 1)

            if action == "replace":
                if 1 <= line_number <= len(lines):
                    original_line_ending = '\r\n' if lines[line_number - 1].endswith('\r\n') else '\n'
                    new_line = content + (original_line_ending if not content.endswith(('\r\n', '\n')) else '')
                    lines[line_number - 1] = new_line
                else: log_widget.insert(tk.END, f"Ошибка 'replace': Номер строки {line_number} вне диапазона (1-{len(lines)}) в файле {file_path.name}\n"); return False
            elif action == "add":
                if 0 <= line_number <= len(lines):
                    new_line = content + ('\n' if not content.endswith(('\r\n', '\n')) else '')
                    lines.insert(line_number, new_line)
                else: log_widget.insert(tk.END, f"Ошибка 'add': Номер строки {line_number} вне диапазона (0-{len(lines)}) в файле {file_path.name}\n"); return False
            elif action == "remove":
                start_index = line_number - 1
                end_index = start_index + count
                if 0 <= start_index < len(lines) and start_index < end_index <= len(lines):
                    del lines[start_index:end_index]
                else:
                    range_str = f"[{line_number}-{line_number+count-1}]"; valid_range = f"[1-{len(lines)}]" if lines else "[пустой]"
                    log_widget.insert(tk.END, f"Ошибка 'remove': Диапазон {range_str} вне допустимого {valid_range} в файле {file_path.name}\n"); return False
            else: log_widget.insert(tk.END, f"Ошибка: Неизвестное действие '{action}' для файла {file_path.name}\n"); return False

        try:
            if not file_exists: file_path.parent.mkdir(parents=True, exist_ok=True)
            with open(file_path, 'w', encoding='utf-8', newline='') as f: f.writelines(lines)
            log_widget.insert(tk.END, f"Изменения JSON успешно применены к файлу: {file_path.name}\n")
            return True
        except Exception as write_e: log_widget.insert(tk.END, f"Ошибка записи файла {file_path.name}: {write_e}\n"); return False

    except Exception as e: log_widget.insert(tk.END, f"Непредвиденная ошибка при применении JSON к файлу {file_path.name}: {e}\n"); return False

# --- НОВЫЕ ФУНКЦИИ для Markdown ---
def parse_markdown_input(markdown_text, log_widget):
    """
    Парсит Markdown текст на блоки файлов, определенных <<<FILE: ...>>> и <<<END_FILE>>>.
    Возвращает словарь {относительный_путь: содержимое_файла}.
    """
    files = {}
    current_file_path = None
    content_lines = []
    in_code_block = False
    in_file_block = False
    file_start_re = re.compile(r'^\s*<<<FILE:\s*(.*?)\s*>>>\s*$')
    file_end_re = re.compile(r'^\s*<<<END_FILE>>>\s*$')
    code_block_start_re = re.compile(r'^\s*```(.*)$')
    code_block_end_re = re.compile(r'^\s*```\s*$')
    lines = markdown_text.splitlines()

    for i, line in enumerate(lines):
        line_stripped = line.strip()
        if file_end_re.match(line_stripped):
            if in_file_block and current_file_path:
                files[current_file_path] = "\n".join(content_lines)
                log_widget.insert(tk.END, f"Найден конец для файла: {current_file_path}\n")
            elif in_file_block: log_widget.insert(tk.END, f"Предупреждение: <<<END_FILE>>> без пути файла (строка {i+1})\n")
            else: log_widget.insert(tk.END, f"Предупреждение: <<<END_FILE>>> без <<<FILE: ...>>> (строка {i+1})\n")
            current_file_path = None; content_lines = []; in_code_block = False; in_file_block = False; continue

        match_start = file_start_re.match(line_stripped)
        if match_start:
            raw_path = match_start.group(1).strip()
            normalized_path = Path(raw_path).as_posix()
            if in_file_block and current_file_path:
                log_widget.insert(tk.END, f"Предупреждение: Новый <<<FILE: {normalized_path}>>> до <<<END_FILE>>> для '{current_file_path}' (строка {i+1})\n")
                files[current_file_path] = "\n".join(content_lines)
            if normalized_path:
                current_file_path = normalized_path
                log_widget.insert(tk.END, f"Начало обработки файла: {current_file_path}\n")
                content_lines = []; in_code_block = False; in_file_block = True
            else:
                log_widget.insert(tk.END, f"Ошибка: Пустой путь в <<<FILE: ...>>> (строка {i+1})\n")
                current_file_path = None; in_file_block = False; continue
            continue

        if in_file_block:
            match_code_start = code_block_start_re.match(line)
            if match_code_start and not in_code_block: in_code_block = True; continue
            if code_block_end_re.match(line) and in_code_block: in_code_block = False; continue
            if in_code_block: content_lines.append(line)

    if in_file_block and current_file_path:
         log_widget.insert(tk.END, f"Предупреждение: Текст закончился до <<<END_FILE>>> для '{current_file_path}'. Файл обработан.\n")
         files[current_file_path] = "\n".join(content_lines)
    if not files: log_widget.insert(tk.END, "Предупреждение: Не найдено валидных блоков <<<FILE: path>>>...<<<END_FILE>>>.\n")
    return files

def apply_markdown_changes(project_dir, file_data, log_widget):
    """
    Применяет изменения (перезаписывает файлы) на основе данных из parse_markdown_input.
    """
    project_path = Path(project_dir).resolve()
    success_count = 0; error_count = 0
    if not file_data: log_widget.insert(tk.END, "Нет данных Markdown для применения.\n"); return False

    for relative_path_str, content in file_data.items():
        try:
            relative_path = Path(relative_path_str)
            full_path = (project_path / relative_path).resolve()
            if os.path.commonpath([project_path, full_path]) != str(project_path):
                log_widget.insert(tk.END, f"ОШИБКА БЕЗОПАСНОСТИ: Попытка записи вне проекта: '{relative_path_str}'. Пропущено.\n")
                error_count += 1; continue
            full_path.parent.mkdir(parents=True, exist_ok=True)
            with open(full_path, 'w', encoding='utf-8', newline='\n') as f: f.write(content)
            log_widget.insert(tk.END, f"Файл успешно записан/перезаписан: {relative_path_str}\n")
            success_count += 1
        except OSError as e: log_widget.insert(tk.END, f"Ошибка OSError при записи {relative_path_str}: {e.strerror}\n"); error_count += 1
        except Exception as e: log_widget.insert(tk.END, f"Общая ошибка при записи {relative_path_str}: {e}\n"); error_count += 1

    log_widget.insert(tk.END, f"Применение Markdown завершено. Успешно: {success_count}, Ошибки: {error_count}\n")
    return error_count == 0

# --- Функция обработки основного ввода ---
def process_input(input_text, project_dir, log_widget, apply_method):
    """
    Обрабатывает ввод в зависимости от выбранного метода.
    """
    project_path = Path(project_dir)
    if not project_path.is_dir():
        log_widget.insert(tk.END, f"Критическая ошибка: Директория проекта не найдена: {project_dir}\n"); return
    log_widget.insert(tk.END, f"--- Начало обработки. Метод: {apply_method} ---\n")

    try:
        if apply_method == "Markdown":
            log_widget.insert(tk.END, "Обработка ввода как Markdown...\n")
            file_data = parse_markdown_input(input_text, log_widget)
            if file_data: apply_markdown_changes(project_dir, file_data, log_widget)
            else: log_widget.insert(tk.END, "Markdown парсер не нашел данных для применения.\n")
        elif apply_method == "Git":
            log_widget.insert(tk.END, "Обработка ввода как Diff (применение через Git)...\n")
            apply_diff_patch(project_dir, input_text, log_widget)
        elif apply_method == "Diff-Match-Patch":
            log_widget.insert(tk.END, "Обработка ввода как Diff (применение через Diff-Match-Patch)...\n")
            apply_diff_with_dmp(project_dir, input_text, log_widget)
        elif apply_method == "JSON":
            log_widget.insert(tk.END, "Обработка ввода как JSON...\n")
            try:
                changes_dict = json.loads(input_text)
                if "changes" not in changes_dict or not isinstance(changes_dict["changes"], list):
                    log_widget.insert(tk.END, "Ошибка: JSON должен содержать ключ 'changes' со списком объектов.\n"); return
                total_files = len(changes_dict["changes"]); applied_count = 0
                log_widget.insert(tk.END, f"Найдено изменений для {total_files} файлов в JSON.\n")
                for i, file_change in enumerate(changes_dict["changes"]):
                    if not isinstance(file_change, dict):
                        log_widget.insert(tk.END, f"Ошибка: Элемент #{i+1} в 'changes' не объект.\n"); continue
                    file_path_rel = file_change.get("file")
                    if not file_path_rel:
                        log_widget.insert(tk.END, f"Ошибка: Запись #{i+1} без ключа 'file'.\n"); continue
                    norm_rel_path = Path(file_path_rel).as_posix()
                    full_path = os.path.join(project_dir, norm_rel_path)
                    log_widget.insert(tk.END, f"---> Применение JSON к файлу: {norm_rel_path}\n")
                    if apply_json_patch(full_path, file_change, log_widget): applied_count += 1
                    log_widget.insert(tk.END, f"<--- Завершено для файла: {norm_rel_path}\n")
                log_widget.insert(tk.END, f"Обработка JSON завершена. Успешно: {applied_count} из {total_files}.\n")
            except json.JSONDecodeError as e: log_widget.insert(tk.END, f"Ошибка разбора JSON: {e}\n")
            except Exception as e: import traceback; log_widget.insert(tk.END, f"Ошибка обработки JSON: {e}\n{traceback.format_exc()}\n")
        else: log_widget.insert(tk.END, f"Ошибка: Неизвестный метод '{apply_method}'\n")
    except Exception as e: import traceback; log_widget.insert(tk.END, f"КРИТИЧЕСКАЯ ОШИБКА обработки: {e}\n{traceback.format_exc()}\n")
    finally: log_widget.insert(tk.END, f"--- Обработка завершена ---\n"); log_widget.see(tk.END)


# --- НОВЫЕ ФУНКЦИИ и КОНСТАНТЫ для Treeview ---
CHECKED_TAG = "checked"
FOLDER_TAG = "folder"
FILE_TAG = "file"
IGNORED_DIRS = {'.git', '__pycache__', '.vscode', '.idea', 'node_modules', 'venv', '.env', 'build', 'dist'} # Добавил build/dist
IGNORED_FILES = {'.DS_Store', '*.pyc', '*.spec', '*.log', '*.tmp', '*.bak'} # Добавил еще игноры

tree_item_paths = {} # Словарь для хранения полного пути {item_id: full_path}

def populate_file_tree(directory_path, tree):
    """Очищает и заполняет Treeview файлами и папками из указанной директории."""
    global tree_item_paths
    for i in tree.get_children(): tree.delete(i)
    tree_item_paths = {}

    if not directory_path or not os.path.isdir(directory_path):
        tree.insert("", tk.END, text="Выберите корректную директорию проекта", open=False, tags=('message',))
        return

    try:
        dir_name = os.path.basename(directory_path)
        root_node = tree.insert("", tk.END, text=dir_name, open=True, tags=(FOLDER_TAG,))
        tree_item_paths[root_node] = directory_path
        _populate_recursive(directory_path, root_node, tree)
        # По умолчанию выделяем корень при загрузке
        # toggle_item_check(tree, root_node, True) # Вызываем новую функцию
        set_check_state_recursive(tree, root_node, True) # Выделяем все по умолчанию
    except Exception as e:
        tree.insert("", tk.END, text=f"Ошибка доступа: {e}", open=False, tags=('error',))


def _populate_recursive(parent_path, parent_node, tree):
    """Рекурсивно добавляет элементы в Treeview."""
    global tree_item_paths
    from fnmatch import fnmatch # Импортируем для проверки масок файлов

    try:
        # Сортируем: сначала папки, потом файлы, все по алфавиту
        items = sorted(os.listdir(parent_path), key=lambda x: (os.path.isfile(os.path.join(parent_path, x)), x.lower()))
    except OSError as e:
        tree.insert(parent_node, tk.END, text=f"Ошибка чтения: {e.strerror}", open=False, tags=('error',))
        return

    for item_name in items:
        item_path = os.path.join(parent_path, item_name)

        is_dir = os.path.isdir(item_path)
        is_file = os.path.isfile(item_path)

        # Проверка на игнорирование
        if is_dir and item_name in IGNORED_DIRS: continue
        if is_file and item_name in IGNORED_FILES: continue
        # Проверка по маске
        is_ignored_by_pattern = False
        if is_file:
            for pattern in IGNORED_FILES:
                if '*' in pattern or '?' in pattern or '[' in pattern:
                    if fnmatch(item_name, pattern):
                        is_ignored_by_pattern = True
                        break
        if is_ignored_by_pattern: continue

        # Добавляем элемент
        node_id = None
        if is_dir:
            node_id = tree.insert(parent_node, tk.END, text=item_name, open=False, tags=(FOLDER_TAG,))
            tree_item_paths[node_id] = item_path
            _populate_recursive(item_path, node_id, tree) # Рекурсия для папок
        elif is_file:
            node_id = tree.insert(parent_node, tk.END, text=item_name, open=False, tags=(FILE_TAG,))
            tree_item_paths[node_id] = item_path


def toggle_check(event, tree):
    """Обрабатывает клик по элементу Treeview для переключения состояния 'checked'."""
    item_id = tree.identify_row(event.y)
    if not item_id: return
    # Не позволяем снять выделение с корневого элемента (если он один)
    # if not tree.parent(item_id) and len(tree.get_children()) == 1: return

    current_state = CHECKED_TAG in tree.item(item_id, 'tags')
    new_state = not current_state
    set_check_state_recursive(tree, item_id, new_state)
    _update_parent_check_state_recursive(tree, item_id) # Обновить состояние родительских


def set_check_state_recursive(tree, item_id, state):
    """Рекурсивно устанавливает состояние 'checked' для элемента и всех его потомков."""
    tags = set(tree.item(item_id, 'tags'))
    tag_present = CHECKED_TAG in tags

    if state: # Ставим галочку
        if not tag_present:
            tags.add(CHECKED_TAG)
            tree.item(item_id, tags=tuple(tags))
    else: # Снимаем галочку
        if tag_present:
            tags.discard(CHECKED_TAG)
            tree.item(item_id, tags=tuple(tags))

    # Рекурсия для дочерних элементов папки
    if FOLDER_TAG in tags:
        for child_id in tree.get_children(item_id):
            set_check_state_recursive(tree, child_id, state)


def _update_parent_check_state_recursive(tree, item_id):
    """Рекурсивно обновляет состояние родительских папок."""
    parent_id = tree.parent(item_id)
    if not parent_id: return # Дошли до корня

    children = tree.get_children(parent_id)
    if not children: return

    # Проверяем состояние детей
    all_children_checked = all(CHECKED_TAG in tree.item(child_id, 'tags') for child_id in children)
    # any_child_checked = any(CHECKED_TAG in tree.item(child_id, 'tags') for child_id in children) # Для "частичного" состояния

    parent_tags = set(tree.item(parent_id, 'tags'))
    parent_is_checked = CHECKED_TAG in parent_tags

    if all_children_checked:
        if not parent_is_checked:
            parent_tags.add(CHECKED_TAG)
            tree.item(parent_id, tags=tuple(parent_tags))
    else: # Хотя бы один ребенок не отмечен
        if parent_is_checked:
            parent_tags.discard(CHECKED_TAG)
            tree.item(parent_id, tags=tuple(parent_tags))

    # Рекурсивно идем вверх
    _update_parent_check_state_recursive(tree, parent_id)


def set_all_tree_check_state(tree, state):
    """Устанавливает или снимает отметку со всех корневых элементов дерева."""
    root_items = tree.get_children()
    for item_id in root_items:
        set_check_state_recursive(tree, item_id, state)


# --- Обновленные функции GUI ---

def select_project_dir(entry_widget, tree_widget, log_widget):
    """Выбирает директорию и обновляет Treeview."""
    dir_path = filedialog.askdirectory()
    if dir_path:
        entry_widget.delete(0, tk.END)
        entry_widget.insert(0, dir_path)
        log_widget.insert(tk.END, f"Выбрана директория проекта: {dir_path}\n")
        try:
            populate_file_tree(dir_path, tree_widget)
            log_widget.insert(tk.END, "Дерево файлов обновлено.\n")
        except Exception as e:
            log_widget.insert(tk.END, f"Ошибка при построении дерева файлов: {e}\n")
            populate_file_tree(None, tree_widget) # Показать сообщение об ошибке в дереве

def copy_project_files(project_dir_entry, tree_widget, log_widget):
    """Копирует выбранные в Treeview файлы (в Markdown-формате)
       и добавляет содержимое change_control_doc.md (из ресурсов сборки) в конец."""
    project_dir = project_dir_entry.get().strip()
    if not project_dir or not os.path.isdir(project_dir):
        log_widget.insert(tk.END, "Ошибка: Неверная или не выбранная директория проекта\n"); return

    base_path = Path(project_dir).resolve() # Для относительных путей из дерева
    result_blocks = []
    copied_files_count = 0
    global tree_item_paths

    all_tree_items = []
    def collect_all_items_recursive(parent=""):
        for item_id in tree_widget.get_children(parent):
            all_tree_items.append(item_id)
            collect_all_items_recursive(item_id)
    collect_all_items_recursive()

    log_widget.insert(tk.END, "Сбор выбранных файлов из дерева...\n")
    # Обработка выбранных файлов из дерева (код без изменений)
    for item_id in all_tree_items:
        tags = tree_widget.item(item_id, 'tags')
        if FILE_TAG in tags and CHECKED_TAG in tags:
            if item_id in tree_item_paths:
                full_path = Path(tree_item_paths[item_id])
                if full_path.is_file():
                    try:
                        relative_path = full_path.relative_to(base_path).as_posix()
                    except ValueError:
                         log_widget.insert(tk.END, f"Предупреждение: Файл '{full_path.name}' вне '{base_path}'. Пропущено.\n"); continue
                    try:
                        with open(full_path, 'r', encoding='utf-8') as f: content = f.read()
                        lang = full_path.suffix[1:] if full_path.suffix else ""
                        file_block = (f"<<<FILE: {relative_path}>>>\n```{lang}\n{content}\n```\n<<<END_FILE>>>")
                        result_blocks.append(file_block)
                        copied_files_count += 1
                    except UnicodeDecodeError: log_widget.insert(tk.END, f"Предупреждение: Файл '{relative_path}' не UTF-8. Пропущено.\n")
                    except Exception as e:
                        log_widget.insert(tk.END, f"Ошибка чтения {relative_path}: {e}\n")
                        result_blocks.append(f"<<<FILE: {relative_path}>>>\n```\n[error reading file: {e}]\n```\n<<<END_FILE>>>")
            else: log_widget.insert(tk.END, f"Предупреждение: Нет пути для ID {item_id}. Пропущено.\n")


    # --- ПОЛУЧЕНИЕ СОДЕРЖИМОГО change_control_doc.md ИЗ РЕСУРСОВ СБОРКИ ---
    control_doc_filename = "change_control_doc.md"
    control_doc_content = ""
    control_doc_error = None
    control_doc_path_in_bundle = None # Для логирования пути

    try:
        # Используем resource_path для получения пути к файлу внутри сборки или рядом со скриптом
        control_doc_path_in_bundle = resource_path(control_doc_filename)
        log_widget.insert(tk.END, f"Поиск и чтение {control_doc_filename} из ресурсов ('{control_doc_path_in_bundle}')...\n")

        if os.path.isfile(control_doc_path_in_bundle): # Проверяем путь, полученный resource_path
            try:
                with open(control_doc_path_in_bundle, 'r', encoding='utf-8') as f:
                    control_doc_content = f.read()
                log_widget.insert(tk.END, f"Содержимое '{control_doc_filename}' успешно прочитано из ресурсов.\n")
            except UnicodeDecodeError:
                control_doc_error = f"Предупреждение: Файл '{control_doc_filename}' в ресурсах не UTF-8. Содержимое не добавлено."
                log_widget.insert(tk.END, control_doc_error + "\n")
            except Exception as e:
                control_doc_error = f"Ошибка чтения файла {control_doc_filename} из ресурсов: {str(e)}. Содержимое не добавлено."
                log_widget.insert(tk.END, control_doc_error + "\n")
        else:
            control_doc_error = f"Файл '{control_doc_filename}' не найден в ресурсах по пути '{control_doc_path_in_bundle}'. Содержимое не добавлено."
            log_widget.insert(tk.END, control_doc_error + "\n")
    except Exception as e:
        # Ошибка при вызове resource_path или другая неожиданная проблема
        control_doc_error = f"Критическая ошибка при получении пути к ресурсу {control_doc_filename}: {e}"
        log_widget.insert(tk.END, control_doc_error + "\n")
    # --- КОНЕЦ ПОЛУЧЕНИЯ СОДЕРЖИМОГО ---


    # Собираем финальный текст (код без изменений)
    final_text_parts = []
    if result_blocks:
        final_text_parts.append("\n\n".join(result_blocks))
    if control_doc_content:
        if final_text_parts:
            final_text_parts.append("\n\n") # Добавляем разделитель
        final_text_parts.append(control_doc_content)
    final_text = "".join(final_text_parts)

    if not final_text:
        log_widget.insert(tk.END, "Нет данных для копирования.\n"); return

    # Копирование в буфер (код без изменений)
    try:
        pyperclip.copy(final_text)
        log_msg = f"Содержимое {copied_files_count} выбранных файлов"
        if control_doc_content:
            log_msg += f" и содержимое '{control_doc_filename}' (из ресурсов)"
        elif control_doc_error:
            log_msg += f" ({control_doc_error})" # Указываем на проблему с control_doc
        log_msg += " скопировано в буфер обмена!"
        log_widget.insert(tk.END, log_msg + "\n")
    except Exception as e:
        log_widget.insert(tk.END, f"Ошибка копирования в буфер обмена: {e}\n")
        log_widget.insert(tk.END, "--- СОДЕРЖИМОЕ ДЛЯ РУЧНОГО КОПИРОВАНИЯ ---\n")
        log_widget.insert(tk.END, final_text + "\n")
        log_widget.insert(tk.END, "--- КОНЕЦ СОДЕРЖИМОГО ---\n")

    log_widget.see(tk.END)


def apply_changes(input_text_widget, project_dir_entry, log_widget, apply_method_var):
    """Основная функция кнопки 'Apply Changes'."""
    input_text = input_text_widget.get("1.0", tk.END).strip()
    project_dir = project_dir_entry.get().strip()
    apply_method = apply_method_var.get()
    if not input_text: log_widget.insert(tk.END, "Ошибка: Поле ввода пустое.\n"); return
    if not project_dir or not os.path.isdir(project_dir): log_widget.insert(tk.END, "Ошибка: Неверная директория проекта.\n"); return
    log_widget.delete("1.0", tk.END); process_input(input_text, project_dir, log_widget, apply_method)

def clear_input_field(input_text_widget, log_widget):
    input_text_widget.delete("1.0", tk.END); log_widget.insert(tk.END, "Поле ввода очищено.\n")

def copy_logs(log_widget):
    log_text = log_widget.get("1.0", tk.END).strip()
    if log_text:
        try: pyperclip.copy(log_text)
        except Exception as e: log_widget.insert(tk.END, f"\nОшибка копирования логов: {e}\n")
        else: log_widget.insert(tk.END, "\nЛоги скопированы в буфер обмена!\n")
    else: log_widget.insert(tk.END, "\nЛоги пусты.\n")
    log_widget.see(tk.END)

def create_context_menu(widget):
    """Создает стандартное контекстное меню для текстовых полей."""
    menu = tk.Menu(widget, tearoff=0)
    is_text_widget = isinstance(widget, (tk.Text, scrolledtext.ScrolledText))
    is_entry_widget = isinstance(widget, tk.Entry)
    is_editable = widget.cget("state") != "disabled" if hasattr(widget, 'cget') else True # Проверка на редактируемость

    cmd_cut = lambda: widget.event_generate("<<Cut>>")
    cmd_copy = lambda: widget.event_generate("<<Copy>>")
    cmd_paste = lambda: widget.event_generate("<<Paste>>")
    cmd_select_all = lambda: widget.tag_add(tk.SEL, "1.0", tk.END) if is_text_widget else (widget.select_range(0, tk.END) if is_entry_widget else None)

    menu.add_command(label="Cut", command=cmd_cut, state="disabled")
    menu.add_command(label="Copy", command=cmd_copy, state="disabled")
    menu.add_command(label="Paste", command=cmd_paste, state="disabled") # Начнем с disabled
    menu.add_separator()
    menu.add_command(label="Select All", command=cmd_select_all, state="disabled")

    def update_menu_state():
        # Состояние Cut, Copy зависит от наличия выделения
        has_selection = False
        try:
            if widget.selection_get(): has_selection = True
        except (tk.TclError, AttributeError): has_selection = False
        cut_copy_state = "normal" if has_selection and is_editable else "disabled"
        copy_only_state = "normal" if has_selection else "disabled" # Copy доступно и для read-only

        # Состояние Paste зависит от содержимого буфера и редактируемости
        can_paste = False
        try:
            if pyperclip.paste(): can_paste = True
        except Exception: can_paste = False # Ошибка pyperclip или пустой буфер
        paste_state = "normal" if can_paste and is_editable else "disabled"

        # Состояние Select All зависит от типа виджета
        select_all_state = "normal" if (is_text_widget or is_entry_widget) and is_editable else "disabled"

        # Обновляем меню
        try: menu.entryconfigure("Cut", state="disabled" if not is_editable else cut_copy_state) # Cut только если редактируемый
        except tk.TclError: pass
        try: menu.entryconfigure("Copy", state=copy_only_state) # Copy всегда если есть выделение
        except tk.TclError: pass
        try: menu.entryconfigure("Paste", state=paste_state)
        except tk.TclError: pass
        try: menu.entryconfigure("Select All", state=select_all_state)
        except tk.TclError: pass


    def show_context_menu(event):
        update_menu_state() # Обновляем состояние перед показом
        menu.tk_popup(event.x_root, event.y_root)

    widget.bind("<Button-3>", show_context_menu)


# --- Создание GUI ---
root = tk.Tk()
root.title("Apply Project Changes")
root.geometry("950x750") # Еще немного увеличим

# --- САМЫЙ ВЕРХ: Выбор директории ---
top_dir_frame = tk.Frame(root)
top_dir_frame.pack(side=tk.TOP, fill=tk.X, pady=(5, 5), padx=10) # Уменьшим нижний отступ

tk.Label(top_dir_frame, text="Project Directory:").pack(side=tk.LEFT, padx=(0, 5))
project_dir_entry = tk.Entry(top_dir_frame, width=80) # Сделаем поле пошире
project_dir_entry.pack(side=tk.LEFT, fill=tk.X, expand=True)
# Кнопка Browse теперь вызывает функцию, которая также обновляет дерево (file_tree нужно определить до этого)
# Пока оставим lambda, но привяжем позже
browse_button = tk.Button(top_dir_frame, text="Browse...", command=lambda: select_project_dir(project_dir_entry, file_tree, log_widget))
browse_button.pack(side=tk.LEFT, padx=(5, 0))
create_context_menu(project_dir_entry) # Добавим меню и сюда


# --- Основная рама для разделения на левую/правую части ---
main_frame = tk.Frame(root)
# Уменьшаем верхний отступ, т.к. top_dir_frame уже дает отступ
main_frame.pack(pady=(0, 10), padx=10, fill=tk.BOTH, expand=True)

# --- Левая часть: Поле ввода и настройки ---
left_frame = tk.Frame(main_frame)
left_frame.pack(side=tk.LEFT, fill=tk.BOTH, expand=True, padx=(0, 10))

# Поле ввода
input_area_frame = tk.Frame(left_frame)
input_area_frame.pack(fill=tk.BOTH, expand=True)
tk.Label(input_area_frame, text="Input (Markdown, Diff, or JSON):").pack(anchor=tk.W)
input_text_widget = scrolledtext.ScrolledText(input_area_frame, height=15, width=50, wrap=tk.WORD)
input_text_widget.pack(fill=tk.BOTH, expand=True)
create_context_menu(input_text_widget)
input_text_widget.bind("<Control-a>", lambda event: input_text_widget.tag_add(tk.SEL, "1.0", tk.END))


# Настройки под полем ввода (БЕЗ выбора директории)
settings_frame = tk.Frame(left_frame)
settings_frame.pack(fill=tk.X, pady=(10, 0))

# Выбор метода и кнопки Apply/Clear (в одном ряду)
apply_method_frame = tk.Frame(settings_frame)
apply_method_frame.pack(fill=tk.X)

tk.Label(apply_method_frame, text="Apply Method:").pack(side=tk.LEFT, padx=(0, 5))
apply_method_var = tk.StringVar(value="Markdown")
apply_method_options = ["Markdown", "Git", "Diff-Match-Patch", "JSON"]
# Используем ttk.OptionMenu для лучшего вида
apply_method_menu = ttk.OptionMenu(apply_method_frame, apply_method_var, apply_method_var.get(), *apply_method_options)
apply_method_menu.pack(side=tk.LEFT, padx=(0,15)) # Увеличим отступ

apply_button = tk.Button(apply_method_frame, text="Apply Changes", command=lambda: apply_changes(input_text_widget, project_dir_entry, log_widget, apply_method_var), width=15, height=1, bg="#90EE90") # Сделаем зеленее
apply_button.pack(side=tk.LEFT, padx=5)

clear_button = tk.Button(apply_method_frame, text="Clear Input", command=lambda: clear_input_field(input_text_widget, log_widget))
clear_button.pack(side=tk.LEFT, padx=5)


# --- Правая часть: Дерево файлов и кнопка копирования ---
right_frame = tk.Frame(main_frame, width=400) # Можно сделать чуть шире
right_frame.pack(side=tk.RIGHT, fill=tk.BOTH, expand=False)
right_frame.pack_propagate(False)

tk.Label(right_frame, text="Select Files/Folders to Copy:").pack(anchor=tk.W, padx=5)

# Frame для Treeview и Scrollbar
tree_frame = tk.Frame(right_frame)
tree_frame.pack(fill=tk.BOTH, expand=True, pady=(0, 5), padx=5)

tree_scrollbar_y = ttk.Scrollbar(tree_frame, orient=tk.VERTICAL)
tree_scrollbar_x = ttk.Scrollbar(tree_frame, orient=tk.HORIZONTAL)
tree_scrollbar_y.pack(side=tk.RIGHT, fill=tk.Y)
tree_scrollbar_x.pack(side=tk.BOTTOM, fill=tk.X)

# Treeview
file_tree = ttk.Treeview(tree_frame, yscrollcommand=tree_scrollbar_y.set, xscrollcommand=tree_scrollbar_x.set, selectmode="none")
file_tree.pack(fill=tk.BOTH, expand=True)
tree_scrollbar_y.config(command=file_tree.yview)
tree_scrollbar_x.config(command=file_tree.xview)

# --- ИЗМЕНЕНИЕ ЦВЕТА ВЫДЕЛЕНИЯ ---
# Используем 'lightblue' или другой заметный цвет
# Можно использовать и foreground для изменения цвета текста
file_tree.tag_configure(CHECKED_TAG, background='lightblue') # , foreground='black'
file_tree.tag_configure('error', foreground='red') # Для ошибок в дереве
file_tree.tag_configure('message', foreground='grey') # Для сообщений в дереве

# Привязка клика для переключения чекбокса
file_tree.bind("<Button-1>", lambda event: toggle_check(event, file_tree))

# Кнопки управления выделением дерева
tree_buttons_frame = tk.Frame(right_frame)
tree_buttons_frame.pack(fill=tk.X, padx=5)
select_all_button = tk.Button(tree_buttons_frame, text="Select All", command=lambda: set_all_tree_check_state(file_tree, True))
select_all_button.pack(side=tk.LEFT, padx=(0,5))
deselect_all_button = tk.Button(tree_buttons_frame, text="Deselect All", command=lambda: set_all_tree_check_state(file_tree, False))
deselect_all_button.pack(side=tk.LEFT)

# Кнопка копирования под деревом
copy_button = tk.Button(right_frame, text="Copy Selected to Clipboard", command=lambda: copy_project_files(project_dir_entry, file_tree, log_widget), height=2)
copy_button.pack(fill=tk.X, pady=(10, 5), padx=5)


# --- Нижняя часть: Лог ---
log_frame = tk.Frame(root)
log_frame.pack(pady=(0, 10), padx=10, fill=tk.BOTH, expand=True) # Убрали верхний отступ
tk.Label(log_frame, text="Log:").pack(anchor=tk.W)
log_widget = scrolledtext.ScrolledText(log_frame, height=10, width=80, wrap=tk.WORD, state='normal') # state normal для записи
log_widget.pack(fill=tk.BOTH, expand=True)
create_context_menu(log_widget) # Применяем контекстное меню к логу
# Можно сделать лог read-only после добавления текста, но это усложнит код
# log_widget.configure(state='disabled') # Сделать read-only

copy_log_button = tk.Button(log_frame, text="Copy Logs", command=lambda: copy_logs(log_widget))
copy_log_button.pack(pady=(5,0), anchor=tk.E, padx=(0,5)) # Кнопка справа


# --- Загрузка/Сохранение конфигурации ---
config_file = Path("app_config.json")
# Загрузка последней директории
if config_file.exists():
    try:
        with open(config_file, 'r', encoding='utf-8') as f: config = json.load(f)
        last_dir = config.get("last_project_dir")
        if last_dir and os.path.isdir(last_dir):
            project_dir_entry.insert(0, last_dir)
            populate_file_tree(last_dir, file_tree) # Заполняем дерево при запуске
        else: populate_file_tree(None, file_tree)
    except (json.JSONDecodeError, OSError) as e:
        log_widget.insert(tk.END, f"Warning: Could not load config '{config_file}': {e}\n")
        populate_file_tree(None, file_tree)
    except Exception as e:
        log_widget.insert(tk.END, f"Unexpected error loading config: {e}\n")
        populate_file_tree(None, file_tree)
else: populate_file_tree(None, file_tree) # Если конфига нет

# Сохранение последней директории при выходе
def on_closing():
    last_dir = project_dir_entry.get()
    config = {"last_project_dir": last_dir if os.path.isdir(last_dir) else ""}
    try:
        with open(config_file, 'w', encoding='utf-8') as f: json.dump(config, f, indent=4)
    except Exception as e: print(f"Warning: Could not save config: {e}")
    root.destroy()

root.protocol("WM_DELETE_WINDOW", on_closing)
root.mainloop()