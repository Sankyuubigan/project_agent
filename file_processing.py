# file_processing.py
import os
import sys
from pathlib import Path
import hashlib
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
    """Подсчитывает токены для файла."""
    if not tiktoken:
        return None, "tiktoken не установлен"

    file_path_obj = Path(file_path_str)
    try:
        file_size = file_path_obj.stat().st_size
        if file_size > MAX_FILE_SIZE_BYTES * 5:
             return None, f"файл > {MAX_FILE_SIZE_BYTES*5 // (1024*1024)} MB"

        with open(file_path_obj, 'r', encoding='utf-8') as f:
            content = f.read()

    except UnicodeDecodeError:
        return None, "бинарный (ошибка декодирования)"
    except OSError as e: # Ловим ошибки ОС (доступ и т.д.)
         return None, f"ошибка чтения ОС: {e.strerror}"
    except Exception as e: # Другие ошибки чтения
        return None, f"ошибка чтения: {str(e)[:50]}"

    if not content.strip():
        return 0, None

    try:
        # TODO: Реализовать кэширование энкодера, если будет много вызовов с одной моделью
        encoding = tiktoken.encoding_for_model(model_name)
        num_tokens = len(encoding.encode(content))
        return num_tokens, None
    except Exception as e:
        if log_widget_ref:
            # Безопасно вставляем в лог (если он есть)
            try:
                 log_widget_ref.insert(tk.END, f"Ошибка tiktoken для {file_path_obj.name}: {e}\n")
            except: pass # Игнорируем ошибки самого лога
        return None, "ошибка tiktoken"
