# build.py

import os
import PyInstaller.__main__
import sys
import shutil
import subprocess
from pathlib import Path
from datetime import datetime


def check_upx():
    try:
        subprocess.run(["upx", "--version"], capture_output=True, check=True)
        return True
    except (subprocess.CalledProcessError, FileNotFoundError):
        return False


def build_executable():
    script_name = "apply_changes_gui.py"
    base_output_name = "project_agent"
    icon_path = "icon_launch.ico"
    control_doc_name = "change_control_doc.md"
    use_upx = True
    console = False

    current_date = datetime.now()
    version_string = current_date.strftime("%y.%m.%d")
    output_name_with_version = f"{base_output_name}_v{version_string}"

    script_path = Path(script_name)
    if not script_path.is_file():
        print(f"Ошибка: Основной скрипт '{script_name}' не найден.");
        sys.exit(1)

    icon_file = Path(icon_path) if icon_path else None
    if icon_file and not icon_file.is_file():
        print(f"Предупреждение: Иконка '{icon_path}' не найдена.");
        icon_file = None

    control_doc_file = Path(control_doc_name)

    args = [
        str(script_path),
        "--onefile",
        f"--name={output_name_with_version}",
        "--hidden-import=pyperclip",
        "--hidden-import=tkinter.ttk",
        "--hidden-import=diff_match_patch",
        "--hidden-import=fnmatch",
        "--hidden-import=tiktoken",
        "--hidden-import=tiktoken_ext",
        "--hidden-import=tiktoken_ext.openai_public",
        "--hidden-import=tiktoken_ext.cl100k_base",
        "--hidden-import=regex",
        "--hidden-import=charset_normalizer",
        "--hidden-import=idna",
        "--hidden-import=gitignore_parser",
    ]

    if not console: args.append("--noconsole")
    if icon_file: args.append(f"--icon={icon_file}")

    if control_doc_file and control_doc_file.is_file():
        data_arg = f"{control_doc_file.resolve()}{os.pathsep}."
        args.append(f"--add-data={data_arg}")
        print(f"Добавление файла данных: {control_doc_file} -> корень сборки")
    else:
        print(f"Файл данных '{control_doc_name}' не найден и не будет включен в сборку.")

    try:
        import tiktoken as tk_module
        tiktoken_path = Path(tk_module.__file__).parent
        args.append(f"--add-data={tiktoken_path}{os.pathsep}tiktoken")
        tiktoken_ext_path = tiktoken_path / "tiktoken_ext"
        if tiktoken_ext_path.is_dir():
            args.append(f"--add-data={tiktoken_ext_path}{os.pathsep}tiktoken/tiktoken_ext")
        encodings_path = tiktoken_path / "encodings"
        if encodings_path.is_dir():
            for item in encodings_path.iterdir():
                if item.is_file() and item.suffix == '.tiktoken':
                    args.append(f"--add-data={item}{os.pathsep}tiktoken/encodings")
        print(f"Добавлены данные для tiktoken из {tiktoken_path}")
    except ImportError:
        print("tiktoken не импортирован, данные для него не добавляются.")
    except Exception as e:
        print(f"Ошибка при добавлении данных tiktoken: {e}")

    if use_upx and check_upx():
        print("UPX найден, будет использован.")
        try:
            upx_dir = Path(shutil.which("upx") or "upx").parent
            args.append(f"--upx-dir={upx_dir}")
        except:
            print("Не удалось определить путь к UPX. UPX не будет использован.")
    elif use_upx:
        print("UPX не найден, сжатие не будет использовано.")

    # Очистка: удаляем папку build и .spec файл.
    # Для папки dist: создаем ее, если не существует, и удаляем только предыдущий .exe с таким же именем.

    build_dir = Path("build")
    if build_dir.exists() and build_dir.is_dir():
        shutil.rmtree(build_dir)

    spec_file = Path(f"{output_name_with_version}.spec")
    if spec_file.exists():
        os.remove(spec_file)

    dist_dir = Path("dist")
    if not dist_dir.exists():
        dist_dir.mkdir(parents=True, exist_ok=True)  # Создаем папку dist, если ее нет

    previous_exe_path = dist_dir / f"{output_name_with_version}.exe"
    if previous_exe_path.exists():
        print(f"Удаление предыдущей сборки с таким же именем: {previous_exe_path}")
        os.remove(previous_exe_path)

    print(f"\nСобираемый файл: {output_name_with_version}.exe (в папке 'dist')")
    print("Аргументы PyInstaller:", " ".join(args), "\nСборка началась...")
    try:
        PyInstaller.__main__.run(args)
        print(f"\nСборка завершена! Файл: 'dist/{output_name_with_version}.exe'")
    except Exception as e:
        print(f"\nОшибка сборки: {e}");
        sys.exit(1)


if __name__ == "__main__":
    build_executable()