import os
import sys
import pyperclip
import hashlib

def process_folder(folder_path):
    # Проверяем, существует ли папка
    if not os.path.isdir(folder_path):
        print(f"Ошибка: Папка '{folder_path}' не существует.")
        sys.exit(1)

    result = []
    file_hashes = {}

    # Рекурсивно обходим папку
    for root, dirs, files in os.walk(folder_path):
        # Игнорируем папки .git и __pycache__
        dirs[:] = [d for d in dirs if d not in ('.git', '__pycache__')]

        for file_name in files:
            file_path = os.path.join(root, file_name)
            # Получаем относительный путь от исходной папки
            relative_path = os.path.relpath(file_path, folder_path)

            try:
                # Читаем содержимое файла
                with open(file_path, 'rb') as f:
                    content = f.read()
                    # Вычисляем хэш
                    hasher = hashlib.sha256()
                    hasher.update(content)
                    file_hashes[relative_path] = hasher.hexdigest()
                    # Декодируем содержимое для копирования
                    content = content.decode('utf-8')
            except UnicodeDecodeError:
                content = "[binary file]"
            except Exception as e:
                content = f"[error reading file: {str(e)}]"

            # Добавляем путь и содержимое в результат
            result.append(f"File: {relative_path}\n{content}\n---")

    # Объединяем все в один текст
    final_text = "\n".join(result)

    # Копируем в буфер обмена
    pyperclip.copy(final_text)
    print(f"Содержимое файлов из '{folder_path}' скопировано в буфер обмена!")
    print("\nХэши файлов:")
    for file_path, file_hash in file_hashes.items():
        print(f"{file_path}: {file_hash}")

if __name__ == "__main__":
    # Проверяем, передан ли аргумент с путём к папке
    if len(sys.argv) != 2:
        print("Использование: python pastemax.py <путь_к_папке>")
        sys.exit(1)

    folder_path = sys.argv[1]
    process_folder(folder_path)