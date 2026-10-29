//! Where this user's home directory is.
//!
//! Every adapter and every recovery path needs it, and none of them should
//! depend on `tool_adapters` to get it. Windows needs the `USERPROFILE` /
//! `HOMEDRIVE`+`HOMEPATH` fallback; that detail also does not belong to any one
//! tool.
use std::path::PathBuf;

pub(crate) fn user_home() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .or_else(|| {
                let drive = std::env::var_os("HOMEDRIVE")?;
                let path = std::env::var_os("HOMEPATH")?;
                let mut home = PathBuf::from(drive);
                home.push(path);
                Some(home)
            })
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}
