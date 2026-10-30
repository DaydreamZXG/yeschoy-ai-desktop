//! Read-only target resolution, deliberately separate from configuration authority.
use std::path::Path;

/// True when `path` lives inside a macOS `.app` bundle's executable/resources.
///
/// Only used to avoid mistaking an app-bundled CLI for a standalone install when
/// the activation scan resolves a user's executable.
pub(crate) fn is_app_component(path: &Path) -> bool {
    let parts: Vec<_> = path
        .components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect();
    parts.windows(3).any(|parts| {
        parts[0].to_ascii_lowercase().ends_with(".app")
            && parts[1] == "Contents"
            && matches!(
                parts[2].as_ref(),
                "Resources" | "MacOS" | "Frameworks" | "Helpers"
            )
    })
}

#[cfg(test)]
mod tests {
    use super::is_app_component;
    use std::path::Path;

    #[test]
    fn app_bundle_subdirectories_are_components() {
        for suffix in [
            "A.app/Contents/MacOS/codex",
            "A.app/Contents/Resources/codex",
            "A.app/Contents/Frameworks/codex",
            "A.app/Contents/Helpers/codex",
        ] {
            assert!(is_app_component(Path::new(suffix)), "{suffix}");
        }
    }

    #[test]
    fn ordinary_app_named_folder_is_not_a_component() {
        assert!(!is_app_component(Path::new("project.app/bin/codex")));
        assert!(!is_app_component(Path::new("A.app/Contents/Other/codex")));
        assert!(!is_app_component(Path::new("/usr/local/bin/codex")));
    }
}
