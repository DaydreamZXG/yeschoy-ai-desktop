// Prevents an additional console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Some(code) = yeschoy_desktop_lib::credential_helper_exit_code() {
        std::process::exit(code);
    }
    yeschoy_desktop_lib::run();
}
