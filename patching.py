# patching.py
import os
import json
import subprocess
import tempfile
import re
import tkinter as tk # Импорт для log_widget.insert
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
            # Не считаем ошибкой, если это просто вывод stderr при успехе (например, git apply --verbose)
            if result.returncode != 0:
                 log_widget.insert(tk.END, f"!!! {step_name} ОШИБКА stderr:\n{result.stderr}\n")
            else:
                 log_widget.insert(tk.END, f"{step_name} stderr/warnings:\n{result.stderr}\n")

        return result
    except FileNotFoundError:
        log_widget.insert(tk.END, f"!!! ОШИБКА: Команда 'git' не найдена. Убедитесь, что Git установлен и в PATH.\n")
        return None
    except Exception as e:
        log_widget.insert(tk.END, f"!!! Непредвиденная ошибка при выполнении {step_name}: {e}\n")
        return None


def apply_diff_patch(project_dir, diff_content, log_widget):
    """Применяет diff-патч через git apply."""
    # 1. Проверка Git
    git_check_res = _run_git_command(["git", "--version"], project_dir, log_widget, "Проверка версии Git")
    if not git_check_res or git_check_res.returncode != 0:
        return False
    log_widget.insert(tk.END, f"Git версия: {git_check_res.stdout.strip()}\n")

    # 2. Создание временного файла
    temp_file_path = None
    try:
        with tempfile.NamedTemporaryFile(mode='w', suffix='.patch', delete=False, encoding='utf-8', newline='\n') as tf:
            tf.write(diff_content.replace('\r\n', '\n'))
            temp_file_path = tf.name
        log_widget.insert(tk.END, f"Создан временный патч: {temp_file_path}\n")

        # 3. Проверка патча
        cmd_check = ["git", "apply", "--check", "--ignore-space-change", "--ignore-whitespace", temp_file_path]
        check_res = _run_git_command(cmd_check, project_dir, log_widget, "Проверка diff")
        if not check_res or check_res.returncode != 0:
            log_widget.insert(tk.END, f"Патч не прошел проверку.\n")
            return False # Ошибка уже залогирована в _run_git_command
        log_widget.insert(tk.END, f"Diff корректен, применяем...\n")

        # 4. Применение патча
        cmd_apply = ["git", "apply", "--verbose", "--reject", "--ignore-space-change", "--ignore-whitespace", temp_file_path]
        apply_res = _run_git_command(cmd_apply, project_dir, log_widget, "Применение diff")

        if not apply_res: # Если была ошибка запуска команды
             return False
        if apply_res.returncode != 0:
            log_widget.insert(tk.END, f"Патч не применен (ошибка git apply).\n")
            # Проверяем rej файлы даже при ошибке
            if list(Path(project_dir).rglob('*.rej')):
                 log_widget.insert(tk.END, "Обнаружены .rej файлы с отклоненными изменениями.\n")
            return False
        else:
            log_widget.insert(tk.END, f"Патч успешно применен.\n")
            # Проверяем rej файлы при успехе (могут быть предупреждения)
            if list(Path(project_dir).rglob('*.rej')):
                 log_widget.insert(tk.END, "ПРЕДУПРЕЖДЕНИЕ: Обнаружены .rej файлы (некоторые части могли не примениться).\n")
            return True

    finally:
        # 5. Удаление временного файла
        if temp_file_path and os.path.exists(temp_file_path):
            try:
                os.unlink(temp_file_path)
                log_widget.insert(tk.END, f"Временный файл удален: {temp_file_path}\n")
            except OSError as e:
                log_widget.insert(tk.END, f"Не удалось удалить временный файл {temp_file_path}: {e}\n")


def apply_diff_with_dmp(project_dir, diff_content, log_widget):
    """Применяет diff-патч через diff-match-patch (вызывает ручной разбор)."""
    if not dmp_module:
        log_widget.insert(tk.END, "Ошибка: Библиотека diff-match-patch не найдена.\n"); return False
    log_widget.insert(tk.END, "DMP: Попытка ручного разбора git diff...\n")
    # В текущей реализации всегда вызываем ручной разбор
    return apply_git_diff_manually_with_dmp(project_dir, diff_content, log_widget)


