# patching.py
import os
import json
import subprocess
import tempfile
import re
import tkinter as tk 
from pathlib import Path
try:
    import diff_match_patch as dmp_module
except ImportError:
    dmp_module = None

# --- Функции применения патчей ---

def _run_git_command(command, cwd, log_widget, step_name=""):
    """Вспомогательная функция для запуска git и логирования."""
    log_widget.insert(tk.END, f"Выполнение {step_name}: {' '.join(command)}\n")
    startupinfo = None
    if os.name == 'nt':
        startupinfo = subprocess.STARTUPINFO()
        startupinfo.dwFlags |= subprocess.STARTF_USESHOWWINDOW
        startupinfo.wShowWindow = subprocess.SW_HIDE
    try:
        result = subprocess.run(command, cwd=cwd, capture_output=True, text=True, encoding='utf-8', check=False, startupinfo=startupinfo)
        if result.stdout:
            log_widget.insert(tk.END, f"{step_name} stdout:\n{result.stdout}\n")
        if result.stderr:
            if result.returncode != 0:
                 log_widget.insert(tk.END, f"!!! {step_name} ОШИБКА stderr:\n{result.stderr}\n", ('error',))
            else:
                 log_widget.insert(tk.END, f"{step_name} stderr/warnings:\n{result.stderr}\n", ('warning',))

        return result
    except FileNotFoundError:
        log_widget.insert(tk.END, f"!!! ОШИБКА: Команда 'git' не найдена. Убедитесь, что Git установлен и в PATH.\n", ('error',))
        return None
    except Exception as e:
        log_widget.insert(tk.END, f"!!! Непредвиденная ошибка при выполнении {step_name}: {e}\n", ('error',))
        return None


def apply_diff_patch(project_dir, diff_content, log_widget):
    """Применяет diff-патч через git apply."""
    git_check_res = _run_git_command(["git", "--version"], project_dir, log_widget, "Проверка версии Git")
    if not git_check_res or git_check_res.returncode != 0:
        return False
    log_widget.insert(tk.END, f"Git версия: {git_check_res.stdout.strip()}\n")

    temp_file_path = None
    try:
        with tempfile.NamedTemporaryFile(mode='w', suffix='.patch', delete=False, encoding='utf-8', newline='\n') as tf:
            tf.write(diff_content.replace('\r\n', '\n'))
            temp_file_path = tf.name
        log_widget.insert(tk.END, f"Создан временный патч: {temp_file_path}\n")

        cmd_check = ["git", "apply", "--check", "--ignore-space-change", "--ignore-whitespace", temp_file_path]
        check_res = _run_git_command(cmd_check, project_dir, log_widget, "Проверка diff")
        if not check_res or check_res.returncode != 0:
            log_widget.insert(tk.END, f"Патч не прошел проверку.\n", ('error',))
            return False
        log_widget.insert(tk.END, f"Diff корректен, применяем...\n", ('info',))

        cmd_apply = ["git", "apply", "--verbose", "--reject", "--ignore-space-change", "--ignore-whitespace", temp_file_path]
        apply_res = _run_git_command(cmd_apply, project_dir, log_widget, "Применение diff")

        if not apply_res: return False
        if apply_res.returncode != 0:
            log_widget.insert(tk.END, f"Патч не применен (ошибка git apply).\n", ('error',))
            if list(Path(project_dir).rglob('*.rej')):
                 log_widget.insert(tk.END, "Обнаружены .rej файлы с отклоненными изменениями.\n", ('warning',))
            return False
        else:
            log_widget.insert(tk.END, f"Патч успешно применен.\n", ('success',))
            if list(Path(project_dir).rglob('*.rej')):
                 log_widget.insert(tk.END, "ПРЕДУПРЕЖДЕНИЕ: Обнаружены .rej файлы (некоторые части могли не примениться).\n", ('warning',))
            return True
    finally:
        if temp_file_path and os.path.exists(temp_file_path):
            try:
                os.unlink(temp_file_path)
                log_widget.insert(tk.END, f"Временный файл удален: {temp_file_path}\n")
            except OSError as e:
                log_widget.insert(tk.END, f"Не удалось удалить временный файл {temp_file_path}: {e}\n", ('warning',))


