use serde::{Deserialize, Serialize};
use tauri::{window::Color, Manager, Theme, Window};

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Palette {
    Light,
    Dark,
}

impl Palette {
    fn color(self) -> Color {
        match self {
            Self::Light => Color(244, 246, 250, 255),
            Self::Dark => Color(17, 28, 44, 255),
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AppearanceRequest {
    theme: Palette,
}

pub fn initialize(app: &tauri::App) -> tauri::Result<()> {
    if let Some(webview) = app.get_webview_window("main") {
        let window = webview.as_ref().window();
        let palette = match window.theme()? {
            Theme::Dark => Palette::Dark,
            _ => Palette::Light,
        };
        window.set_background_color(Some(palette.color()))?;
    }
    Ok(())
}

/// Only paints native chrome. Never overrides NSApplication/OS appearance:
/// matchMedia must keep observing the real system preference in System mode.
#[tauri::command]
pub async fn set_window_appearance(
    window: Window,
    request: AppearanceRequest,
) -> Result<AppearanceRequest, String> {
    if window.label() != "main" {
        return Err("window_not_allowed".into());
    }
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let target = window.clone();
    window
        .run_on_main_thread(move || {
            let result = target
                .set_background_color(Some(request.theme.color()))
                .map(|_| request)
                .map_err(|_| "appearance_unavailable".to_string());
            let _ = sender.send(result);
        })
        .map_err(|_| "appearance_unavailable".to_string())?;
    receiver
        .await
        .map_err(|_| "appearance_unavailable".to_string())?
}
