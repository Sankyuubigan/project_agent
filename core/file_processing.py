# file_processing.py
import os
import sys
from pathlib import Path
import hashlib
import tkinter as tk 

# Заменяем tiktoken на transformers
try:
    from transformers import AutoTokenizer
except ImportError:
    AutoTokenizer = None

# Глобальная переменная для хранения инициализированного токенизатора
tokenizer = None
tokenizer_initialization_error = None

# Константы
BINARY_EXTENSIONS = {'.jpg', '.jpeg', '.png', '.gif', '.bmp', '.tiff', '.webp', '.svg', '.ico', '.mp3', '.wav', '.aac', '.ogg', '.flac', '.m4a', '.mp4', '.mkv', '.avi', '.mov', '.wmv', '.flv', '.webm', '.pdf', '.doc', '.docx', '.xls', '.xlsx', '.ppt', '.pptx', '.odt', '.ods', '.odp', '.zip', '.rar', '.tar', '.gz', '.bz2', '.7z', '.jar', '.war', '.exe', '.dll', '.so', '.dylib', '.app', '.msi', '.sqlite', '.db', '.mdb', '.ttf', '.otf', '.woff', '.woff2', '.pyc', '.pyo', '.pyd', '.class', '.bundle', '.swf', '.dat', '.bin', '.obj', '.lib', '.a', '.pak', '.assets', '.resource', '.resS'}
MAX_FILE_SIZE_BYTES = 1 * 1024 * 1024
MAX_TOKENS_FOR_DISPLAY = 50000

def initialize_tokenizer(log_widget_ref=None):
    """
    Инициализирует токенизатор Hugging Face. Вызывается один раз при старте.
    """
    global tokenizer, tokenizer_initialization_error
    if tokenizer is not None or tokenizer_initialization_error is not None:
        return # Уже была попытка инициализации

    if not AutoTokenizer:
        tokenizer_initialization_error = "Библиотека 'transformers' не установлена. Пожалуйста, установите ее: pip install transformers"
        if log_widget_ref:
            log_widget_ref.insert(tk.END, f"ОШИБКА: {tokenizer_initialization_error}\n", ('error',))
        return

    try:
        # Используем 'gpt2' как стандартный токенизатор.
        # При первом запуске он скачает файлы в кэш.
        # Все последующие запуски будут использовать кэш оффлайн.
        if log_widget_ref:
            log_widget_ref.insert(tk.END, "Инициализация токенизатора 'gpt2'...\n", ('info',))
        tokenizer = AutoTokenizer.from_pretrained("gpt2")
        if log_widget_ref:
            log_widget_ref.insert(tk.END, "Токенизатор успешно инициализирован.\n", ('success',))
    except Exception as e:
        tokenizer_initialization_error = f"Не удалось загрузить токенизатор. Проверьте интернет-соединение для первого запуска. Ошибка: {e}"
        if log_widget_ref:
            log_widget_ref.insert(tk.END, f"ОШИБКА: {tokenizer_initialization_error}\n", ('error',))
        tokenizer = None


def resource_path(relative_path_from_root):
    """
    Возвращает абсолютный путь к ресурсу.
    """
    try:
        base_path = sys._MEIPASS
    except AttributeError:
        base_path = Path(__file__).resolve().parent.parent
    return os.path.join(base_path, relative_path_from_root)

def calculate_file_hash(file_path):
    hasher = hashlib.sha256()
    if not os.path.isfile(file_path):
        return "not_a_file"
    try:
        with open(file_path, 'rb') as f:
            while chunk := f.read(4096): 
                hasher.update(chunk)
        return hasher.hexdigest()
    except OSError: 
        return "error_calculating_hash"
    except Exception: 
        return "error_calculating_hash"


def count_file_tokens(file_path_str, log_widget_ref, model_name="gpt2"): # model_name больше не используется, но оставим для совместимости
    file_path_obj = Path(file_path_str)
    file_name_for_log = file_path_obj.name 

    # Проверяем, был ли токенизатор успешно инициализирован
    if tokenizer is None:
        # Если была ошибка при старте, возвращаем ее
        if tokenizer_initialization_error:
            return None, tokenizer_initialization_error
        # Если не было ошибки, но токенизатор все равно None (например, transformers не установлен)
        return None, "Токенизатор не инициализирован."

    try:
        file_size = file_path_obj.stat().st_size
        if file_size > MAX_FILE_SIZE_BYTES * 5: 
             return None, f"файл > {MAX_FILE_SIZE_BYTES*5 // (1024*1024)} MB"

        with open(file_path_obj, 'r', encoding='utf-8') as f:
            content = f.read()

    except UnicodeDecodeError:
        return None, "бинарный (ошибка декодирования)"
    except OSError as e:
         if log_widget_ref:
             try:
                 log_widget_ref.insert(tk.END, f"ОШИБКА: Чтение файла '{file_name_for_log}' для подсчета токенов (ОС): {e.strerror}\n", ('error',))
             except: pass
         return None, f"ошибка чтения ОС: {e.strerror}"
    except Exception as e:
        if log_widget_ref:
            try:
                log_widget_ref.insert(tk.END, f"ОШИБКА: Чтение файла '{file_name_for_log}' для подсчета токенов: {str(e)[:50]}\n", ('error',))
            except: pass
        return None, f"ошибка чтения: {str(e)[:50]}"

    if not content.strip(): 
        return 0, None 

    try:
        # Используем токенизатор transformers
        num_tokens = len(tokenizer.encode(content))
        return num_tokens, None
    except Exception as e:
        log_message = f"ОШИБКА: Токенизатор не смог обработать '{file_name_for_log}': {str(e)[:100]}"
        if log_widget_ref:
            try:
                 log_widget_ref.insert(tk.END, log_message + "\n", ('error',))
            except: pass
        return None, f"ошибка токенизатора: {str(e)[:50]}"