def apply_diff_with_dmp(project_dir, diff_content, log_widget):
    """Применяет diff-патч через diff-match-patch (вызывает ручной разбор)."""
    if not dmp_module:
        log_widget.insert(tk.END, "Ошибка: Библиотека diff-match-patch не найдена.\n", ('error',)); return False
    log_widget.insert(tk.END, "DMP: Попытка ручного разбора git diff...\n", ('info',))
    return apply_git_diff_manually_with_dmp(project_dir, diff_content, log_widget)


def apply_git_diff_manually_with_dmp(project_dir, diff_content, log_widget):
    """Ручной разбор и применение git diff через diff-match-patch."""
    if not dmp_module:
        log_widget.insert(tk.END, "Ошибка: diff-match-patch не найдена.\n", ('error',)); return False

    log_widget.insert(tk.END, "DMP: Ручной разбор git diff...\n", ('info',))
    project_root = Path(project_dir).resolve()
    dmp = dmp_module.diff_match_patch()
    all_patches = {}; current_file = None; diff_lines = []

    for line in diff_content.splitlines():
        if line.startswith("diff --git"):
            if current_file and diff_lines: all_patches[current_file] = "\n".join(diff_lines) + "\n"
            diff_lines = [line]
            match = re.match(r'diff --git a/(.*?) b/(.*)', line)
            if match: current_file = Path(match.group(2).strip('"')).as_posix()
            else: log_widget.insert(tk.END, f"DMP Warn: не извлечен путь: {line}\n", ('warning',)); current_file = None
        elif current_file: diff_lines.append(line)
    if current_file and diff_lines: all_patches[current_file] = "\n".join(diff_lines) + "\n"

    if not all_patches:
        log_widget.insert(tk.END, "DMP Ошибка: diff --git блоки не найдены.\n", ('error',)); return False

    overall_success = True
    for rel_path, patch_text in all_patches.items():
        full_path = project_root / rel_path
        log_widget.insert(tk.END, f"DMP Обработка: {rel_path}\n")
        is_new = "new file mode" in patch_text and "index 0000000.." in patch_text
        is_del = "deleted file mode" in patch_text and "..0000000" in patch_text

        if is_del:
            if full_path.is_file():
                try: os.remove(full_path); log_widget.insert(tk.END, f"DMP: {rel_path} удален.\n", ('success',))
                except OSError as e: log_widget.insert(tk.END, f"DMP Err: Ошибка удаления {rel_path}: {e}\n", ('error',)); overall_success = False
            else: log_widget.insert(tk.END, f"DMP Warn: {rel_path} не найден для удаления.\n", ('warning',))
            continue

        original_content = ""
        if is_new:
            log_widget.insert(tk.END, f"DMP: Создание нового файла {rel_path}\n", ('info',))
            try: full_path.parent.mkdir(parents=True, exist_ok=True)
            except OSError as e: log_widget.insert(tk.END, f"DMP Err: Ошибка mkdir для {rel_path}: {e}\n", ('error',)); overall_success = False; continue
        elif full_path.is_file():
            try:
                with open(full_path, 'r', encoding='utf-8') as f: original_content = f.read()
            except Exception as e: log_widget.insert(tk.END, f"DMP Err: Ошибка чтения {rel_path}: {e}\n", ('error',)); overall_success = False; continue
        else: log_widget.insert(tk.END, f"DMP Err: Файл {full_path} не найден и не новый.\n", ('error',)); overall_success = False; continue

        try:
            patches = dmp.patch_fromText(patch_text)
            if not patches: log_widget.insert(tk.END, f"DMP Warn: не создан DMP патч для {rel_path}.\n", ('warning',)); overall_success = False; continue
            new_content, results = dmp.patch_apply(patches, original_content)
            if all(results):
                log_widget.insert(tk.END, f"DMP: Патч применен к {rel_path}\n", ('success',))
                with open(full_path, 'w', encoding='utf-8', newline='\n') as f: f.write(new_content)
            else: log_widget.insert(tk.END, f"DMP Err: Ошибка применения патча к {rel_path}. Результаты: {results}\n", ('error',)); overall_success = False
        except ValueError as e: log_widget.insert(tk.END, f"DMP ValueError для {rel_path}: {e}.\n", ('error',)); overall_success = False
        except Exception as e: log_widget.insert(tk.END, f"DMP Ошибка для {rel_path}: {e}\n", ('error',)); overall_success = False
    log_widget.insert(tk.END, f"DMP: Ручной разбор завершен. Результат: {'Успех' if overall_success else 'Неудача'}\n")
    return overall_success

