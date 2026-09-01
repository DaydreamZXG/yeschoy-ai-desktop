//! Read-only target resolution, deliberately separate from configuration authority.
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub const LIMIT: usize = 32;

#[derive(Debug, Clone)]
pub struct Installation {
    pub path: PathBuf,
    pub path_rank: Option<usize>,
}

#[derive(Debug, Default)]
pub struct Inventory {
    pub standalone: Vec<Installation>,
    bundled: HashSet<PathBuf>,
}

impl Inventory {
    pub fn bundled_count(&self) -> usize {
        self.bundled.len()
    }

    pub fn add(&mut self, entry: &Path, canonical: PathBuf, rank: Option<usize>, mac: bool) {
        if !entry.is_absolute() || !canonical.is_absolute() {
            return;
        }
        if mac && (is_app_component(entry) || is_app_component(&canonical)) {
            if self.bundled.len() < LIMIT {
                self.bundled.insert(canonical);
            }
            return;
        }
        if let Some(existing) = self
            .standalone
            .iter_mut()
            .find(|item| item.path == canonical)
        {
            existing.path_rank = match (existing.path_rank, rank) {
                (Some(left), Some(right)) => Some(left.min(right)),
                (left, right) => left.or(right),
            };
        } else if self.standalone.len() < LIMIT {
            self.standalone.push(Installation {
                path: canonical,
                path_rank: rank,
            });
        }
    }

    pub fn selection(&self) -> (&'static str, Option<&Installation>) {
        match self.standalone.as_slice() {
            [] => (
                if self.bundled.is_empty() {
                    "not_found"
                } else {
                    "bundled_only"
                },
                None,
            ),
            [only] => ("single_installation", Some(only)),
            many if many.len() >= LIMIT => ("unresolved", None),
            many => {
                let first_rank = many.iter().filter_map(|item| item.path_rank).min();
                if let Some(rank) = first_rank {
                    let mut matches = many.iter().filter(|item| item.path_rank == Some(rank));
                    let first = matches.next();
                    if matches.next().is_none() {
                        return ("path_precedence", first);
                    }
                }
                ("unresolved", None)
            }
        }
    }
}