def apply_git_diff_manually_with_dmp(project_dir, diff_content, log_widget):
    """Ручной разбор и применение git diff через diff-match-patch."""
    if not dmp_module:
        log_widget.insert(tk.END, "Ошибка: diff-match-patch не найдена.\n"); return False

    log_widget.insert(tk.END, "DMP: Ручной разбор git diff...\n")
    project_root = Path(project_dir).resolve()
    dmp = dmp_module.diff_match_patch()
    all_patches = {}
    current_file = None
    diff_lines = []

    # Разбор diff текста на патчи по файлам
    for line in diff_content.splitlines():
        if line.startswith("diff --git"):
            if current_file and diff_lines:
                all_patches[current_file] = "\n".join(diff_lines) + "\n"
            diff_lines = [line]
            match = re.match(r'diff --git a/(.*?) b/(.*)', line)
            if match:
                # Убираем кавычки если есть
                path_b = match.group(2).strip('"')
                current_file = Path(path_b).as_posix()
            else:
                log_widget.insert(tk.END, f"DMP Warn: не извлечен путь: {line}\n")
                current_file = None # Пропускаем этот блок diff
        elif current_file: # Собираем строки для текущего файла
            diff_lines.append(line)
    # Добавляем последний файл
    if current_file and diff_lines:
        all_patches[current_file] = "\n".join(diff_lines) + "\n"

    if not all_patches:
        log_widget.insert(tk.END, "DMP Ошибка: diff --git блоки не найдены.\n"); return False

    overall_success = True
    # Применение патчей к файлам
    for rel_path, patch_text in all_patches.items():
        full_path = project_root / rel_path
        log_widget.insert(tk.END, f"DMP Обработка: {rel_path}\n")
        is_new = "new file mode" in patch_text and "index 0000000.." in patch_text
        is_del = "deleted file mode" in patch_text and "..0000000" in patch_text

        # --- Обработка удаления ---
        if is_del:
            if full_path.is_file():
                try:
                    os.remove(full_path)
                    log_widget.insert(tk.END, f"DMP: {rel_path} удален.\n")
                except OSError as e:
                    log_widget.insert(tk.END, f"DMP Err: Ошибка удаления {rel_path}: {e}\n")
                    overall_success = False
            else:
                log_widget.insert(tk.END, f"DMP Warn: {rel_path} не найден для удаления.\n")
            continue # Переходим к следующему файлу

        # --- Чтение или подготовка к созданию ---
        original_content = ""
        if is_new:
            log_widget.insert(tk.END, f"DMP: Создание нового файла {rel_path}\n")
            try:
                full_path.parent.mkdir(parents=True, exist_ok=True)
            except OSError as e:
                log_widget.insert(tk.END, f"DMP Err: Ошибка mkdir для {rel_path}: {e}\n")
                overall_success = False
                continue # Не можем создать файл, пропускаем
        elif full_path.is_file():
            try:
                with open(full_path, 'r', encoding='utf-8') as f:
                    original_content = f.read()
            except Exception as e:
                log_widget.insert(tk.END, f"DMP Err: Ошибка чтения {rel_path}: {e}\n")
                overall_success = False
                continue # Не можем прочитать, пропускаем
        else: # Файл не существует и не помечен как новый
             log_widget.insert(tk.END, f"DMP Err: Файл {full_path} не найден и не новый.\n")
             overall_success = False
             continue

        # --- Применение патча DMP ---
        try:
            patches = dmp.patch_fromText(patch_text)
            if not patches:
                log_widget.insert(tk.END, f"DMP Warn: не создан DMP патч для {rel_path}.\n")
                overall_success = False
                continue

            new_content, results = dmp.patch_apply(patches, original_content)

            if all(results):
                log_widget.insert(tk.END, f"DMP: Патч применен к {rel_path}\n")
                try:
                    # Записываем результат
                    with open(full_path, 'w', encoding='utf-8', newline='\n') as f:
                        f.write(new_content)
                except Exception as e:
                    log_widget.insert(tk.END, f"DMP Err: Ошибка записи {rel_path}: {e}\n")
                    overall_success = False
            else:
                log_widget.insert(tk.END, f"DMP Err: Ошибка применения патча к {rel_path}. Результаты: {results}\n")
                overall_success = False
        except ValueError as e: # Конкретная ошибка DMP
            log_widget.insert(tk.END, f"DMP ValueError для {rel_path}: {e}.\n")
            overall_success = False
        except Exception as e: # Другие ошибки DMP
            log_widget.insert(tk.END, f"DMP Ошибка для {rel_path}: {e}\n")
            overall_success = False

    log_widget.insert(tk.END, f"DMP: Ручной разбор завершен. Результат: {'Успех' if overall_success else 'Неудача'}\n")
    return overall_success


