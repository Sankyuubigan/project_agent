# patching.py
import os
import json
import subprocess
import tempfile
import re
import shutil
import tkinter as tk 
from pathlib import Path

# This will crash if the module is not installed.
import diff_match_patch as dmp_module

def _run_git_command(command, cwd, log_widget, step_name=""):
    """Helper function to run git and log the output."""
    if log_widget.winfo_exists():
        log_widget.insert(tk.END, f"Выполнение {step_name}: {' '.join(command)}\n", ('info',))
    
    startupinfo = None
    if os.name == 'nt':
        startupinfo = subprocess.STARTUPINFO()
        startupinfo.dwFlags |= subprocess.STARTF_USESHOWWINDOW
        startupinfo.wShowWindow = subprocess.SW_HIDE
    
    # This will crash if git is not found.
    result = subprocess.run(command, cwd=cwd, capture_output=True, text=True, encoding='utf-8', check=False, startupinfo=startupinfo)
    
    if log_widget.winfo_exists():
        if result.stdout:
            log_widget.insert(tk.END, f"{step_name} stdout:\n{result.stdout}\n", ('info',))
        if result.stderr:
            tag = 'error' if result.returncode != 0 else 'warning'
            log_widget.insert(tk.END, f"!!! {step_name} stderr:\n{result.stderr}\n", (tag,))
    return result

def apply_diff_patch(project_dir, diff_content, log_widget):
    """Applies a diff patch using git apply."""
    if not shutil.which("git"):
        if log_widget.winfo_exists():
            log_widget.insert(tk.END, "!!! ОШИБКА: Команда 'git' не найдена. Убедитесь, что Git установлен и в PATH.\n", ('error',))
        return False

    git_check_res = _run_git_command(["git", "--version"], project_dir, log_widget, "Проверка версии Git")
    if git_check_res.returncode != 0:
        return False
    if log_widget.winfo_exists():
        log_widget.insert(tk.END, f"Git версия: {git_check_res.stdout.strip()}\n", ('info',))

    # This will crash on permission errors.
    fd, temp_file_path = tempfile.mkstemp(suffix='.patch', text=True)
    with os.fdopen(fd, 'w', encoding='utf-8', newline='\n') as tf:
        tf.write(diff_content.replace('\r\n', '\n'))
    if log_widget.winfo_exists():
        log_widget.insert(tk.END, f"Создан временный патч: {temp_file_path}\n", ('info',))

    cmd_check = ["git", "apply", "--check", "--ignore-space-change", "--ignore-whitespace", temp_file_path]
    check_res = _run_git_command(cmd_check, project_dir, log_widget, "Проверка diff")
    
    success = False
    if check_res.returncode == 0:
        if log_widget.winfo_exists():
            log_widget.insert(tk.END, f"Diff корректен, применяем...\n", ('info',))
        cmd_apply = ["git", "apply", "--verbose", "--reject", "--ignore-space-change", "--ignore-whitespace", temp_file_path]
        apply_res = _run_git_command(cmd_apply, project_dir, log_widget, "Применение diff")
        if apply_res.returncode == 0:
            if log_widget.winfo_exists():
                log_widget.insert(tk.END, f"Патч успешно применен.\n", ('success',))
            success = True
        else:
            if log_widget.winfo_exists():
                log_widget.insert(tk.END, f"Патч не применен (ошибка git apply).\n", ('error',))
    else:
        if log_widget.winfo_exists():
            log_widget.insert(tk.END, f"Патч не прошел проверку.\n", ('error',))

    if os.path.exists(temp_file_path):
        os.unlink(temp_file_path)
    return success

def apply_diff_with_dmp(project_dir, diff_content, log_widget):
    """Applies a diff patch using diff-match-patch (manual parsing)."""
    if log_widget.winfo_exists():
        log_widget.insert(tk.END, "DMP: Попытка ручного разбора git diff...\n", ('info',))
    return apply_git_diff_manually_with_dmp(project_dir, diff_content, log_widget)

def apply_git_diff_manually_with_dmp(project_dir, diff_content, log_widget):
    """Manually parses and applies a git diff using diff-match-patch."""
    project_root = Path(project_dir).resolve()
    dmp = dmp_module.diff_match_patch()
    all_patches = {}; current_file = None; diff_lines = []

    for line in diff_content.splitlines():
        if line.startswith("diff --git"):
            if current_file and diff_lines: all_patches[current_file] = "\n".join(diff_lines) + "\n"
            diff_lines = [line]
            match = re.match(r'diff --git a/(.*?) b/(.*)', line)
            if match: current_file = Path(match.group(2).strip('"')).as_posix()
            else: current_file = None
        elif current_file: diff_lines.append(line)
    if current_file and diff_lines: all_patches[current_file] = "\n".join(diff_lines) + "\n"

    if not all_patches:
        if log_widget.winfo_exists(): log_widget.insert(tk.END, "DMP Ошибка: diff --git блоки не найдены.\n", ('error',)); 
        return False

    overall_success = True
    for rel_path, patch_text in all_patches.items():
        full_path = project_root / rel_path
        if log_widget.winfo_exists(): log_widget.insert(tk.END, f"DMP Обработка: {rel_path}\n", ('info',))
        is_new = "new file mode" in patch_text
        is_del = "deleted file mode" in patch_text

        if is_del:
            if full_path.is_file():
                os.remove(full_path)
                if log_widget.winfo_exists(): log_widget.insert(tk.END, f"DMP: {rel_path} удален.\n", ('success',))
            continue

        original_content = ""
        if is_new:
            if log_widget.winfo_exists(): log_widget.insert(tk.END, f"DMP: Создание нового файла {rel_path}\n", ('info',))
            full_path.parent.mkdir(parents=True, exist_ok=True)
        elif full_path.is_file():
            with open(full_path, 'r', encoding='utf-8') as f: original_content = f.read()
        else:
            if log_widget.winfo_exists(): log_widget.insert(tk.END, f"DMP Err: Файл {full_path} не найден и не новый.\n", ('error',)); 
            overall_success = False; continue

        # This block can crash on various dmp errors.
        patches = dmp.patch_fromText(patch_text)
        if patches:
            new_content, results = dmp.patch_apply(patches, original_content)
            if all(results):
                with open(full_path, 'w', encoding='utf-8', newline='\n') as f: f.write(new_content)
            else:
                overall_success = False
        else:
            overall_success = False
            
    return overall_success