def apply_precise_block_patch(project_dir_str, changes_list, log_widget):
    project_root = Path(project_dir_str).resolve()
    applied_count = 0; failed_count = 0
    log_prefix_base = "PreciseBlockApply"

    if not isinstance(changes_list, list):
        log_widget.insert(tk.END, f"{log_prefix_base}: Ошибка: 'changes' должен быть списком.\n", ('error',))
        return 0, 1 

    for i, change_instruction in enumerate(changes_list):
        log_prefix = f"{log_prefix_base}({i+1}/{len(changes_list)}): "
        if not isinstance(change_instruction, dict):
            log_widget.insert(tk.END, f"{log_prefix}Ошибка: Инструкция #{i+1} не является словарем.\n", ('error',)); failed_count += 1; continue

        file_path_relative_str = change_instruction.get("filePath")
        operation = change_instruction.get("operation")

        if not file_path_relative_str or not operation:
            log_widget.insert(tk.END, f"{log_prefix}Ошибка: Отсутствует 'filePath' или 'operation' в инструкции #{i+1}.\n", ('error',)); failed_count += 1; continue
        
        log_widget.insert(tk.END, f"{log_prefix}Файл: {file_path_relative_str}, Операция: {operation}\n")
        try:
            target_file_path = (project_root / file_path_relative_str).resolve()
            if not str(target_file_path).startswith(str(project_root) + os.sep) and target_file_path != project_root:
                log_widget.insert(tk.END, f"{log_prefix}ОШИБКА БЕЗОПАСНОСТИ: Попытка доступа к файлу вне проекта: {target_file_path}\n", ('error',)); failed_count += 1; continue
            
            if operation == "CREATE_FILE":
                content_lines = change_instruction.get("content", [])
                if not isinstance(content_lines, list):
                     log_widget.insert(tk.END, f"{log_prefix}Ошибка: 'content' для CREATE_FILE должен быть списком строк.\n", ('error',)); failed_count += 1; continue
                content_to_write = "\n".join(content_lines)
                if target_file_path.exists():
                    log_widget.insert(tk.END, f"{log_prefix}Предупреждение: Файл {target_file_path} уже существует. CREATE_FILE пропущен.\n", ('warning',)); failed_count += 1; continue
                target_file_path.parent.mkdir(parents=True, exist_ok=True)
                with open(target_file_path, "w", encoding="utf-8", newline="\n") as f: f.write(content_to_write)
                log_widget.insert(tk.END, f"{log_prefix}Успех: Файл {target_file_path} создан.\n", ('success',)); applied_count += 1; continue
            elif operation == "DELETE_FILE":
                if not target_file_path.is_file():
                    log_widget.insert(tk.END, f"{log_prefix}Предупреждение/Ошибка: Файл {target_file_path} не найден для удаления или это не файл.\n", ('warning',)); failed_count += 1; continue
                os.remove(target_file_path)
                log_widget.insert(tk.END, f"{log_prefix}Успех: Файл {target_file_path} удален.\n", ('success',)); applied_count += 1; continue

            if not target_file_path.is_file():
                log_widget.insert(tk.END, f"{log_prefix}Ошибка: Файл {target_file_path} не найден для операции '{operation}'.\n", ('error',)); failed_count += 1; continue
            current_file_content = ""
            with open(target_file_path, "r", encoding="utf-8") as f: current_file_content = f.read()
            new_file_content = None
            if operation == "REPLACE_BLOCK":
                original_block_lines = change_instruction.get("original_block_content", [])
                modified_block_lines = change_instruction.get("modified_block_content", [])
                if not isinstance(original_block_lines, list) or not isinstance(modified_block_lines, list):
                    log_widget.insert(tk.END, f"{log_prefix}Ошибка: 'original_block_content' и 'modified_block_content' должны быть списками.\n", ('error',)); failed_count += 1; continue
                if not original_block_lines:
                    log_widget.insert(tk.END, f"{log_prefix}Ошибка: 'original_block_content' не может быть пустым для REPLACE_BLOCK.\n", ('error',)); failed_count += 1; continue
                original_text = "\n".join(original_block_lines); modified_text = "\n".join(modified_block_lines)
                occurrences = current_file_content.count(original_text)
                if occurrences == 1: new_file_content = current_file_content.replace(original_text, modified_text, 1)
                elif occurrences == 0: log_widget.insert(tk.END, f"{log_prefix}Ошибка: Блок для замены (original_block_content) НЕ НАЙДЕН.\n--- Ожидаемый блок: ---\n{original_text}\n---\n", ('error',)); failed_count += 1
                else: log_widget.insert(tk.END, f"{log_prefix}Ошибка: Блок для замены (original_block_content) НАЙДЕН {occurrences} РАЗ. Неоднозначность.\n--- Искомый блок: ---\n{original_text}\n---\n", ('error',)); failed_count += 1
            elif operation == "DELETE_BLOCK":
                original_block_lines = change_instruction.get("original_block_content", [])
                if not isinstance(original_block_lines, list) or not original_block_lines:
                    log_widget.insert(tk.END, f"{log_prefix}Ошибка: 'original_block_content' должен быть непустым списком для DELETE_BLOCK.\n", ('error',)); failed_count += 1; continue
                original_text = "\n".join(original_block_lines)
                occurrences = current_file_content.count(original_text)
                if occurrences == 1: new_file_content = current_file_content.replace(original_text, "", 1)
                elif occurrences == 0: log_widget.insert(tk.END, f"{log_prefix}Ошибка: Блок для удаления (original_block_content) НЕ НАЙДЕН.\n--- Ожидаемый блок: ---\n{original_text}\n---\n", ('error',)); failed_count += 1
                else: log_widget.insert(tk.END, f"{log_prefix}Ошибка: Блок для удаления (original_block_content) НАЙДЕН {occurrences} РАЗ. Неоднозначность.\n--- Искомый блок: ---\n{original_text}\n---\n", ('error',)); failed_count += 1
            elif operation in ["ADD_BLOCK_AFTER_ANCHOR", "ADD_BLOCK_BEFORE_ANCHOR"]:
                anchor_block_lines = change_instruction.get("anchor_block_content", [])
                block_to_add_lines = change_instruction.get("block_to_add_content", [])
                if not isinstance(anchor_block_lines, list) or not anchor_block_lines or not isinstance(block_to_add_lines, list):
                    log_widget.insert(tk.END, f"{log_prefix}Ошибка: некорректные 'anchor_block_content' или 'block_to_add_content'.\n", ('error',)); failed_count += 1; continue
                anchor_text = "\n".join(anchor_block_lines); text_to_add = "\n".join(block_to_add_lines)
                occurrences = current_file_content.count(anchor_text)
                if occurrences == 1:
                    replacement = (anchor_text + "\n" + text_to_add) if operation == "ADD_BLOCK_AFTER_ANCHOR" else (text_to_add + "\n" + anchor_text)
                    new_file_content = current_file_content.replace(anchor_text, replacement, 1)
                elif occurrences == 0: log_widget.insert(tk.END, f"{log_prefix}Ошибка: Якорный блок (anchor_block_content) НЕ НАЙДЕН.\n--- Ожидаемый якорный блок: ---\n{anchor_text}\n---\n", ('error',)); failed_count += 1
                else: log_widget.insert(tk.END, f"{log_prefix}Ошибка: Якорный блок (anchor_block_content) НАЙДЕН {occurrences} РАЗ. Неоднозначность.\n--- Искомый якорный блок: ---\n{anchor_text}\n---\n", ('error',)); failed_count += 1
            else: log_widget.insert(tk.END, f"{log_prefix}Ошибка: Неизвестная операция '{operation}' для существующего файла.\n", ('error',)); failed_count += 1
            if new_file_content is not None:
                with open(target_file_path, "w", encoding="utf-8", newline="\n") as f: f.write(new_file_content)
                log_widget.insert(tk.END, f"{log_prefix}Успех: Файл {target_file_path} изменен ({operation}).\n", ('success',)); applied_count += 1
        except Exception as e:
            import traceback
            log_widget.insert(tk.END, f"{log_prefix}КРИТИЧЕСКАЯ ОШИБКА при обработке инструкции #{i+1} для файла {file_path_relative_str}: {e}\n{traceback.format_exc()}\n", ('error',)); failed_count += 1
    log_widget.insert(tk.END, f"{log_prefix_base}: Завершено. Применено: {applied_count}, Ошибок/Пропущено: {failed_count}\n", ('info' if failed_count == 0 else 'warning',))
    return applied_count, failed_count

