//! 系统那一层的外壳：macOS 顶部菜单栏、Windows 托盘图标、Windows 原生关闭提示的按钮文字。
//! 文字都跟着界面语言（`ui_language`）走。

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Runtime};

use crate::ui_language::{self, Language};

const TRAY_ID: &str = "main";
const TRAY_OPEN: &str = "tray-open";
const TRAY_QUIT: &str = "tray-quit";

/// Tauri 自带的 macOS 菜单只有英文（File / Edit / View）。这里照着系统惯例自己搭一份。
/// 编辑菜单必须在：没有它，macOS 上 ⌘C / ⌘V 在输入框里不起作用。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn app_menu<R: Runtime>(app: &AppHandle<R>, language: Language) -> tauri::Result<Menu<R>> {
    let t = |zh: &'static str, en: &'static str| Some(language.pick(zh, en));
    let name = app.package_info().name.clone();
    let about = format!("{} {name}", language.pick("关于", "About"));
    let hide = format!("{} {name}", language.pick("隐藏", "Hide"));
    let quit = format!("{} {name}", language.pick("退出", "Quit"));
    let application = Submenu::with_items(
        app,
        &name,
        true,
        &[
            &PredefinedMenuItem::about(app, Some(&about), None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::services(app, t("服务", "Services"))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, Some(&hide))?,
            &PredefinedMenuItem::hide_others(app, t("隐藏其他", "Hide Others"))?,
            &PredefinedMenuItem::show_all(app, t("全部显示", "Show All"))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::quit(app, Some(&quit))?,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        language.pick("编辑", "Edit"),
        true,
        &[
            &PredefinedMenuItem::undo(app, t("撤销", "Undo"))?,
            &PredefinedMenuItem::redo(app, t("重做", "Redo"))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, t("剪切", "Cut"))?,
            &PredefinedMenuItem::copy(app, t("拷贝", "Copy"))?,
            &PredefinedMenuItem::paste(app, t("粘贴", "Paste"))?,
            &PredefinedMenuItem::select_all(app, t("全选", "Select All"))?,
        ],
    )?;
    let window = Submenu::with_items(
        app,
        language.pick("窗口", "Window"),
        true,
        &[
            &PredefinedMenuItem::minimize(app, t("最小化", "Minimize"))?,
            &PredefinedMenuItem::maximize(app, t("缩放", "Zoom"))?,
            &PredefinedMenuItem::fullscreen(app, t("进入全屏幕", "Enter Full Screen"))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::close_window(app, t("关闭窗口", "Close Window"))?,
        ],
    )?;
    Menu::with_items(app, &[&application, &edit, &window])
}

/// 启动时和界面换语言时各调一次。
pub(crate) fn refresh_menu<R: Runtime>(app: &AppHandle<R>) {
    #[cfg(target_os = "macos")]
    match app_menu(app, ui_language::current()) {
        Ok(menu) => {
            if let Err(error) = app.set_menu(menu) {
                log::warn!("native_shell stage=set_menu_failed error={error}");
            }
        }
        Err(error) => log::warn!("native_shell stage=build_menu_failed error={error}"),
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

/// 「后台运行」在 Windows 上把窗口藏进托盘：任务栏里不占位置，托盘里点一下就回来。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn show_tray<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    // 托盘只建一次，之后只切换显示。这里可能正跑在托盘自己的菜单回调里（退出… → 更多 → 后台），
    // Tauri 那时还攥着菜单回调表的锁；再 build 一个带 on_menu_event 的托盘会去抢同一把锁，界面就卡死。
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        return tray.set_visible(true);
    }
    let language = ui_language::current();
    let open = MenuItem::with_id(
        app,
        TRAY_OPEN,
        language.pick("打开 YesChoy", "Open YesChoy"),
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(
        app,
        TRAY_QUIT,
        language.pick("退出…", "Quit…"),
        true,
        None::<&str>,
    )?;
    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("YesChoy")
        .menu(&Menu::with_items(app, &[&open, &quit])?)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            TRAY_OPEN => crate::focus_main_webview(app),
            TRAY_QUIT => {
                crate::focus_main_webview(app);
                crate::ask_how_to_close(app);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                crate::focus_main_webview(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

pub(crate) fn hide_tray<R: Runtime>(app: &AppHandle<R>) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_visible(false);
    }
}

/// 系统提示框的按钮文字跟着系统语言走：英文 Windows 上正文写「是/否」，按钮却是 Yes/No。
/// 在提示框出现的那一刻把三个按钮改成我们自己的字，正文就能照着按钮写。
#[cfg(target_os = "windows")]
pub(crate) mod labeled_message_box {
    use std::cell::RefCell;
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, MessageBoxW, SetDlgItemTextW, SetWindowsHookExW, UnhookWindowsHookEx,
        HCBT_ACTIVATE, IDCANCEL, IDNO, IDYES, MB_ICONQUESTION, MB_SETFOREGROUND, MB_YESNOCANCEL,
        MESSAGEBOX_RESULT, WH_CBT,
    };

    thread_local! {
        static LABELS: RefCell<Option<[HSTRING; 3]>> = const { RefCell::new(None) };
    }

    unsafe extern "system" fn relabel(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code == HCBT_ACTIVATE as i32 {
            let dialog = HWND(wparam.0 as *mut _);
            if let Some(labels) = LABELS.with(|labels| labels.borrow_mut().take()) {
                for (id, label) in [IDYES, IDNO, IDCANCEL].into_iter().zip(labels.iter()) {
                    // SAFETY: `dialog` is the message box being activated on this thread.
                    let _ = unsafe { SetDlgItemTextW(dialog, id.0, label) };
                }
            }
        }
        // SAFETY: forwarding the hook call unchanged.
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }

    /// 是 / 否 / 取消 三个按钮依次换成 `labels`。
    pub(crate) fn ask(
        owner: Option<HWND>,
        title: &str,
        body: &str,
        labels: [&str; 3],
    ) -> MESSAGEBOX_RESULT {
        LABELS.with(|slot| *slot.borrow_mut() = Some(labels.map(HSTRING::from)));
        // SAFETY: a thread-local CBT hook, removed right after the modal call returns.
        let hook =
            unsafe { SetWindowsHookExW(WH_CBT, Some(relabel), None, GetCurrentThreadId()) }.ok();
        // SAFETY: plain modal message box owned by our window.
        let result = unsafe {
            MessageBoxW(
                owner,
                &HSTRING::from(body),
                &HSTRING::from(title),
                MB_YESNOCANCEL | MB_ICONQUESTION | MB_SETFOREGROUND,
            )
        };
        if let Some(hook) = hook {
            // SAFETY: `hook` was installed above and is still ours.
            let _ = unsafe { UnhookWindowsHookEx(hook) };
        }
        LABELS.with(|slot| slot.borrow_mut().take());
        result
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn menu_and_tray_builders_type_check() {
        // 真正的菜单要在 macOS、托盘要在 Windows 上看；这里只保证两边的代码都编得过。
        let _ = super::app_menu::<tauri::Wry>;
        let _ = super::show_tray::<tauri::Wry>;
    }
}
