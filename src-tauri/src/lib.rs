mod connectivity;
mod connectivity_core;
mod tool_discovery;
mod tool_discovery_core;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            tool_discovery::scan_tools_read_only,
            connectivity::check_line_connectivity_read_only
        ])
        .run(tauri::generate_context!())
        .expect("failed to run the 野菜API desktop shell");
}
