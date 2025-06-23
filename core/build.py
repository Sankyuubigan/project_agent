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
    # Важно: предполагается, что этот скрипт запускается из корня проекта,
    # например: python core/build.py
    script_name = "core/main.py" 
    base_output_name = "project_agent" 
    icon_path_for_exe = "app_icon.ico" 
    use_upx = True
    console = False 

    current_date = datetime.now()
    version_string = current_date.strftime("%y.%m.%d")
    output_name_for_final_exe = f"{base_output_name}_v{version_string}" 
    
    fixed_spec_filename = f"{base_output_name}.spec"

    script_path = Path(script_name)

    if not script_path.is_file():
        print(f"Ошибка: Основной скрипт '{script_path}' не найден. Убедитесь, что структура проекта верна и build.py запускается из корня.");
        sys.exit(1)

    icon_file_obj = Path(icon_path_for_exe) if icon_path_for_exe else None
    if icon_file_obj and not icon_file_obj.is_file():
        print(f"Предупреждение: Иконка '{icon_path_for_exe}' не найдена в корне проекта.");
        icon_file_obj = None
    
    args = [
        str(script_path.resolve()), 
        "--onefile",
        f"--name={base_output_name}", 
        # Указываем PyInstaller, где искать модули
        "--paths=.", # Корень проекта, чтобы он нашел пакет 'core'
        
        # --- СКРЫТЫЕ ИМПОРТЫ ---
        # Основные зависимости
        "--hidden-import=pyperclip",
        "--hidden-import=tkinter.ttk",
        "--hidden-import=diff_match_patch",
        "--hidden-import=fnmatch", 
        
        # Зависимости для Transformers
        "--hidden-import=transformers",
        "--hidden-import=transformers.models",
        "--hidden-import=transformers.tokenization_utils",
        "--hidden-import=transformers.tokenization_utils_fast",
        "--hidden-import=tokenizers",
        "--hidden-import=tokenizers.models",
        "--hidden-import=tokenizers.pre_tokenizers",
        "--hidden-import=tokenizers.processors",
        "--hidden-import=tokenizers.decoders",
        "--hidden-import=tokenizers.normalizers",
        "--hidden-import=safetensors",
        "--hidden-import=huggingface_hub",
        "--hidden-import=regex",
        "--hidden-import=requests",
        "--hidden-import=packaging",
        "--hidden-import=filelock",
        "--hidden-import=numpy",
        "--hidden-import=pyyaml",
        "--hidden-import=tqdm",
    ]

    if not console: args.append("--noconsole")
    if icon_file_obj: args.append(f"--icon={str(icon_file_obj.resolve())}")

    # --- ДАННЫЕ ---
    app_icon_for_data = Path("app_icon.ico") 
    if app_icon_for_data.is_file():
        args.append(f"--add-data={str(app_icon_for_data.resolve())}{os.pathsep}.")
        print(f"Добавление файла иконки '{app_icon_for_data.name}' как данных -> корень сборки")

    instructions_source_dir = Path("doc") 
    if instructions_source_dir.is_dir():
        for item in instructions_source_dir.iterdir():
            if item.is_file() and item.suffix == '.md': 
                destination_in_bundle = instructions_source_dir.name 
                args.append(f"--add-data={str(item.resolve())}{os.pathsep}{destination_in_bundle}")
                print(f"Добавление файла инструкции: {item.name} -> {destination_in_bundle}/{item.name} в сборке")
    else:
        print(f"Предупреждение: Папка инструкций '{instructions_source_dir}' не найдена. Инструкции не будут включены.")
    
    if use_upx and check_upx():
        print("UPX найден, будет использован.")
        try:
            upx_dir_path = Path(shutil.which("upx") or "upx").parent
            args.append(f"--upx-dir={str(upx_dir_path.resolve())}")
        except Exception: 
            print("Не удалось определить путь к UPX. UPX не будет использован.")
    elif use_upx:
        print("UPX не найден, сжатие не будет использовано.")

    # --- ОЧИСТКА ---
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
    build_executable()