def apply_markdown_changes(project_dir, file_data, log_widget):
    """Applies changes from a dictionary {path: content}."""
    project_path = Path(project_dir).resolve()
    success = 0; errors = 0
    log_prefix = "MarkdownApply: "
    if not file_data:
        if log_widget.winfo_exists(): log_widget.insert(tk.END, f"{log_prefix}Нет данных для применения.\n", ('info',))
        return False
    
    for rel_path, content in file_data.items():
        full_path = (project_path / Path(rel_path)).resolve()
        if not str(full_path).startswith(str(project_path) + os.sep) and full_path != project_path:
            if log_widget.winfo_exists(): log_widget.insert(tk.END, f"{log_prefix}БЕЗОПАСНОСТЬ: Запись вне проекта: '{rel_path}'. Пропущено.\n", ('error',))
            errors += 1; continue

        file_existed = full_path.is_file()
        full_path.parent.mkdir(parents=True, exist_ok=True)
        # This will crash on permission errors.
        with open(full_path, 'w', encoding='utf-8', newline='\n') as f:
            f.write(content)
        
        if log_widget.winfo_exists():
            log_msg = f"Файл изменен: {rel_path}\n" if file_existed else f"Файл создан: {rel_path}\n"
            log_widget.insert(tk.END, log_prefix + log_msg, ('success',))
        success += 1
            
    if log_widget.winfo_exists():
        log_widget.insert(tk.END, f"{log_prefix}Завершено. Успешно: {success}, Ошибки: {errors}\n", ('info' if errors == 0 else 'warning',))
    return errors == 0

def parse_markdown_input(markdown_text, log_widget):
    """Parses Markdown for file blocks."""
    files = {}
    current_file_path_str = None
    current_file_content_lines = []
    in_file_capture_mode = False
    
    re_file_marker = re.compile(r'^\s*<<<FILE:\s*(.*?)\s*>>>\s*$')
    re_end_file_marker = re.compile(r'^\s*<<<END_FILE>>>\s*$')
    re_code_block_start = re.compile(r'^\s*```(.*)$')
    re_code_block_end = re.compile(r'^\s*```\s*$')

    for line in markdown_text.splitlines():
        if not in_file_capture_mode:
            file_marker_match = re_file_marker.match(line)
            if file_marker_match:
                path_from_marker = file_marker_match.group(1).strip()
                if path_from_marker:
                    current_file_path_str = Path(path_from_marker).as_posix()
                    current_file_content_lines = []
                    in_file_capture_mode = True
        else:
            if re_end_file_marker.match(line):
                content_to_process = list(current_file_content_lines)
                if content_to_process:
                    first_line = content_to_process[0]
                    last_line = content_to_process[-1]
                    if re_code_block_start.match(first_line) and re_code_block_end.match(last_line):
                        content_to_process = content_to_process[1:-1]
                
                files[current_file_path_str] = "\n".join(content_to_process)
                current_file_path_str = None
                in_file_capture_mode = False
            else:
                current_file_content_lines.append(line)
    return files

def process_input(input_text, project_dir, log_widget, apply_method):
    """Processes the input based on the selected method."""
    if not Path(project_dir).is_dir():
        if log_widget.winfo_exists():
            log_widget.insert(tk.END, f"Критическая ошибка: Папка проекта не найдена: {project_dir}\n", ('error',))
        return
    if log_widget.winfo_exists():
        log_widget.insert(tk.END, f"--- Начало обработки. Метод: {apply_method} ---\n", ('info',))

    if apply_method == "Markdown":
        file_data = parse_markdown_input(input_text, log_widget)
        if file_data: apply_markdown_changes(project_dir, file_data, log_widget)
    elif apply_method == "Git":
        apply_diff_patch(project_dir, input_text, log_widget)
    elif apply_method == "Diff-Match-Patch":
        apply_diff_with_dmp(project_dir, input_text, log_widget)
    elif apply_method == "JSON": 
        # This will crash if JSON is malformed.
        parsed_json_data = json.loads(input_text)
        changes_list = parsed_json_data.get("changes")
        if isinstance(changes_list, list):
            # This function is not implemented in the provided code, so it will crash.
            # Assuming it's a placeholder for future logic.
            apply_precise_block_patch(project_dir, changes_list, log_widget)
    else:
        if log_widget.winfo_exists():
            log_widget.insert(tk.END, f"Ошибка: Неизвестный метод '{apply_method}'\n", ('error',))
    
    if log_widget.winfo_exists():
        log_widget.insert(tk.END, f"--- Обработка завершена ---\n", ('info',))
        log_widget.see(tk.END)