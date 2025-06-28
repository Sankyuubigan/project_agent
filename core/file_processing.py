# file_processing.py
import os
import sys
from pathlib import Path
import hashlib
import tkinter as tk 

# This import will crash the app if 'transformers' is not installed.
from transformers import AutoTokenizer

# Global variable for storing the initialized tokenizer
tokenizer = None
tokenizer_initialization_error = None

# Constants
BINARY_EXTENSIONS = {'.jpg', '.jpeg', '.png', '.gif', '.bmp', '.tiff', '.webp', '.svg', '.ico', '.mp3', '.wav', '.aac', '.ogg', '.flac', '.m4a', '.mp4', '.mkv', '.avi', '.mov', '.wmv', '.flv', '.webm', '.pdf', '.doc', '.docx', '.xls', '.xlsx', '.ppt', '.pptx', '.odt', '.ods', '.odp', '.zip', '.rar', '.tar', '.gz', '.bz2', '.7z', '.jar', '.war', '.exe', '.dll', '.so', '.dylib', '.app', '.msi', '.sqlite', '.db', '.mdb', '.ttf', '.otf', '.woff', '.woff2', '.pyc', '.pyo', '.pyd', '.class', '.bundle', '.swf', '.dat', '.bin', '.obj', '.lib', '.a', '.pak', '.assets', '.resource', '.resS'}
MAX_FILE_SIZE_BYTES = 1 * 1024 * 1024
MAX_TOKENS_FOR_DISPLAY = 50000

def initialize_tokenizer(log_widget_ref=None):
    """
    Initializes the Hugging Face tokenizer. This will crash if it fails.
    """
    global tokenizer, tokenizer_initialization_error
    if tokenizer is not None or tokenizer_initialization_error is not None:
        return

    if log_widget_ref and log_widget_ref.winfo_exists():
        log_widget_ref.insert(tk.END, "Инициализация токенизатора 'gpt2'...\n", ('info',))
    
    # This will crash on network error or other issues.
    tokenizer = AutoTokenizer.from_pretrained("gpt2")
    
    if log_widget_ref and log_widget_ref.winfo_exists():
        log_widget_ref.insert(tk.END, "Токенизатор успешно инициализирован.\n", ('success',))

def resource_path(relative_path_from_root):
    """
    Returns the absolute path to a resource.
    """
    if hasattr(sys, '_MEIPASS'):
        base_path = sys._MEIPASS
    else:
        base_path = Path(__file__).resolve().parent.parent
    return os.path.join(base_path, relative_path_from_root)

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