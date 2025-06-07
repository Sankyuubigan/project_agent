# core/fs_scanner_utils.py
import os
from pathlib import Path
import fnmatch as fnmatch_lib
import tkinter as tk # Для type hinting и tk.END, если log_widget используется

# Импортируем необходимые компоненты из file_processing
from core.file_processing import ( # Используем абсолютный импорт от корня пакета
    count_file_tokens, BINARY_EXTENSIONS, MAX_FILE_SIZE_BYTES, MAX_TOKENS_FOR_DISPLAY
)

BINARY_STATUS_TAG = "status_binary"
LARGE_FILE_STATUS_TAG = "status_large_file"
ERROR_STATUS_TAG = "status_error"
EXCLUDED_BY_DEFAULT_STATUS_TAG = "status_excluded_default"
TOO_MANY_TOKENS_STATUS_TAG = "status_too_many_tokens"

GLOBAL_IGNORED_DIRS = {
    '.git', '__pycache__', '.vscode', '.idea', 'node_modules', 'venv', '.env',
    'build', 'dist', 'out', 'target', '.pytest_cache', '.mypy_cache', '.tox',
}

GLOBAL_IGNORED_FILES = {
    '.DS_Store', 'Thumbs.db', 'desktop.ini', '*.pyc', '*.pyo', '*.pyd',
    '*.so', '*.dll', '*.log', '*.tmp', '*.bak', '*.swp', '*.swo', '.coverage'
} 

EXCLUDED_BY_DEFAULT_PATTERNS = { 
    'poetry.lock', 'pnpm-lock.yaml', 'yarn.lock', 'package-lock.json',
    'Pipfile.lock', '.eslintcache', '*.min.js', '*.min.css',
    'LICENSE', 'LICENSE.*', 'COPYING', 'NOTICE', '*.ipynb_checkpoints*', 'go.sum'
}

def should_exclude_item(
    item_path_obj: Path,
    item_name: str,
    is_dir: bool,
    gitignore_matcher_func 
):
    if is_dir:
        if item_name in GLOBAL_IGNORED_DIRS:
            return True
    else: 
        for pattern in GLOBAL_IGNORED_FILES:
            if fnmatch_lib.fnmatch(item_name, pattern):
                return True

    if gitignore_matcher_func:
        try:
            if gitignore_matcher_func(item_path_obj): 
                return True
        except Exception:
            pass
            
    return False

def get_item_status_info(
    item_path_obj: Path,
    item_name: str,
    is_dir: bool,
    log_widget_ref, 
    model_name_for_tokens: str = "gpt-4"
):
    status_list_of_tags = [] 
    status_message = ""
    token_count = 0  

    if is_dir:
        return status_list_of_tags, status_message, token_count

    if item_path_obj.suffix.lower() in BINARY_EXTENSIONS:
        status_list_of_tags.append(BINARY_STATUS_TAG)
        status_message = "бинарный"
        token_count = None  
    else:
        file_size = -1
        try:
            if hasattr(item_path_obj, 'stat'):
                stat_result = item_path_obj.stat()
                file_size = stat_result.st_size
            else: 
                status_list_of_tags.append(ERROR_STATUS_TAG)
                status_message = "неверный объект пути"
                token_count = None
        except OSError as e:
            status_list_of_tags.append(ERROR_STATUS_TAG)
            status_message = f"ошибка доступа к файлу: {e.strerror}"
            token_count = None
        
        if token_count is not None and file_size != -1: 
            if file_size > MAX_FILE_SIZE_BYTES:
                status_list_of_tags.append(LARGE_FILE_STATUS_TAG)
                status_message = f"> {MAX_FILE_SIZE_BYTES // (1024*1024)}MB"
                token_count = None
            else:
                token_val, token_err_msg = count_file_tokens(
                    str(item_path_obj), log_widget_ref, model_name_for_tokens
                )
                
                if token_err_msg:
                    status_message = token_err_msg 
                    token_count = None
                    if "бинарный" in token_err_msg and BINARY_STATUS_TAG not in status_list_of_tags:
                        status_list_of_tags.append(BINARY_STATUS_TAG)
                    elif ERROR_STATUS_TAG not in status_list_of_tags:
                         status_list_of_tags.append(ERROR_STATUS_TAG)
                elif token_val is not None:
                    token_count = token_val
                    if token_count > MAX_TOKENS_FOR_DISPLAY:
                        status_list_of_tags.append(TOO_MANY_TOKENS_STATUS_TAG)
                        formatted_max_tokens = f"{MAX_TOKENS_FOR_DISPLAY:,}".replace(",", " ")
                        if not status_message: 
                            status_message = f"токенов > {formatted_max_tokens}"
                        else: 
                            status_message += f", токенов > {formatted_max_tokens}"

    is_excluded_by_default = False
    for pattern in EXCLUDED_BY_DEFAULT_PATTERNS:
        if fnmatch_lib.fnmatch(item_name, pattern):
            is_excluded_by_default = True
            break
    
    if is_excluded_by_default:
        if EXCLUDED_BY_DEFAULT_STATUS_TAG not in status_list_of_tags:
            status_list_of_tags.append(EXCLUDED_BY_DEFAULT_STATUS_TAG)
        if not status_message: 
            status_message = "исключен по умолчанию"
            
    return status_list_of_tags, status_message, token_count