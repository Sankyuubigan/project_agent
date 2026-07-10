# file_processing.py
import os
import sys
from pathlib import Path
import hashlib
import tkinter as tk 

# Lightweight tokenizer (tiktoken instead of transformers)
import tiktoken

# Global variable for storing the initialized tokenizer
tokenizer = None
tokenizer_initialization_error = None

# Constants
BINARY_EXTENSIONS = {'.jpg', '.jpeg', '.png', '.gif', '.bmp', '.tiff', '.webp', '.svg', '.ico', '.mp3', '.wav', '.aac', '.ogg', '.flac', '.m4a', '.mp4', '.mkv', '.avi', '.mov', '.wmv', '.flv', '.webm', '.pdf', '.doc', '.docx', '.xls', '.xlsx', '.ppt', '.pptx', '.odt', '.ods', '.odp', '.zip', '.rar', '.tar', '.gz', '.bz2', '.7z', '.jar', '.war', '.exe', '.dll', '.so', '.dylib', '.app', '.msi', '.sqlite', '.db', '.mdb', '.ttf', '.otf', '.woff', '.woff2', '.pyc', '.pyo', '.pyd', '.class', '.bundle', '.swf', '.dat', '.bin', '.obj', '.lib', '.a', '.pak', '.assets', '.resource', '.resS'}
MAX_FILE_SIZE_BYTES = 1 * 1024 * 1024
MAX_TOKENS_FOR_DISPLAY = 50000

def initialize_tokenizer(log_widget_ref=None):
    """
    Initializes the tokenizer (tiktoken/gpt2). This will crash if it fails.
    """
    global tokenizer, tokenizer_initialization_error
    if tokenizer is not None or tokenizer_initialization_error is not None:
        return

    if log_widget_ref and log_widget_ref.winfo_exists():
        log_widget_ref.insert(tk.END, "Инициализация токенизатора 'gpt2'...\n", ('info',))
    
    try:
        tokenizer = tiktoken.get_encoding("gpt2")
    except Exception:
        from tiktoken_ext.openai_public import gpt2 as _gpt2_ctor
        tokenizer = tiktoken.Encoding(**_gpt2_ctor())
    
    if log_widget_ref and log_widget_ref.winfo_exists():
        log_widget_ref.insert(tk.END, "Токенизатор успешно инициализирован.\n", ('success',))

def resource_path(relative_path_from_root):
    """
    Returns the absolute path to a resource.
    Checks in order: MEIPASS > exe dir > source dir.
    """
    # 1) PyInstaller MEIPASS
    if hasattr(sys, '_MEIPASS'):
        candidate = os.path.join(sys._MEIPASS, relative_path_from_root)
        if os.path.exists(candidate):
            return candidate

    # 2) Next to the exe
    try:
        candidate = os.path.join(os.path.dirname(sys.executable), relative_path_from_root)
        if os.path.exists(candidate):
            return candidate
    except Exception:
        pass

    # 3) Development mode (relative to project root)
    return os.path.join(str(Path(__file__).resolve().parent.parent), relative_path_from_root)

def calculate_file_hash(file_path):
    hasher = hashlib.sha256()
    if not os.path.isfile(file_path):
        return "not_a_file"
    
    # This will crash on permission errors.
    with open(file_path, 'rb') as f:
        while chunk := f.read(4096): 
            hasher.update(chunk)
    return hasher.hexdigest()

def count_file_tokens(file_path_str, log_widget_ref, model_name="gpt2"):
    file_path_obj = Path(file_path_str)
    file_name_for_log = file_path_obj.name 

    if tokenizer is None:
        if tokenizer_initialization_error:
            return None, tokenizer_initialization_error
        return None, "Токенизатор не инициализирован."

    if not file_path_obj.is_file():
        return None, "файл не найден"

    file_size = file_path_obj.stat().st_size
    if file_size > MAX_FILE_SIZE_BYTES * 5: 
         return None, f"файл > {MAX_FILE_SIZE_BYTES*5 // (1024*1024)} MB"

    # This block will crash on UnicodeDecodeError or other read errors.
    with open(file_path_obj, 'r', encoding='utf-8') as f:
        content = f.read()

    if not content.strip(): 
        return 0, None 

    # This will crash if the tokenizer fails on the content.
    num_tokens = len(tokenizer.encode(content))
    return num_tokens, None