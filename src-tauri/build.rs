fn main() {
    // Манифест в exe вшивает gpui-pre (фича windows-manifest): asInvoker,
    // DPI PerMonitorV2, supportedOS Win10, ComCtl32 v6 — надмножество
    // дефолтного манифеста tauri (только ComCtl32 v6). Свой манифест tauri
    // отключаем: два ресурса MANIFEST (id 1) в одном exe дают CVT1100/LNK1123
    // (риск PRP 14.11.26 «манифесты tauri vs gpui»; решение — new_without_app_manifest).
    let attrs = tauri_build::Attributes::default()
        .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
    if let Err(e) = tauri_build::try_build(attrs) {
        eprintln!("build.rs: tauri-build failed: {e:?}");
        std::process::exit(1);
    }
}
