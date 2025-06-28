# core/fs_scanner_utils.py
import os
from pathlib import Path
import fnmatch as fnmatch_lib
import tkinter as tk

from core.file_processing import (
    count_file_tokens, BINARY_EXTENSIONS, MAX_FILE_SIZE_BYTES, MAX_TOKENS_FOR_DISPLAY
)

BINARY_STATUS_TAG = "status_binary"
LARGE_FILE_STATUS_TAG = "status_large_file"
ERROR_STATUS_TAG = "status_error"
EXCLUDED_BY_DEFAULT_STATUS_TAG = "status_excluded_default"
TOO_MANY_TOKENS_STATUS_TAG = "status_too_many_tokens"

DISABLED_LOOK_TAGS_UI = {
    BINARY_STATUS_TAG,
    LARGE_FILE_STATUS_TAG,
    ERROR_STATUS_TAG
}

GLOBAL_IGNORED_DIRS = {
    '.git', '__pycache__', '.vscode', '.idea', 'node_modules', 'venv', '.env',
    'build', 'dist', 'out', 'target', '.pytest_cache', '.mypy_cache', '.tox',
    '.conda', '.condaenv', 'env'
}

GLOBAL_IGNORED_FILES = {
    '.DS_Store', 'Thumbs.db', 'desktop.ini', '*.pyc', '*.pyo', '*.pyd',
    '.so', '*.dll', '*.log', '*.tmp', '*.bak', '*.swp', '*.swo', '.coverage',
    'todo.md'
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

    if gitignore_matcher_func and callable(gitignore_matcher_func):
        if gitignore_matcher_func(item_path_obj): 
            return True
            
    return False

def get_item_status_info(
    item_path_obj: Path,
    item_name: str,
    is_dir: bool,
    log_widget_ref
):
    status_tags = set()
    status_message = ""
    token_count = 0  # По умолчанию токены не считаются

    if is_dir:
        return status_tags, status_message, token_count

    if item_path_obj.suffix.lower() in BINARY_EXTENSIONS:
        status_tags.add(BINARY_STATUS_TAG)
        status_message = "бинарный"
        token_count = None  
    else:
        file_size = -1
        if item_path_obj.exists():
            stat_result = item_path_obj.stat()
            file_size = stat_result.st_size
        else: 
            status_tags.add(ERROR_STATUS_TAG)
            status_message = "неверный объект пути"
            token_count = None
        
        if token_count is not None and file_size != -1: 
            if file_size > MAX_FILE_SIZE_BYTES:
                status_tags.add(LARGE_FILE_STATUS_TAG)
                status_message = f"> {MAX_FILE_SIZE_BYTES // (1024*1024)}MB"
                token_count = None
            # Автоматический подсчет токенов при сканировании удален

    is_excluded_by_default = False
    for pattern in EXCLUDED_BY_DEFAULT_PATTERNS:
        if fnmatch_lib.fnmatch(item_name, pattern):
            is_excluded_by_default = True
            break
    
    if is_excluded_by_default:
        if EXCLUDED_BY_DEFAULT_STATUS_TAG not in status_tags:
            status_tags.add(EXCLUDED_BY_DEFAULT_STATUS_TAG)
        if not status_message: 
            status_message = "исключен по умолчанию"
            
    return status_tags, status_message, token_count