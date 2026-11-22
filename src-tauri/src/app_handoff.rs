//! 打开应用时的衔接：点「打开」后拉一块暗场，角色在上面登场；应用的窗口摆好了，
//! 它的实时画面先出现在暗场里的一块屏幕上，再放大到应用实际所在的位置，暗场撤掉，
//! 手上就是那个应用。
//!
//! 这里只管编排。别人的窗口只看、不挪、不改。任何一步不顺（没找到窗口、系统不给
//! 画面、应用在另一块显示器上、超时）都只是提前撤掉暗场，应用照常打开。
use std::{
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::{
    app_handoff_core::{between, slot, Area, Seen, Settle},
    tool_adapters::{self, codex_desktop, desktop_lifecycle},
};

#[cfg(target_os = "windows")]
use crate::app_handoff_windows as sys;

/// 做不了实时画面的系统上，同样的几个入口，全都答「没有」。
#[cfg(not(target_os = "windows"))]
mod sys {
    use super::{Area, Seen};

    pub(crate) fn pick(_handles: &[isize]) -> Option<Seen> {
        None
    }
    pub(crate) fn inside_origin(_own: isize) -> Option<(i32, i32)> {
        None
    }
    pub(crate) struct Picture;
    impl Picture {
        pub(crate) fn of(_own: isize, _source: Seen) -> Option<Self> {
            None
        }
        pub(crate) fn show(&self, _place: Area, _opacity: u8) -> bool {
            false
        }
    }
    pub(crate) fn bring_forward(_handle: isize) {}
}

pub(crate) const WINDOW_LABEL: &str = "handoff";

/// 画面里的事告诉暗场那一页：屏幕框画在哪、开始放大、撤场。
const SCREEN_EVENT: &str = "handoff://screen";
const LAND_EVENT: &str = "handoff://land";
const LEAVE_EVENT: &str = "handoff://leave";

/// 暗场最多留这么久，到点无论如何撤掉：它盖着整块屏幕，不能卡在那儿。
const HARD_LIMIT: Duration = Duration::from_secs(12);
/// 应用打开之后，最多等它的窗口这么久。
const WINDOW_WAIT: Duration = Duration::from_secs(8);
const LOOK_EVERY: Duration = Duration::from_millis(100);
const FADE_IN: Duration = Duration::from_millis(260);
const HOLD: Duration = Duration::from_millis(650);
const GROW: Duration = Duration::from_millis(720);
/// 暗场那一页淡出要的时间；等它淡完再关窗口。
const LEAVE: Duration = Duration::from_millis(260);
const FRAME: Duration = Duration::from_millis(8);

struct Session {
    id: u64,
    path: PathBuf,
    following: bool,
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);
static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn session() -> std::sync::MutexGuard<'static, Option<Session>> {
    SESSION
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn is_current(id: u64) -> bool {
    session().as_ref().is_some_and(|now| now.id == id)
}

/// 只有一个人能收场：拿到的那个负责撤暗场，别人看到已经没了就收手。
fn claim(id: Option<u64>) -> Option<Session> {
    let mut guard = session();
    match (guard.as_ref(), id) {
        (Some(now), Some(id)) if now.id != id => None,
        _ => guard.take(),
    }
}

/// 哪些应用有衔接。别的照原样直接打开。
fn has_handoff(tool_id: &str) -> bool {
    cfg!(target_os = "windows") && tool_id == "codex_desktop"
}

/// 角色的代号会拼进暗场那一页的地址里，只收短的小写字母数字。
fn who_is_plain(who: &str) -> bool {
    (1..=16).contains(&who.len())
        && who
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BeginRequest {
    tool_id: String,
    who: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoffResponse {
    /// `started`：暗场拉起来了。`unavailable`：这次没有衔接，照原样打开。
    status: &'static str,
}

const UNAVAILABLE: HandoffResponse = HandoffResponse {
    status: "unavailable",
};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScreenEvent {
    /// 屏幕先出现在哪，再放大到哪；都是暗场那一页里的 CSS 像素。
    from: [f64; 4],
    to: [f64; 4],
    fade_ms: u64,
    hold_ms: u64,
    grow_ms: u64,
}

fn css(area: Area, scale: f64) -> [f64; 4] {
    [
        f64::from(area.x) / scale,
        f64::from(area.y) / scale,
        f64::from(area.w) / scale,
        f64::from(area.h) / scale,
    ]
}

/// 拉起暗场。要在叫应用打开之前调：应用的窗口弹出来时暗场已经盖在上面了。
#[tauri::command]
pub async fn app_handoff_begin_v1(
    app: AppHandle,
    request: BeginRequest,
) -> Result<HandoffResponse, String> {
    if !has_handoff(&request.tool_id) || !who_is_plain(&request.who) {
        return Ok(UNAVAILABLE);
    }
    let Ok(installation) = tool_adapters::resolve_preferred_installation(&request.tool_id).await
    else {
        return Ok(UNAVAILABLE);
    };
    // 命令行版开的是终端，没有可以衔接的应用窗口。
    if codex_desktop::is_cli_installation(&installation.path) {
        return Ok(UNAVAILABLE);
    }
    let Some(stage) = stage_of_main(&app) else {
        return Ok(UNAVAILABLE);
    };
    // 上一场还没撤干净就先撤掉，不叠两块暗场。
    if let Some(old) = app.get_webview_window(WINDOW_LABEL) {
        claim(None);
        let _ = old.destroy();
    }
    let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    *session() = Some(Session {
        id,
        path: installation.path,
        following: false,
    });
    let url = format!("index.html?handoff={}&who={}", request.tool_id, request.who);
    let built =
        tauri::WebviewWindowBuilder::new(&app, WINDOW_LABEL, tauri::WebviewUrl::App(url.into()))
            .title("YesChoy")
            .decorations(false)
            .transparent(true)
            .shadow(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(false)
            .focused(true)
            .visible(false)
            .build();
    let placed = built.ok().filter(|window| {
        window
            .set_position(tauri::PhysicalPosition::new(stage.x, stage.y))
            .and_then(|_| window.set_size(tauri::PhysicalSize::new(stage.w as u32, stage.h as u32)))
            .and_then(|_| window.show())
            .is_ok()
    });
    if placed.is_none() {
        leave(&app, Some(id), Duration::ZERO).await;
        return Ok(UNAVAILABLE);
    }
    let watchdog = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(HARD_LIMIT).await;
        leave(&watchdog, Some(id), LEAVE).await;
    });
    Ok(HandoffResponse { status: "started" })
}

/// 应用已经叫开了：开始等它的窗口，等到了就演衔接。
#[tauri::command]
pub async fn app_handoff_follow_v1(app: AppHandle) -> Result<(), String> {
    let started = {
        let mut guard = session();
        guard.as_mut().filter(|now| !now.following).map(|now| {
            now.following = true;
            (now.id, now.path.clone())
        })
    };
    if let Some((id, path)) = started {
        tauri::async_runtime::spawn(async move {
            let seen = present(&app, id, &path).await;
            leave(&app, Some(id), LEAVE).await;
            if let Some(seen) = seen {
                sys::bring_forward(seen.handle);
            }
        });
    }
    Ok(())
}

/// 提前收场：没打开成，或者用户点了跳过。
#[tauri::command]
pub async fn app_handoff_end_v1(app: AppHandle) -> Result<(), String> {
    let path = session().as_ref().map(|now| now.path.clone());
    leave(&app, None, LEAVE).await;
    // 跳过的时候应用可能已经在后面了，把它提上来。
    if let Some(path) = path {
        if let Some(seen) = look(path).await {
            sys::bring_forward(seen.handle);
        }
    }
    Ok(())
}

/// 野菜的窗口现在在哪块显示器上，暗场就盖住那一块。
fn stage_of_main(app: &AppHandle) -> Option<Area> {
    let monitor = app
        .get_webview_window("main")?
        .current_monitor()
        .ok()
        .flatten()?;
    let (position, size) = (monitor.position(), monitor.size());
    let area = Area::new(
        position.x,
        position.y,
        i32::try_from(size.width).ok()?,
        i32::try_from(size.height).ok()?,
    );
    (area.w > 0 && area.h > 0).then_some(area)
}

async fn look(path: PathBuf) -> Option<Seen> {
    tokio::task::spawn_blocking(move || sys::pick(&desktop_lifecycle::window_handles(&path)))
        .await
        .ok()
        .flatten()
}

#[cfg(target_os = "windows")]
fn own_handle(window: &tauri::WebviewWindow) -> Option<isize> {
    window.hwnd().ok().map(|hwnd| hwnd.0 as isize)
}

#[cfg(not(target_os = "windows"))]
fn own_handle(_window: &tauri::WebviewWindow) -> Option<isize> {
    None
}

/// 等应用的窗口摆好，把它的画面演到位。返回那个窗口；没演成也照样返回找到的窗口，
/// 好让收场的人把它提到前面。
async fn present(app: &AppHandle, id: u64, path: &std::path::Path) -> Option<Seen> {
    let deadline = Instant::now() + WINDOW_WAIT;
    let mut settle = Settle::default();
    let seen = loop {
        if !is_current(id) || Instant::now() >= deadline {
            return None;
        }
        let seen = look(path.to_owned()).await;
        if settle.observe(seen.map(|seen| seen.visible)).is_some() {
            break seen?;
        }
        tokio::time::sleep(LOOK_EVERY).await;
    };
    let window = app.get_webview_window(WINDOW_LABEL)?;
    let own = own_handle(&window)?;
    let (left, top) = sys::inside_origin(own)?;
    let size = window.inner_size().ok()?;
    let stage = Area::new(
        left,
        top,
        i32::try_from(size.width).ok()?,
        i32::try_from(size.height).ok()?,
    );
    // 应用开在另一块显示器上、或者比这块屏幕还大：不演画面，直接撤暗场。
    if !stage.holds(seen.visible) {
        return Some(seen);
    }
    let Some(picture) = sys::Picture::of(own, seen) else {
        return Some(seen);
    };
    let from = slot(stage, seen.visible).within(stage);
    let to = seen.visible.within(stage);
    let scale = window.scale_factor().unwrap_or(1.0).max(0.1);
    let _ = app.emit_to(
        WINDOW_LABEL,
        SCREEN_EVENT,
        ScreenEvent {
            from: css(from, scale),
            to: css(to, scale),
            fade_ms: FADE_IN.as_millis() as u64,
            hold_ms: HOLD.as_millis() as u64,
            grow_ms: GROW.as_millis() as u64,
        },
    );
    let began = Instant::now();
    while is_current(id) {
        let progress = began.elapsed().as_secs_f64() / FADE_IN.as_secs_f64();
        if !picture.show(from, (progress.min(1.0) * 255.0) as u8) {
            return Some(seen);
        }
        if progress >= 1.0 {
            break;
        }
        tokio::time::sleep(FRAME).await;
    }
    let held = Instant::now();
    while is_current(id) && held.elapsed() < HOLD {
        tokio::time::sleep(FRAME * 4).await;
    }
    let _ = app.emit_to(WINDOW_LABEL, LAND_EVENT, ());
    let began = Instant::now();
    while is_current(id) {
        let progress = began.elapsed().as_secs_f64() / GROW.as_secs_f64();
        if !picture.show(between(from, to, progress), 255) {
            break;
        }
        if progress >= 1.0 {
            break;
        }
        tokio::time::sleep(FRAME).await;
    }
    // 画面留到暗场淡完：它正好盖在真窗口上，撤的那一下看不出来。
    if is_current(id) {
        let _ = app.emit_to(WINDOW_LABEL, LEAVE_EVENT, ());
        tokio::time::sleep(LEAVE).await;
    }
    drop(picture);
    Some(seen)
}

/// 撤暗场：先让那一页淡出，再关窗口。`id` 给了就只撤那一场。
async fn leave(app: &AppHandle, id: Option<u64>, fade: Duration) {
    if claim(id).is_none() {
        return;
    }
    let Some(window) = app.get_webview_window(WINDOW_LABEL) else {
        return;
    };
    if !fade.is_zero() {
        let _ = app.emit_to(WINDOW_LABEL, LEAVE_EVENT, ());
        tokio::time::sleep(fade).await;
    }
    // `close` 会走「要不要退出野菜」那一套；暗场不是用户开的窗口，直接销毁。
    let _ = window.destroy();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_character_ids_reach_the_page_address() {
        for good in ["ds", "gpt", "qw", "k2"] {
            assert!(who_is_plain(good), "{good}");
        }
        for bad in [
            "",
            "DS",
            "a&b=c",
            "a b",
            "../x",
            "abcdefghijklmnopq",
            "角色",
        ] {
            assert!(!who_is_plain(bad), "{bad}");
        }
    }

    #[test]
    fn only_listed_desktop_apps_get_a_handoff() {
        assert_eq!(has_handoff("codex_desktop"), cfg!(target_os = "windows"),);
        for other in [
            "claude_code",
            "opencode",
            "dsh_desktop",
            "claude_desktop",
            "",
        ] {
            assert!(!has_handoff(other), "{other}");
        }
    }

    #[test]
    fn screen_rectangles_reach_the_page_in_css_pixels() {
        assert_eq!(
            css(Area::new(150, 300, 1200, 750), 1.5),
            [100.0, 200.0, 800.0, 500.0]
        );
    }
}
