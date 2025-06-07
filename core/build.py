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
    # script_name теперь просто "main.py", так как он будет в корне проекта
    script_name = "main.py" 
    base_output_name = "project_agent" 
    icon_path_for_exe = "app_icon.ico" # Иконка тоже в корне проекта
    # control_doc_name = "change_control_doc.md" # Не используется
    use_upx = True
    console = False 

    current_date = datetime.now()
    version_string = current_date.strftime("%y.%m.%d")
    output_name_for_final_exe = f"{base_output_name}_v{version_string}" 
    
    fixed_spec_filename = f"{base_output_name}.spec"

    # Путь к скрипту относительно текущей директории, откуда запускается build.py
    # Если build.py в core/, а main.py в корне, то путь будет "../main.py"
    # Но если вы запускаете `python core/build.py` из корня `project_agent/`,
    # то Path("main.py") будет корректно указывать на `project_agent/main.py`.
    # Давайте исходить из того, что `build.py` запускается из КОРНЯ проекта.
    # Если `build.py` перемещен в корень, то `script_name` остается "main.py".
    # Если `build.py` в `core/`, то:
    # script_path = Path("..") / script_name # Поднимаемся на один уровень из core/
    # Пока оставим так, предполагая запуск build.py из корня.
    script_path = Path(script_name)

    if not script_path.is_file():
        print(f"Ошибка: Основной скрипт '{script_path}' не найден. Убедитесь, что он в корне проекта и build.py запускается из корня.");
        sys.exit(1)

    icon_file_obj = Path(icon_path_for_exe) if icon_path_for_exe else None
    if icon_file_obj and not icon_file_obj.is_file():
        print(f"Предупреждение: Иконка '{icon_path_for_exe}' не найдена в корне проекта.");
        icon_file_obj = None
    
    # Аргументы для PyInstaller
    args = [
        str(script_path.resolve()), # Передаем абсолютный путь к main.py
        "--onefile",
        f"--name={base_output_name}", 
        # Скрытые импорты (hidden imports)
        # Если main.py теперь в корне, а модули в core/, PyInstaller должен сам их найти через from core.xxx
        # Но на всякий случай можно оставить, если есть динамические импорты или что-то не находится.
        # Либо указать '--paths=core' чтобы PyInstaller искал модули и в папке core.
        # Я бы добавил '--paths=.' и '--paths=core' для надежности.
        "--paths=.", # Искать в текущей директории (корень проекта)
        "--paths=core", # Искать в папке core
        "--hidden-import=pyperclip",
        "--hidden-import=tkinter.ttk",
        "--hidden-import=diff_match_patch",
        "--hidden-import=fnmatch", # Используется в fs_scanner_utils
        "--hidden-import=tiktoken",
        "--hidden-import=tiktoken_ext",
        "--hidden-import=tiktoken_ext.openai_public",
        "--hidden-import=tiktoken_ext.cl100k_base",
        "--hidden-import=regex", # Зависимость tiktoken
        "--hidden-import=charset_normalizer", # Зависимость requests/pyperclip?
        "--hidden-import=idna", # Зависимость requests/pyperclip?
        "--hidden-import=gitignore_parser",
    ]

    if not console: args.append("--noconsole")
    if icon_file_obj: args.append(f"--icon={str(icon_file_obj.resolve())}")

    # Добавление данных: иконка и папка doc
    # Пути к данным должны быть относительно директории, где запускается PyInstaller (корень проекта)
    app_icon_for_data = Path("app_icon.ico") 
    if app_icon_for_data.is_file():
        args.append(f"--add-data={str(app_icon_for_data.resolve())}{os.pathsep}.")
        print(f"Добавление файла иконки '{app_icon_for_data.name}' как данных -> корень сборки")

    instructions_source_dir = Path("doc") # Папка doc в корне проекта
    if instructions_source_dir.is_dir():
        for item in instructions_source_dir.iterdir():
            if item.is_file() and item.suffix == '.md': 
                destination_in_bundle = instructions_source_dir.name 
                args.append(f"--add-data={str(item.resolve())}{os.pathsep}{destination_in_bundle}")
                print(f"Добавление файла инструкции: {item.name} -> {destination_in_bundle}/{item.name} в сборке")
    else:
        print(f"Предупреждение: Папка инструкций '{instructions_source_dir}' не найдена. Инструкции не будут включены.")
    
    # Добавление данных для tiktoken
    try:
        import tiktoken as tk_module
        tiktoken_path = Path(tk_module.__file__).parent
        # Путь назначения в сборке: 'tiktoken'
        args.append(f"--add-data={str(tiktoken_path.resolve())}{os.pathsep}tiktoken")
        
        # Добавление tiktoken_ext, если есть
        tiktoken_ext_path_candidate = tiktoken_path / "tiktoken_ext"
        if tiktoken_ext_path_candidate.is_dir():
             args.append(f"--add-data={str(tiktoken_ext_path_candidate.resolve())}{os.pathsep}tiktoken/tiktoken_ext")
        
        # Добавление файлов encodings
        encodings_dir_candidate = tiktoken_path / "encodings"
        if encodings_dir_candidate.is_dir():
            for item_encoding in encodings_dir_candidate.iterdir():
                if item_encoding.is_file() and item_encoding.suffix == '.tiktoken':
                    args.append(f"--add-data={str(item_encoding.resolve())}{os.pathsep}tiktoken/encodings")
        print(f"Добавлены данные для tiktoken из {tiktoken_path}")
    except ImportError:
        print("tiktoken не импортирован, данные для него не добавляются.")
    except Exception as e_tiktoken_data:
        print(f"Ошибка при добавлении данных tiktoken: {e_tiktoken_data}")

    if use_upx and check_upx():
        print("UPX найден, будет использован.")
        try:
            upx_dir_path = Path(shutil.which("upx") or "upx").parent
            args.append(f"--upx-dir={str(upx_dir_path.resolve())}")
        except Exception: # shutil.which может вернуть None, если upx не в PATH
            print("Не удалось определить путь к UPX. UPX не будет использован.")
    elif use_upx:
        print("UPX не найден, сжатие не будет использовано.")

    build_dir = Path("build")
    if build_dir.exists() and build_dir.is_dir():
        print(f"Удаление папки 'build': {build_dir}")
        shutil.rmtree(build_dir)

    spec_file_to_clean = Path(fixed_spec_filename)
    if spec_file_to_clean.exists():
        print(f"Удаление предыдущего .spec файла: {spec_file_to_clean}")
        os.remove(spec_file_to_clean)
    
    old_spec_patterns_to_remove = [f"{base_output_name}_v*.spec", "ApplyDiffGUI*.spec", f"{output_name_for_final_exe}.spec"]
    for pattern in old_spec_patterns_to_remove:
        for old_spec_file in Path(".").glob(pattern):
            if old_spec_file.is_file() and old_spec_file.name != fixed_spec_filename:
                try:
                    print(f"Удаление старого/неверного .spec файла: {old_spec_file}")
                    os.remove(old_spec_file)
                except OSError as e_rm_spec:
                    print(f"Не удалось удалить старый .spec: {old_spec_file}, ошибка: {e_rm_spec}")

    dist_dir = Path("dist")
    if not dist_dir.exists():
        dist_dir.mkdir(parents=True, exist_ok=True)

    previous_versioned_exe_path = dist_dir / f"{output_name_for_final_exe}.exe"
    if previous_versioned_exe_path.exists():
        print(f"Удаление предыдущей сборки (версионной): {previous_versioned_exe_path}")
        try: os.remove(previous_versioned_exe_path)
        except OSError as e_rm_ver: print(f"Не удалось удалить {previous_versioned_exe_path}: {e_rm_ver}")

    previous_unversioned_exe_path = dist_dir / f"{base_output_name}.exe"
    if previous_unversioned_exe_path.exists():
        print(f"Удаление предыдущей сборки (неверсионной): {previous_unversioned_exe_path}")
        try: os.remove(previous_unversioned_exe_path)
        except OSError as e_rm_unver: print(f"Не удалось удалить {previous_unversioned_exe_path}: {e_rm_unver}")

    print(f"\nСобираемый файл будет (после переименования): {output_name_for_final_exe}.exe (в папке 'dist')")
    print(f"Имя .spec файла будет: {fixed_spec_filename}")
    print("Аргументы PyInstaller:", " ".join(args), "\nСборка началась...")
    try:
        PyInstaller.__main__.run(args)
        
        generated_exe_path = dist_dir / f"{base_output_name}.exe"
        final_exe_path = dist_dir / f"{output_name_for_final_exe}.exe"

        if generated_exe_path.exists():
            if final_exe_path.exists(): 
                try: os.remove(final_exe_path)
                except OSError as e_mv:
                    print(f"Критическая ошибка: Не удалось удалить существующий файл {final_exe_path} перед переименованием: {e_mv}")
            
            print(f"Переименование {generated_exe_path} в {final_exe_path}")
            shutil.move(str(generated_exe_path), str(final_exe_path))
            print(f"\nСборка завершена! Файл: '{final_exe_path}'")
            print(f".spec файл: '{fixed_spec_filename}'")
        else:
            print(f"\nОшибка: собранный файл {generated_exe_path} (ожидался от PyInstaller) не найден.")
            sys.exit(1)
    except Exception as e_build:
        print(f"\nОшибка сборки: {e_build}");
        sys.exit(1)


if __name__ == "__main__":
    # Важно: этот скрипт сборки должен запускаться из КОРНЕВОЙ директории проекта.
    # Например: python core/build.py (если вы в project_agent/)
    # или python build.py (если build.py перемещен в корень project_agent/)
    # Для текущей структуры (build.py в core/), запускать из корня:
    # cd path/to/project_agent
    # python core/build.py
    build_executable()