def apply_json_patch(file_path_str, changes, log_widget):
    """Применяет изменения из JSON (СТАРЫЙ МЕТОД, построчный)."""
    from file_processing import calculate_file_hash 
    file_path = Path(file_path_str)
    log_prefix = f"JSON_Legacy ({file_path.name}): "
    try:
        expected_hash = changes.get("expected_hash")
        file_exists = file_path.is_file()
        if expected_hash and file_exists:
            current_hash = calculate_file_hash(file_path_str)
            if current_hash not in ["not_a_file", "error_calculating_hash"] and current_hash != expected_hash:
                log_widget.insert(tk.END, f"{log_prefix}Ошибка хэша. Ожидаемый: {expected_hash}, текущий: {current_hash}\n", ('error',)); return False
        elif expected_hash and not file_exists: log_widget.insert(tk.END, f"{log_prefix}Warn: expected_hash, но файл не существует.\n", ('warning',))
        lines = []
        if file_exists:
            try:
                with open(file_path, 'r', encoding='utf-8') as f: lines = f.readlines()
            except Exception as e: log_widget.insert(tk.END, f"{log_prefix}Ошибка чтения: {e}\n", ('error',)); return False
        elif changes.get("action") != "replace_all" and not changes.get("modifications"):
             log_widget.insert(tk.END, f"{log_prefix}Ошибка: Файл не существует, нет данных для создания.\n", ('error',)); return False
        modifications = changes.get("modifications", [])
        if changes.get("action") == "replace_all":
             lines = [(line + '\n') for line in changes.get("content", "").splitlines()]; log_widget.insert(tk.END, f"{log_prefix}Действие 'replace_all'.\n"); modifications = []
        for change in modifications:
            action = change.get("action"); line_num = change.get("line_number"); content = change.get("content", ""); count = change.get("count", 1)
            if not isinstance(line_num, int) or line_num < 0: log_widget.insert(tk.END, f"{log_prefix}Ошибка: Неверный номер строки {line_num} для '{action}'.\n", ('error',)); return False
            if action == "replace":
                idx = line_num - 1
                if 0 <= idx < len(lines): lines[idx] = content + ('\r\n' if lines[idx].endswith('\r\n') else '\n' if not content.endswith(('\r\n', '\n')) else '')
                else: log_widget.insert(tk.END, f"{log_prefix}Ошибка 'replace': строка {line_num} вне диапазона (1-{len(lines)}).\n", ('error',)); return False
            elif action == "add":
                if 0 <= line_num <= len(lines): lines.insert(line_num, content + ('\n' if not content.endswith(('\r\n','\n')) else ''))
                else: log_widget.insert(tk.END, f"{log_prefix}Ошибка 'add': строка {line_num} вне диапазона (0-{len(lines)}).\n", ('error',)); return False
            elif action == "remove":
                start = line_num - 1; end = start + count
                if 0 <= start < len(lines) and start < end <= len(lines): del lines[start:end]
                else: log_widget.insert(tk.END, f"{log_prefix}Ошибка 'remove': диапазон [{line_num}-{line_num+count-1}] вне (1-{len(lines)}).\n", ('error',)); return False
            else: log_widget.insert(tk.END, f"{log_prefix}Ошибка: Неизвестное действие '{action}'.\n", ('error',)); return False
        try:
            if not file_exists: file_path.parent.mkdir(parents=True, exist_ok=True)
            with open(file_path, 'w', encoding='utf-8', newline='') as f: f.writelines(lines)
            log_widget.insert(tk.END, f"{log_prefix}Изменения применены.\n", ('success',)); return True
        except Exception as e: log_widget.insert(tk.END, f"{log_prefix}Ошибка записи: {e}\n", ('error',)); return False
    except Exception as e: log_widget.insert(tk.END, f"{log_prefix}Общая ошибка: {e}\n", ('error',)); return False

