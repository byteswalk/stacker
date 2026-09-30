fn main() {
    // tauri-build embeds its manifest (a Common Controls 6 dependency, nothing else) through
    // a resource linked into binaries only, so test binaries had none: one that links a call
    // only Common Controls 6 exports (TaskDialogIndirect, via the dialog plugin) failed to
    // load with STATUS_ENTRYPOINT_NOT_FOUND. The same dependency now goes to every linked
    // target through the linker, and tauri-build adds no second manifest.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        );
        let windows = tauri_build::WindowsAttributes::new_without_app_manifest();
        tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
            .expect("tauri build");
    } else {
        tauri_build::build()
    }
}
