# core/treeview_constants.py
# Этот файл содержит общие константы для логики и сканера дерева,
# чтобы избежать циклических импортов.

# --- Теги состояний ---
CHECKED_TAG = "checked"
UNCHECKED_TAG = "unchecked"
TRISTATE_TAG = "tristate" # Для папок с частичным выделением

# --- Символы для отображения ---
CHECK_CHAR = "☑"
UNCHECK_CHAR = "☐"
# Для папок с частичным выделением можно использовать тот же символ, что и для выделенных.
TRISTATE_CHAR = "☑" 

# --- Теги для стилизации ---
BINARY_TAG_UI = "status_binary"
LARGE_FILE_TAG_UI = "status_large_file"
ERROR_TAG_UI = "status_error"
EXCLUDED_BY_DEFAULT_TAG_UI = "status_excluded_default"
TOO_MANY_TOKENS_TAG_UI = "status_too_many_tokens"