def apply_json_patch(file_path_str, changes, log_widget):
    """Применяет изменения из JSON."""
    from file_processing import calculate_file_hash # Импортируем здесь, если нужно
    file_path = Path(file_path_str)
    log_prefix = f"JSON ({file_path.name}): "
    try:
        expected_hash = changes.get("expected_hash")
        file_exists = file_path.is_file()

        # Проверка хэша
        if expected_hash and file_exists:
            current_hash = calculate_file_hash(file_path_str)
            if current_hash not in ["not_a_file", "error_calculating_hash"] and current_hash != expected_hash:
                log_widget.insert(tk.END, f"{log_prefix}Ошибка хэша. Ожидаемый: {expected_hash}, текущий: {current_hash}\n")
                return False
        elif expected_hash and not file_exists:
             log_widget.insert(tk.END, f"{log_prefix}Warn: expected_hash, но файл не существует.\n")

        # Чтение или инициализация
        lines = []
        if file_exists:
            try:
                with open(file_path, 'r', encoding='utf-8') as f:
                    lines = f.readlines()
            except Exception as e:
                log_widget.insert(tk.END, f"{log_prefix}Ошибка чтения: {e}\n")
                return False
        elif changes.get("action") != "replace_all" and not changes.get("modifications"):
             log_widget.insert(tk.END, f"{log_prefix}Ошибка: Файл не существует, нет данных для создания.\n")
             return False

        # Применение модификаций
        modifications = changes.get("modifications", [])
        if changes.get("action") == "replace_all":
             lines = [(line + '\n') for line in changes.get("content", "").splitlines()]
             log_widget.insert(tk.END, f"{log_prefix}Действие 'replace_all'.\n")
             modifications = [] # Игнорируем остальные

        for change in modifications:
            action = change.get("action")
            line_num = change.get("line_number")
            content = change.get("content", "")
            count = change.get("count", 1)

            # Проверка line_num на тип и значение
            if not isinstance(line_num, int) or line_num < 0:
                 log_widget.insert(tk.END, f"{log_prefix}Ошибка: Неверный номер строки {line_num} для действия '{action}'.\n")
                 return False

            if action == "replace":
                idx = line_num - 1
                if 0 <= idx < len(lines):
                    ending = '\r\n' if lines[idx].endswith('\r\n') else '\n'
                    lines[idx] = content + (ending if not content.endswith(('\r\n', '\n')) else '')
                else:
                    log_widget.insert(tk.END, f"{log_prefix}Ошибка 'replace': строка {line_num} вне диапазона (1-{len(lines)}).\n")
                    return False
            elif action == "add":
                # line_num 0 означает вставку в начало
                if 0 <= line_num <= len(lines):
                     lines.insert(line_num, content + ('\n' if not content.endswith(('\r\n','\n')) else ''))
                else:
                     log_widget.insert(tk.END, f"{log_prefix}Ошибка 'add': строка {line_num} вне диапазона (0-{len(lines)}).\n")
                     return False
            elif action == "remove":
                start = line_num - 1
                end = start + count
                if 0 <= start < len(lines) and start < end <= len(lines):
                    del lines[start:end]
                else:
                    log_widget.insert(tk.END, f"{log_prefix}Ошибка 'remove': диапазон [{line_num}-{line_num+count-1}] вне (1-{len(lines)}).\n")
                    return False
            else:
                log_widget.insert(tk.END, f"{log_prefix}Ошибка: Неизвестное действие '{action}'.\n")
                return False

        # Запись результата
        try:
            if not file_exists:
                file_path.parent.mkdir(parents=True, exist_ok=True)
            with open(file_path, 'w', encoding='utf-8', newline='') as f:
                f.writelines(lines)
            log_widget.insert(tk.END, f"{log_prefix}Изменения применены.\n")
            return True
        except Exception as e:
            log_widget.insert(tk.END, f"{log_prefix}Ошибка записи: {e}\n")
            return False

    except Exception as e:
        # Ловим общие ошибки (например, при доступе к 'changes')
        log_widget.insert(tk.END, f"{log_prefix}Общая ошибка: {e}\n")
        return False


