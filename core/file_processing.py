# file_processing.py
import os
import sys
from pathlib import Path
import hashlib
import tkinter as tk 
try:
    import tiktoken
except ImportError:
    tiktoken = None

# Константы
BINARY_EXTENSIONS = {'.jpg', '.jpeg', '.png', '.gif', '.bmp', '.tiff', '.webp', '.svg', '.ico', '.mp3', '.wav', '.aac', '.ogg', '.flac', '.m4a', '.mp4', '.mkv', '.avi', '.mov', '.wmv', '.flv', '.webm', '.pdf', '.doc', '.docx', '.xls', '.xlsx', '.ppt', '.pptx', '.odt', '.ods', '.odp', '.zip', '.rar', '.tar', '.gz', '.bz2', '.7z', '.jar', '.war', '.exe', '.dll', '.so', '.dylib', '.app', '.msi', '.sqlite', '.db', '.mdb', '.ttf', '.otf', '.woff', '.woff2', '.pyc', '.pyo', '.pyd', '.class', '.bundle', '.swf', '.dat', '.bin', '.obj', '.lib', '.a', '.pak', '.assets', '.resource', '.resS'}
MAX_FILE_SIZE_BYTES = 1 * 1024 * 1024
MAX_TOKENS_FOR_DISPLAY = 50000

def resource_path(relative_path_from_root):
    """
    Возвращает абсолютный путь к ресурсу.
    В режиме разработки (не в сборке PyInstaller) ищет от корня проекта.
    В сборке PyInstaller (_MEIPASS) ищет от временной папки сборки.
    """
    try:
        # PyInstaller создает временную папку и сохраняет путь в sys._MEIPASS.
        # Все данные, добавленные через --add-data, будут здесь.
        base_path = sys._MEIPASS
    except AttributeError:
        # Мы не в сборке PyInstaller. Нам нужно найти корень проекта.
        # __file__ -> .../project_agent/core/file_processing.py
        # Path(__file__).parent -> .../project_agent/core
        # Path(__file__).parent.parent -> .../project_agent (это корень проекта)
        base_path = Path(__file__).resolve().parent.parent
        
    # os.path.join корректно соединит Path-объект и строку
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


def count_file_tokens(file_path_str, log_widget_ref, model_name="gpt-4"):
    file_path_obj = Path(file_path_str)
    file_name_for_log = file_path_obj.name 

    if not tiktoken:
        if log_widget_ref:
            try:
                log_widget_ref.insert(tk.END, f"ПРЕДУПРЕЖДЕНИЕ: tiktoken не установлен. Токены для '{file_name_for_log}' не посчитаны.\n", ('error',))
            except: pass
        return None, "tiktoken не установлен"

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
        encoding = tiktoken.encoding_for_model(model_name)
        num_tokens = len(encoding.encode(content))
        return num_tokens, None
    except Exception as e:
        log_message = f"ОШИБКА: tiktoken не смог обработать '{file_name_for_log}': {str(e)[:100]}"
        if log_widget_ref:
            try:
                 log_widget_ref.insert(tk.END, log_message + "\n", ('error',))
            except: pass
        return None, f"ошибка tiktoken: {str(e)[:50]}"