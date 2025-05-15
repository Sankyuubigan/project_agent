# file_processing.py
import os
import sys
from pathlib import Path
import hashlib
import tkinter as tk # Для log_widget_ref.insert, если он есть
try:
    import tiktoken
except ImportError:
    tiktoken = None

# Константы
BINARY_EXTENSIONS = {'.jpg', '.jpeg', '.png', '.gif', '.bmp', '.tiff', '.webp', '.svg', '.ico', '.mp3', '.wav', '.aac', '.ogg', '.flac', '.m4a', '.mp4', '.mkv', '.avi', '.mov', '.wmv', '.flv', '.webm', '.pdf', '.doc', '.docx', '.xls', '.xlsx', '.ppt', '.pptx', '.odt', '.ods', '.odp', '.zip', '.rar', '.tar', '.gz', '.bz2', '.7z', '.jar', '.war', '.exe', '.dll', '.so', '.dylib', '.app', '.msi', '.sqlite', '.db', '.mdb', '.ttf', '.otf', '.woff', '.woff2', '.pyc', '.pyo', '.pyd', '.class', '.bundle', '.swf', '.dat', '.bin', '.obj', '.lib', '.a', '.pak', '.assets', '.resource', '.resS'}
MAX_FILE_SIZE_BYTES = 1 * 1024 * 1024
MAX_TOKENS_FOR_DISPLAY = 50000

def resource_path(relative_path):
    """ Возвращает абсолютный путь к ресурсу """
    try:
        # PyInstaller временная папка
        base_path = sys._MEIPASS
    except AttributeError: # Используем AttributeError вместо общего Exception
        # Обычный режим
        base_path = os.path.abspath(Path(__file__).parent)
    return os.path.join(base_path, relative_path)

def calculate_file_hash(file_path):
    """Вычисляет SHA-256 хэш файла."""
    hasher = hashlib.sha256()
    if not os.path.isfile(file_path):
        return "not_a_file"
    try:
        with open(file_path, 'rb') as f:
            while chunk := f.read(4096): # Более современный способ чтения по чанкам
                hasher.update(chunk)
        return hasher.hexdigest()
    except OSError as e: # Ловим конкретно OSError
        print(f"Error calculating hash for {file_path}: {e}")
        return "error_calculating_hash"
    except Exception as e: # Ловим остальные непредвиденные ошибки
        print(f"Unexpected error calculating hash for {file_path}: {e}")
        return "error_calculating_hash"


def count_file_tokens(file_path_str, log_widget_ref, model_name="gpt-4"):
    """Подсчитывает токены для файла. Добавлено логирование для Задачи 3."""
    file_path_obj = Path(file_path_str)
    file_name_for_log = file_path_obj.name # Для краткости в логах

    if not tiktoken:
        if log_widget_ref:
            try:
                log_widget_ref.insert(tk.END, f"ПРЕДУПРЕЖДЕНИЕ: tiktoken не установлен. Токены для '{file_name_for_log}' не посчитаны.\n")
            except: pass
        return None, "tiktoken не установлен"

    try:
        file_size = file_path_obj.stat().st_size
        if file_size > MAX_FILE_SIZE_BYTES * 5: # Лимит размера файла для попытки чтения
             if log_widget_ref:
                 try:
                     log_widget_ref.insert(tk.END, f"ИНФО: Файл '{file_name_for_log}' ({file_size // (1024*1024)}MB) слишком большой, токены не считаются.\n")
                 except: pass
             return None, f"файл > {MAX_FILE_SIZE_BYTES*5 // (1024*1024)} MB"

        with open(file_path_obj, 'r', encoding='utf-8') as f:
            content = f.read()

    except UnicodeDecodeError:
        if log_widget_ref:
            try:
                log_widget_ref.insert(tk.END, f"ИНФО: Файл '{file_name_for_log}' бинарный (ошибка декодирования), токены не считаются.\n")
            except: pass
        return None, "бинарный (ошибка декодирования)"
    except OSError as e:
         if log_widget_ref:
             try:
                 log_widget_ref.insert(tk.END, f"ОШИБКА: Чтение файла '{file_name_for_log}' для подсчета токенов (ОС): {e.strerror}\n")
             except: pass
         return None, f"ошибка чтения ОС: {e.strerror}"
    except Exception as e:
        if log_widget_ref:
            try:
                log_widget_ref.insert(tk.END, f"ОШИБКА: Чтение файла '{file_name_for_log}' для подсчета токенов: {str(e)[:50]}\n")
            except: pass
        return None, f"ошибка чтения: {str(e)[:50]}"

    if not content.strip(): # Пустой или только пробельные символы
        return 0, None # 0 токенов, ошибки нет

    try:
        encoding = tiktoken.encoding_for_model(model_name)
        num_tokens = len(encoding.encode(content))
        return num_tokens, None
    except Exception as e:
        log_message = f"ОШИБКА: tiktoken не смог обработать '{file_name_for_log}': {str(e)[:100]}"
        if log_widget_ref:
            try:
                 log_widget_ref.insert(tk.END, log_message + "\n")
            except: pass
        return None, f"ошибка tiktoken: {str(e)[:50]}"