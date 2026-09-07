fn main() {
    let authorization_page_origin = std::env::var("YESCHOY_AUTHORIZATION_PAGE_ORIGIN")
        .unwrap_or_else(|_| "https://yeschoy.com".to_owned());
    if !matches!(
        authorization_page_origin.as_str(),
        "https://yeschoy.com" | "https://ai.yeschoy.io"
    ) {
        panic!(
            "YESCHOY_AUTHORIZATION_PAGE_ORIGIN must be exactly https://yeschoy.com or https://ai.yeschoy.io"
        );
    }
    println!("cargo:rerun-if-env-changed=YESCHOY_AUTHORIZATION_PAGE_ORIGIN");
    println!("cargo:rustc-env=YESCHOY_AUTHORIZATION_PAGE_ORIGIN={authorization_page_origin}");

    tauri_build::build();

    // Windows: Embed Common Controls v6 manifest for test binaries
    //
    // When running `cargo test`, the generated test executables don't include
    // the standard Tauri application manifest. Without Common Controls v6,
    // `tauri::test` calls fail with STATUS_ENTRYPOINT_NOT_FOUND.
    //
    // This workaround:
    // 1. Embeds the manifest into test binaries via /MANIFEST:EMBED
    // 2. Uses /MANIFEST:NO for the main binary to avoid duplicate resources
    //    (Tauri already handles manifest embedding for the app binary)
    #[cfg(target_os = "windows")]
    {
        let manifest_path = std::path::PathBuf::from(
            std::env::var("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"),
        )
        .join("common-controls.manifest");
        let manifest_arg = format!("/MANIFESTINPUT:{}", manifest_path.display());

        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg={}", manifest_arg);
        // Avoid duplicate manifest resources in binary builds.
        println!("cargo:rustc-link-arg-bins=/MANIFEST:NO");
        println!("cargo:rerun-if-changed={}", manifest_path.display());
    }
}