def parse_markdown_input(markdown_text, log_widget):
    """Парсит Markdown на блоки файлов."""
    files = {}; current_file = None; content = []; in_code = False; in_file = False
    re_file = re.compile(r'^\s*<<<FILE:\s*(.*?)\s*>>>\s*$'); re_end = re.compile(r'^\s*<<<END_FILE>>>\s*$')
    re_code = re.compile(r'^\s*```(.*)$'); re_code_end = re.compile(r'^\s*```\s*$')
    log_prefix = "MarkdownParse: "

    for i, line in enumerate(markdown_text.splitlines()):
        s_line = line.strip()
        # Конец файла
        if re_end.match(s_line):
            if in_file and current_file:
                files[current_file] = "\n".join(content)
                log_widget.insert(tk.END, f"{log_prefix}Конец файла: {current_file}\n")
            elif in_file: log_widget.insert(tk.END, f"{log_prefix}Warn: <<<END_FILE>>> без пути (строка {i+1})\n")
            else: log_widget.insert(tk.END, f"{log_prefix}Warn: <<<END_FILE>>> без <<<FILE>>> (строка {i+1})\n")
            current_file, content, in_code, in_file = None, [], False, False
            continue
        # Начало файла
        m_start = re_file.match(s_line)
        if m_start:
            path = Path(m_start.group(1).strip()).as_posix()
            if in_file and current_file:
                 log_widget.insert(tk.END, f"{log_prefix}Warn: Новый <<<FILE: {path}>>> до <<<END_FILE>>> для '{current_file}' (строка {i+1})\n")
                 files[current_file] = "\n".join(content) # Сохраняем предыдущий
            if path:
                current_file, content, in_code, in_file = path, [], False, True
                log_widget.insert(tk.END, f"{log_prefix}Начало файла: {current_file}\n")
            else:
                log_widget.insert(tk.END, f"{log_prefix}Err: Пустой путь в <<<FILE: ...>>> (строка {i+1})\n")
                current_file, in_file = None, False
            continue
        # Содержимое блока
        if in_file:
            if re_code.match(line) and not in_code: in_code = True; continue # Начало блока кода
            if re_code_end.match(line) and in_code: in_code = False; continue # Конец блока кода
            if in_code: content.append(line) # Добавляем строку из блока кода

    # Обработка незавершенного блока в конце
    if in_file and current_file:
        log_widget.insert(tk.END, f"{log_prefix}Warn: Текст закончился до <<<END_FILE>>> для '{current_file}'.\n")
        files[current_file] = "\n".join(content)
    if not files: log_widget.insert(tk.END, f"{log_prefix}Warn: Не найдено валидных блоков.\n")
    return files


