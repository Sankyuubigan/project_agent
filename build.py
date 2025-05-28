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
    script_name = "main.py"
    base_output_name = "project_agent" 
    icon_path_for_exe = "app_icon.ico" 
    control_doc_name = "change_control_doc.md" # Это теперь не используется для инструкций
    use_upx = True
    console = False 

    current_date = datetime.now()
    version_string = current_date.strftime("%y.%m.%d")
    output_name_for_final_exe = f"{base_output_name}_v{version_string}" 
    
    fixed_spec_filename = f"{base_output_name}.spec"

    script_path = Path(script_name)
    if not script_path.is_file():
        print(f"Ошибка: Основной скрипт '{script_name}' не найден.");
        sys.exit(1)

    icon_file_obj = Path(icon_path_for_exe) if icon_path_for_exe else None
    if icon_file_obj and not icon_file_obj.is_file():
        print(f"Предупреждение: Иконка '{icon_path_for_exe}' не найдена.");
        icon_file_obj = None

    # control_doc_file больше не нужен для основного процесса, если инструкции теперь отдельные
    # control_doc_file = Path(control_doc_name) 

    args = [
        str(script_path),
        "--onefile",
        f"--name={base_output_name}", 
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
    if icon_file_obj: args.append(f"--icon={str(icon_file_obj.resolve())}")

    app_icon_for_data = Path("app_icon.ico") 
    if app_icon_for_data.is_file():
        args.append(f"--add-data={str(app_icon_for_data.resolve())}{os.pathsep}.")
        print(f"Добавление файла иконки '{app_icon_for_data.name}' как данных -> корень сборки")

    # --- ДОБАВЛЕНИЕ ФАЙЛОВ ИНСТРУКЦИЙ ИЗ ПАПКИ 'doc' ---
    instructions_source_dir = Path("doc")
    if instructions_source_dir.is_dir():
        for item in instructions_source_dir.iterdir():
            if item.is_file() and item.suffix == '.md': # Добавляем только .md файлы из папки doc
                # Путь назначения в сборке будет 'doc/имя_файла.md'
                destination_in_bundle = instructions_source_dir.name # Имя папки 'doc'
                args.append(f"--add-data={str(item.resolve())}{os.pathsep}{destination_in_bundle}")
                print(f"Добавление файла инструкции: {item.name} -> {destination_in_bundle}/{item.name} в сборке")
    else:
        print(f"Предупреждение: Папка инструкций '{instructions_source_dir}' не найдена. Инструкции не будут включены.")
    # ----------------------------------------------------
    
    # Старый код для change_control_doc.md, если он все еще нужен для чего-то другого,
    # можно оставить, но для инструкций он больше не используется.
    # if control_doc_file and control_doc_file.is_file():
    #     data_arg = f"{control_doc_file.resolve()}{os.pathsep}."
    #     args.append(f"--add-data={data_arg}")
    #     print(f"Добавление файла данных: {control_doc_file} -> корень сборки")
    # else:
    #     print(f"Файл данных '{control_doc_name}' не найден и не будет включен в сборку.")

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
                except OSError as e:
                    print(f"Не удалось удалить старый .spec: {old_spec_file}, ошибка: {e}")

    dist_dir = Path("dist")
    if not dist_dir.exists():
        dist_dir.mkdir(parents=True, exist_ok=True)

    previous_versioned_exe_path = dist_dir / f"{output_name_for_final_exe}.exe"
    if previous_versioned_exe_path.exists():
        print(f"Удаление предыдущей сборки (версионной): {previous_versioned_exe_path}")
        try: os.remove(previous_versioned_exe_path)
        except OSError as e: print(f"Не удалось удалить {previous_versioned_exe_path}: {e}")

    previous_unversioned_exe_path = dist_dir / f"{base_output_name}.exe"
    if previous_unversioned_exe_path.exists():
        print(f"Удаление предыдущей сборки (неверсионной): {previous_unversioned_exe_path}")
        try: os.remove(previous_unversioned_exe_path)
        except OSError as e: print(f"Не удалось удалить {previous_unversioned_exe_path}: {e}")

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
                except OSError as e:
                    print(f"Критическая ошибка: Не удалось удалить существующий файл {final_exe_path} перед переименованием: {e}")
            
            print(f"Переименование {generated_exe_path} в {final_exe_path}")
            shutil.move(str(generated_exe_path), str(final_exe_path))
            print(f"\nСборка завершена! Файл: '{final_exe_path}'")
            print(f".spec файл: '{fixed_spec_filename}'")
        else:
            print(f"\nОшибка: собранный файл {generated_exe_path} (ожидался от PyInstaller) не найден.")
            sys.exit(1)
    except Exception as e:
        print(f"\nОшибка сборки: {e}");
        sys.exit(1)


if __name__ == "__main__":
    build_executable()