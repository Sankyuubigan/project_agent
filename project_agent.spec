# -*- mode: python ; coding: utf-8 -*-


a = Analysis(
    ['D:\\Projects\\project_agent\\core\\main.py'],
    pathex=['.'],
    binaries=[],
    datas=[('D:\\Projects\\project_agent\\app_icon.ico', '.'), ('D:\\Projects\\project_agent\\doc\\diffmatchpatch_method.md', 'doc'), ('D:\\Projects\\project_agent\\doc\\git_method.md', 'doc'), ('D:\\Projects\\project_agent\\doc\\json_method.md', 'doc'), ('D:\\Projects\\project_agent\\doc\\markdown_method.md', 'doc')],
    hiddenimports=['pyperclip', 'tkinter.ttk', 'diff_match_patch', 'fnmatch', 'tiktoken', 'tiktoken_ext.openai_public'],
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=['torch', 'torchaudio', 'torchvision', 'transformers', 'tensorflow', 'onnx', 'onnxruntime', 'pandas', 'matplotlib', 'scipy', 'sklearn'],
    noarchive=False,
    optimize=0,
)
pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    a.binaries,
    a.datas,
    [],
    name='project_agent',
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=True,
    upx_exclude=[],
    runtime_tmpdir=None,
    console=False,
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
    icon=['D:\\Projects\\project_agent\\app_icon.ico'],
)
