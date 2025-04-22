# build.py

import os
import PyInstaller.__main__
import sys
import shutil
import subprocess
from pathlib import Path # Используем pathlib для путей

def check_upx():
    # ... (код без изменений)
    try:
        subprocess.run(["upx", "--version"], capture_output=True, check=True)
        return True
    except (subprocess.CalledProcessError, FileNotFoundError):
        return False

def build_executable():
    # Параметры сборки
    script_name = "apply_changes_gui.py"
    output_name = "ApplyDiffGUI"
    icon_path = "icon_launch.ico"
    control_doc_name = "change_control_doc.md" # Имя файла данных
    use_upx = True
    console = False

    # --- Проверки существования файлов ---
    script_path = Path(script_name)
    if not script_path.is_file():
        print(f"Ошибка: Основной скрипт '{script_name}' не найден.")
        sys.exit(1)

    icon_file = Path(icon_path) if icon_path else None
    if icon_file and not icon_file.is_file():
        print(f"Предупреждение: Файл иконки '{icon_path}' не найден. Сборка без иконки.")
        icon_file = None # Сбрасываем, чтобы не использовать

    control_doc_file = Path(control_doc_name)
    if not control_doc_file.is_file():
        print(f"Ошибка: Файл данных '{control_doc_name}' не найден рядом со скриптом сборки.")
        print("Этот файл должен быть добавлен в сборку.")
        sys.exit(1)
    # --- Конец проверок ---

    # Формируем аргументы для PyInstaller
    args = [
        str(script_path),         # Основной скрипт
        "--onefile",              # Собрать в один файл
        f"--name={output_name}",  # Имя выходного файла
        "--hidden-import=pyperclip",
        "--hidden-import=tkinter.ttk", # Явно добавим ttk на всякий случай
        "--hidden-import=diff_match_patch",
        "--hidden-import=fnmatch", # Добавим для игнорирования по маске в дереве
    ]

    if not console:
        args.append("--noconsole")
    if icon_file:
        print(f"Используется иконка: {icon_file}")
        args.append(f"--icon={icon_file}")

    # --- ДОБАВЛЕНИЕ ФАЙЛА ДАННЫХ ---
    # os.pathsep - это разделитель для путей в ОС (';' для Windows, ':' для Linux/macOS)
    # "." означает, что файл будет помещен в корень временной директории (_MEIPASS)
    data_arg = f"{control_doc_file}{os.pathsep}."
    args.append(f"--add-data={data_arg}")
    print(f"Добавление файла данных: {control_doc_file} -> корень сборки")
    # --- КОНЕЦ ДОБАВЛЕНИЯ ---

    if use_upx and check_upx():
        print("UPX найден, будет использован для сжатия.")
        try:
             upx_path = Path(shutil.which("upx") or "upx").parent
             args.append("--upx-dir")
             args.append(str(upx_path))
        except Exception as e:
             print(f"Не удалось определить путь к UPX: {e}. UPX не будет использован.")

    elif use_upx:
        print("UPX не найден в системе, сжатие не будет использовано.")

    # Очищаем предыдущие сборки
    for folder in ["dist", "build"]:
        if os.path.exists(folder):
            print(f"Удаление старой папки: {folder}")
            shutil.rmtree(folder)

    # Запускаем PyInstaller
    print("\nАргументы PyInstaller:")
    print(" ".join(args)) # Выводим финальные аргументы для отладки
    print("\nСборка началась...")
    try:
        PyInstaller.__main__.run(args)
        print(f"\nСборка завершена! Исполняемый файл: 'dist/{output_name}.exe'") # Добавим .exe для Windows
    except Exception as e:
        print(f"\nОшибка при сборке: {str(e)}")
        sys.exit(1)

if __name__ == "__main__":
    build_executable()