fn is_app_component(path: &Path) -> bool {
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

pub fn exact_version(value: &str) -> bool {
    let mut parts = value.splitn(3, '.');
    let numeric = |part: Option<&str>| {
        part.is_some_and(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
    };
    numeric(parts.next())
        && numeric(parts.next())
        && parts
            .next()
            .is_some_and(|v| v.starts_with(|c: char| c.is_ascii_digit()))
        && value.len() <= 256
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+' | b'_'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn absolute(parts: &[&str]) -> PathBuf {
        #[cfg(windows)]
        let mut path = PathBuf::from(r"C:\");
        #[cfg(not(windows))]
        let mut path = PathBuf::from("/");
        for part in parts {
            path.push(part);
        }
        assert!(path.is_absolute());
        path
    }

    fn add(inventory: &mut Inventory, parts: &[&str], rank: Option<usize>) {
        let path = absolute(parts);
        inventory.add(&path, path.clone(), rank, true);
    }

    #[test]
    fn standalone_and_desktop_component_are_not_conflicting() {
        let mut inventory = Inventory::default();
        add(
            &mut inventory,
            &[
                "Applications",
                "ChatGPT.app",
                "Contents",
                "Resources",
                "codex",
            ],
            Some(0),
        );
        add(&mut inventory, &["users", "local", "bin", "codex"], Some(3));
        assert_eq!(inventory.bundled_count(), 1);
        assert_eq!(inventory.standalone.len(), 1);
        assert_eq!(inventory.selection().0, "single_installation");
    }
    #[test]
    fn aliases_deduplicate_and_keep_earliest_rank() {
        let mut inventory = Inventory::default();
        let canonical = absolute(&["release", "bin", "codex"]);
        inventory.add(
            &absolute(&["local", "bin", "codex"]),
            canonical.clone(),
            None,
            true,
        );
        inventory.add(
            &absolute(&["npm", "bin", "codex"]),
            canonical.clone(),
            Some(2),
            true,
        );
        inventory.add(&absolute(&["link", "codex"]), canonical, Some(1), true);
        assert_eq!(inventory.standalone.len(), 1);
        assert_eq!(inventory.selection().1.unwrap().path_rank, Some(1));
    }
    #[test]
    fn alias_into_bundle_is_excluded() {
        let mut inventory = Inventory::default();
        inventory.add(
            &absolute(&["local", "bin", "codex"]),
            absolute(&[
                "Applications",
                "ChatGPT.app",
                "Contents",
                "Resources",
                "codex",
            ]),
            Some(0),
            true,
        );
        assert_eq!(inventory.selection().0, "bundled_only");
        assert_eq!(inventory.bundled_count(), 1);
    }
    #[test]
    fn unique_path_priority_not_version_or_alphabetical_order() {
        let mut inventory = Inventory::default();
        add(&mut inventory, &["a", "new", "codex"], Some(5));
        add(&mut inventory, &["z", "old", "codex"], Some(1));
        add(&mut inventory, &["common", "codex"], None);
        assert_eq!(inventory.selection().0, "path_precedence");
        assert_eq!(
            inventory.selection().1.unwrap().path,
            absolute(&["z", "old", "codex"])
        );
    }
    #[test]
    fn same_rank_and_common_only_ambiguity_are_not_guessed() {
        for rank in [Some(0), None] {
            let mut inventory = Inventory::default();
            add(&mut inventory, &["bin", "codex.exe"], rank);
            add(&mut inventory, &["bin", "codex.cmd"], rank);
            assert_eq!(inventory.selection().0, "unresolved");
            assert!(inventory.selection().1.is_none());
        }
    }
    #[test]
    fn relative_and_empty_paths_never_select_working_directory() {
        let mut inventory = Inventory::default();
        inventory.add(
            Path::new("bin/codex"),
            absolute(&["tmp", "project", "bin", "codex"]),
            Some(0),
            true,
        );
        inventory.add(
            Path::new(""),
            absolute(&["tmp", "project", "codex"]),
            Some(0),
            true,
        );
        assert_eq!(inventory.selection().0, "not_found");
    }
    #[test]
    fn bound_fails_closed_and_bundle_limit_does_not_hide_standalone() {
        let mut inventory = Inventory::default();
        for n in 0..40 {
            let app = format!("A{n}.app");
            add(
                &mut inventory,
                &[&app, "Contents", "MacOS", "codex"],
                Some(n),
            );
        }
        add(&mut inventory, &["local", "codex"], None);
        assert_eq!(inventory.bundled_count(), LIMIT);
        assert_eq!(inventory.selection().0, "single_installation");
        for n in 0..40 {
            let item = n.to_string();
            add(&mut inventory, &["bin", &item, "codex"], Some(n));
        }
        assert_eq!(inventory.standalone.len(), LIMIT);
        assert_eq!(inventory.selection().0, "unresolved");
    }
    #[test]
    fn ordinary_app_named_folder_is_not_a_bundle_component() {
        let mut inventory = Inventory::default();
        add(&mut inventory, &["project.app", "bin", "codex"], None);
        assert_eq!(inventory.selection().0, "single_installation");
        let mut non_mac = Inventory::default();
        let path = absolute(&["A.app", "Contents", "MacOS", "codex"]);
        non_mac.add(&path, path.clone(), None, false);
        assert_eq!(non_mac.selection().0, "single_installation");
    }
    #[test]
    fn version_never_returns_raw_diagnostics_paths_or_secrets() {
        for valid in ["0.146.0", "1.2.3-alpha.2", "1.2.3+build_5"] {
            assert!(exact_version(valid));
        }
        for invalid in [
            "",
            "error",
            "/Users/person/key",
            "token secret",
            "1.2",
            "v1.2.3",
            "1.2.3\nsecret",
        ] {
            assert!(!exact_version(invalid));
        }
    }
}
