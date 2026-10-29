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

    // 更新通道：公开（社区）构建默认走 GitHub Releases，完全不读
    // update-channels.json；只有官方发版流程（deploy/self-update/build-local.mjs）
    // 设 YESCHOY_DISTRIBUTION=official|partner（即 OFFICIAL_BUILD）时才编入 ergou 官方更新源。
    // 默认 community：走 GitHub / 关闭自动更新，见 docs/BRANDING.md。
    let distribution =
        std::env::var("YESCHOY_DISTRIBUTION").unwrap_or_else(|_| "community".to_owned());
    let manifest_dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let channels_path = manifest_dir.join("update-channels.json");
    let channels = match distribution.as_str() {
        "community" => "{}".to_owned(),
        "official" | "partner" => std::fs::read_to_string(&channels_path).unwrap_or_else(|_| {
            panic!(
                "YESCHOY_DISTRIBUTION={distribution} needs src-tauri/update-channels.json \
                 (official release pipeline only)"
            )
        }),
        other => panic!("YESCHOY_DISTRIBUTION must be community, official or partner, got {other}"),
    };
    std::fs::write(out_dir.join("update-channels.json"), channels).unwrap();
    let repo = std::env::var("YESCHOY_GITHUB_REPO")
        .unwrap_or_else(|_| "DaydreamZXG/yeschoy-ai-desktop".to_owned());
    let pubkey = std::env::var("YESCHOY_COMMUNITY_UPDATER_PUBKEY").unwrap_or_default();
    println!("cargo:rerun-if-env-changed=YESCHOY_DISTRIBUTION");
    println!("cargo:rerun-if-env-changed=YESCHOY_GITHUB_REPO");
    println!("cargo:rerun-if-env-changed=YESCHOY_MODEL_CATALOG_URL");
    println!("cargo:rerun-if-env-changed=YESCHOY_COMMUNITY_UPDATER_PUBKEY");
    // 野菜提示词注入（预留，默认关闭），见 src/prompt_injection.rs。
    println!("cargo:rerun-if-env-changed=YESCHOY_PROMPT_INJECTION");
    println!("cargo:rerun-if-env-changed=YESCHOY_PROMPT_INJECTION_TEXT");
    println!("cargo:rerun-if-changed=update-channels.json");
    println!("cargo:rustc-env=YESCHOY_DISTRIBUTION={distribution}");
    println!("cargo:rustc-env=YESCHOY_GITHUB_REPO={repo}");
    println!("cargo:rustc-env=YESCHOY_COMMUNITY_UPDATER_PUBKEY={pubkey}");

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