def parse_markdown_input(markdown_text, log_widget):
    """Парсит Markdown на блоки файлов. Файл добавляется только если найден его <<<END_FILE>>>."""
    files = {}
    current_file_path_str = None
    current_file_content_lines = []
    in_code_block = False 
    re_file_marker = re.compile(r'^\s*<<<FILE:\s*(.*?)\s*>>>\s*$')
    re_end_file_marker = re.compile(r'^\s*<<<END_FILE>>>\s*$')
    re_code_block_start_or_end = re.compile(r'^\s*```(.*)$') 
    log_prefix = "MarkdownParse: "

    for i, line in enumerate(markdown_text.splitlines()):
        stripped_line = line.strip()
        file_marker_match = re_file_marker.match(stripped_line)
        if file_marker_match:
            if current_file_path_str:
                log_widget.insert(tk.END, f"{log_prefix}Предупреждение: Новый маркер <<<FILE:...>>> найден до <<<END_FILE>>> для предыдущего файла '{current_file_path_str}'. Предыдущий файл НЕ будет обработан.\n", ('warning',))
            path_from_marker = file_marker_match.group(1).strip()
            if path_from_marker:
                current_file_path_str = Path(path_from_marker).as_posix()
                current_file_content_lines = []
                in_code_block = False 
                log_widget.insert(tk.END, f"{log_prefix}Обнаружен файл: {current_file_path_str}\n")
            else:
                log_widget.insert(tk.END, f"{log_prefix}Ошибка: Пустой путь в маркере <<<FILE:...>>> на строке {i+1}.\n", ('error',))
                current_file_path_str = None 
            continue
        if re_end_file_marker.match(stripped_line):
            if current_file_path_str:
                files[current_file_path_str] = "\n".join(current_file_content_lines)
                log_widget.insert(tk.END, f"{log_prefix}Файл '{current_file_path_str}' успешно завершен и добавлен для обработки.\n", ('success',))
                current_file_path_str = None 
                current_file_content_lines = []
                in_code_block = False
            else:
                log_widget.insert(tk.END, f"{log_prefix}Предупреждение: Маркер <<<END_FILE>>> найден без активного маркера <<<FILE:...>>> на строке {i+1}.\n", ('warning',))
            continue
        if current_file_path_str:
            code_block_match = re_code_block_start_or_end.match(line) 
            if code_block_match:
                in_code_block = not in_code_block 
                continue 
            if in_code_block:
                current_file_content_lines.append(line)
    if current_file_path_str:
        log_widget.insert(tk.END, f"{log_prefix}Предупреждение: Входной текст закончился, но для файла '{current_file_path_str}' не найден маркер <<<END_FILE>>>. Файл НЕ будет обработан.\n", ('warning',))
    if not files:
        log_widget.insert(tk.END, f"{log_prefix}Не найдено корректно завершенных файлов для обработки (<<<FILE:...>>> ... <<<END_FILE>>>).\n", ('warning',))
    return files

