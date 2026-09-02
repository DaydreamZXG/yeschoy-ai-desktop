mod account_v2;
mod connectivity;
mod connectivity_core;
mod desktop_app_discovery;
mod desktop_app_discovery_core;
mod service_catalog;
mod service_catalog_core;
mod tool_activation;
mod tool_discovery;
mod tool_discovery_core;
mod tool_discovery_v2;
mod tool_selection_core;
mod window_appearance;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(account_v2::AccountV2State::default())
        .setup(|app| {
            window_appearance::initialize(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            desktop_app_discovery::scan_desktop_apps_read_only,
            account_v2::account_inspect_v2,
            account_v2::account_begin_authorization_v2,
            account_v2::account_poll_authorization_v2,
            account_v2::account_cancel_authorization_v2,
            account_v2::account_logout_v2,
            account_v2::account_open_wallet_v2,
            tool_activation::configure_desktop_tool_v1,
            tool_discovery::scan_tools_read_only,
            tool_discovery_v2::scan_tools_read_only_v2,
            window_appearance::set_window_appearance,
            connectivity::check_line_connectivity_read_only,
            service_catalog::read_public_service_catalog
        ])
        .run(tauri::generate_context!())
        .expect("failed to run the 野菜API desktop shell");
}