def apply_markdown_changes(project_dir, file_data, log_widget):
    """Применяет изменения из словаря {путь: содержимое}."""
    project_path = Path(project_dir).resolve(); success = 0; errors = 0
    log_prefix = "MarkdownApply: "
    if not file_data:
        log_widget.insert(tk.END, f"{log_prefix}Нет данных.\n"); return False
    for rel_path, content in file_data.items():
        full_path = (project_path / Path(rel_path)).resolve()
        # Проверка безопасности
        try:
            if os.path.commonpath([project_path, full_path]) != str(project_path):
                log_widget.insert(tk.END, f"{log_prefix}БЕЗОПАСНОСТЬ: Запись вне проекта: '{rel_path}'.\n")
                errors += 1; continue
        except ValueError: # Если пути на разных дисках
             log_widget.insert(tk.END, f"{log_prefix}БЕЗОПАСНОСТЬ: Запись на другой диск: '{rel_path}'.\n")
             errors += 1; continue

        # Запись файла
        try:
            full_path.parent.mkdir(parents=True, exist_ok=True)
            with open(full_path, 'w', encoding='utf-8', newline='\n') as f:
                f.write(content)
            log_widget.insert(tk.END, f"{log_prefix}Файл записан: {rel_path}\n"); success += 1
        except OSError as e: # Ловим ошибки ОС
            log_widget.insert(tk.END, f"{log_prefix}OSError записи {rel_path}: {e.strerror}\n"); errors += 1
        except Exception as e: # Другие ошибки
            log_widget.insert(tk.END, f"{log_prefix}Ошибка записи {rel_path}: {e}\n"); errors += 1
    log_widget.insert(tk.END, f"{log_prefix}Завершено. Успешно: {success}, Ошибки: {errors}\n")
    return errors == 0


def process_input(input_text, project_dir, log_widget, apply_method):
    """Обрабатывает ввод в зависимости от выбранного метода."""
    if not Path(project_dir).is_dir():
        log_widget.insert(tk.END, f"Критическая ошибка: Папка проекта не найдена: {project_dir}\n")
        return
    log_widget.insert(tk.END, f"--- Начало обработки. Метод: {apply_method} ---\n")
    try:
        if apply_method == "Markdown":
            file_data = parse_markdown_input(input_text, log_widget)
            if file_data:
                apply_markdown_changes(project_dir, file_data, log_widget)
            else:
                log_widget.insert(tk.END, "Markdown: Нет данных для применения.\n")
        elif apply_method == "Git":
            apply_diff_patch(project_dir, input_text, log_widget)
        elif apply_method == "Diff-Match-Patch":
            apply_diff_with_dmp(project_dir, input_text, log_widget)
        elif apply_method == "JSON":
            try:
                changes_data = json.loads(input_text)
                if "changes" not in changes_data or not isinstance(changes_data["changes"], list):
                    log_widget.insert(tk.END, "JSON Err: 'changes' должен быть списком.\n"); return
                total = len(changes_data["changes"]); applied = 0
                log_widget.insert(tk.END, f"JSON: Найдено {total} файлов.\n")
                for i, file_change in enumerate(changes_data["changes"]):
                    if not isinstance(file_change, dict):
                        log_widget.insert(tk.END, f"JSON Err: Элемент #{i+1} не объект.\n"); continue
                    rel_path = file_change.get("file")
                    if not rel_path:
                        log_widget.insert(tk.END, f"JSON Err: Запись #{i+1} без 'file'.\n"); continue
                    # Используем os.path.join для совместимости, Path(rel_path).as_posix() для нормализации
                    full_path = os.path.join(project_dir, Path(rel_path).as_posix())
                    if apply_json_patch(full_path, file_change, log_widget):
                        applied += 1
                log_widget.insert(tk.END, f"JSON: Обработано. Успешно: {applied} из {total}.\n")
            except json.JSONDecodeError as e:
                log_widget.insert(tk.END, f"JSON Err: Ошибка разбора: {e}\n")
            except Exception as e:
                import traceback
                log_widget.insert(tk.END, f"JSON Err: Ошибка обработки: {e}\n{traceback.format_exc()}\n")
        else:
            log_widget.insert(tk.END, f"Ошибка: Неизвестный метод '{apply_method}'\n")
    except Exception as e: # Общий обработчик на всякий случай
        import traceback
        log_widget.insert(tk.END, f"КРИТИЧЕСКАЯ ОШИБКА: {e}\n{traceback.format_exc()}\n")
    finally:
        log_widget.insert(tk.END, f"--- Обработка завершена ---\n")
        # Безопасная прокрутка лога
        try:
            log_widget.see(tk.END)
        except: pass