def apply_markdown_changes(project_dir, file_data, log_widget):
    """Применяет изменения из словаря {путь: содержимое}."""
    project_path = Path(project_dir).resolve(); success = 0; errors = 0
    log_prefix = "MarkdownApply: "
    if not file_data: log_widget.insert(tk.END, f"{log_prefix}Нет данных для применения.\n", ('info',)); return False # Изменено на info
    for rel_path, content in file_data.items():
        full_path = (project_path / Path(rel_path)).resolve()
        try: 
            if not str(full_path).startswith(str(project_path) + os.sep) and full_path != project_path:
                log_widget.insert(tk.END, f"{log_prefix}БЕЗОПАСНОСТЬ: Запись вне проекта: '{rel_path}'. Пропущено.\n", ('error',)); errors += 1; continue
        except ValueError: log_widget.insert(tk.END, f"{log_prefix}БЕЗОПАСНОСТЬ: Запись на другой диск: '{rel_path}'. Пропущено.\n", ('error',)); errors += 1; continue
        try: 
            full_path.parent.mkdir(parents=True, exist_ok=True)
            with open(full_path, 'w', encoding='utf-8', newline='\n') as f: f.write(content) 
            log_widget.insert(tk.END, f"{log_prefix}Файл записан: {rel_path}\n", ('success',)); success += 1
        except OSError as e: log_widget.insert(tk.END, f"{log_prefix}OSError записи {rel_path}: {e.strerror}\n", ('error',)); errors += 1
        except Exception as e: log_widget.insert(tk.END, f"{log_prefix}Ошибка записи {rel_path}: {e}\n", ('error',)); errors += 1
    log_widget.insert(tk.END, f"{log_prefix}Завершено. Успешно: {success}, Ошибки: {errors}\n", ('info' if errors == 0 else 'warning',))
    return errors == 0

def process_input(input_text, project_dir, log_widget, apply_method):
    """Обрабатывает ввод в зависимости от выбранного метода."""
    if not Path(project_dir).is_dir():
        log_widget.insert(tk.END, f"Критическая ошибка: Папка проекта не найдена: {project_dir}\n", ('error',))
        return
    log_widget.insert(tk.END, f"--- Начало обработки. Метод: {apply_method} ---\n", ('info',))
    
    try: # Настройка тегов, если еще не было
        log_widget.tag_config('error', foreground='red'); log_widget.tag_config('warning', foreground='orange') 
        log_widget.tag_config('success', foreground='green'); log_widget.tag_config('info', foreground='blue')
    except tk.TclError: pass

    try:
        if apply_method == "Markdown":
            file_data = parse_markdown_input(input_text, log_widget)
            if file_data: apply_markdown_changes(project_dir, file_data, log_widget)
            # else: log_widget.insert(tk.END, "Markdown: Нет данных для применения (уже залогировано в parse_markdown_input).\n", ('info',)) # parse_markdown_input уже логирует
        elif apply_method == "Git":
            apply_diff_patch(project_dir, input_text, log_widget)
        elif apply_method == "Diff-Match-Patch":
            apply_diff_with_dmp(project_dir, input_text, log_widget)
        elif apply_method == "JSON": 
            try:
                parsed_json_data = json.loads(input_text)
                changes_list = parsed_json_data.get("changes")
                if changes_list is None: log_widget.insert(tk.END, "JSON PreciseBlock: Ошибка: отсутствует ключ 'changes' в JSON или он null.\n", ('error',)); return
                if not isinstance(changes_list, list): log_widget.insert(tk.END, "JSON PreciseBlock: Ошибка: 'changes' должен быть списком.\n", ('error',)); return
                apply_precise_block_patch(project_dir, changes_list, log_widget)
            except json.JSONDecodeError as e: log_widget.insert(tk.END, f"JSON PreciseBlock: Ошибка разбора JSON: {e}\n", ('error',))
            except Exception as e: import traceback; log_widget.insert(tk.END, f"JSON PreciseBlock: Непредвиденная ошибка обработки: {e}\n{traceback.format_exc()}\n", ('error',))
        else:
            log_widget.insert(tk.END, f"Ошибка: Неизвестный метод '{apply_method}'\n", ('error',))
    except Exception as e: 
        import traceback
        log_widget.insert(tk.END, f"КРИТИЧЕСКАЯ ОШИБКА в process_input: {e}\n{traceback.format_exc()}\n", ('error',))
    finally:
        log_widget.insert(tk.END, f"--- Обработка завершена ---\n", ('info',))
        try: log_widget.see(tk.END)
        except: pass