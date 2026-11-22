//! 打开应用时的衔接：应用要被打开的那一刻拉一块暗场，角色在上面登场；应用的窗口
//! 摆好了，它的实时画面先出现在暗场里的一块屏幕上，再放大到应用实际所在的位置，
//! 暗场撤掉，手上就是那个应用。
//!
//! 界面先说一声「接下来要打开的这个应用，由谁登场」（`arm`）；真到了叫应用打开
//! 的那一步才拉暗场，所以接入中途停下来问话、没接成的时候不会闪一下黑。
//!
//! 这里只管编排。别人的窗口只看、不挪、不改。任何一步不顺（没找到窗口、系统不给
//! 画面、应用在另一块显示器上、超时）都只是提前撤掉暗场，应用照常打开。
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, MutexGuard, OnceLock,
    },
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::{
    app_handoff_core::{between, slot, Area, Seen, Settle},
    tool_adapters::desktop_lifecycle,
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

/// 说了「由谁登场」之后这么久还没轮到打开应用，这句话就作废。
const ARMED_FOR: Duration = Duration::from_secs(45);
/// 暗场最多留这么久，到点无论如何撤掉：它盖着整块屏幕，不能卡在那儿。
const HARD_LIMIT: Duration = Duration::from_secs(12);
/// 应用叫开之后，最多等它的窗口这么久。
const WINDOW_WAIT: Duration = Duration::from_secs(8);
/// 暗场那一页画出来之前，最多让应用晚这么久再开：先开的话它会在暗场前面弹一下。
const PAINT_WAIT: Duration = Duration::from_millis(600);
const LOOK_EVERY: Duration = Duration::from_millis(100);
const FADE_IN: Duration = Duration::from_millis(260);
const HOLD: Duration = Duration::from_millis(650);
const GROW: Duration = Duration::from_millis(720);
/// 暗场那一页淡出要的时间；等它淡完再关窗口。
const LEAVE: Duration = Duration::from_millis(260);
const FRAME: Duration = Duration::from_millis(8);

static APP: OnceLock<AppHandle> = OnceLock::new();
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
/// 暗场那一页画好了的是哪一场。
static PAINTED: AtomicU64 = AtomicU64::new(0);

struct Armed {
    tool_id: String,
    who: String,
    at: Instant,
}

struct Session {
    id: u64,
    path: PathBuf,
}

static ARMED: Mutex<Option<Armed>> = Mutex::new(None);
static SESSION: Mutex<Option<Session>> = Mutex::new(None);

fn held<T>(lock: &'static Mutex<T>) -> MutexGuard<'static, T> {
    lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(crate) fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

fn is_current(id: u64) -> bool {
    held(&SESSION).as_ref().is_some_and(|now| now.id == id)
}

/// 只有一个人能收场：拿到的那个负责撤暗场，别人看到已经没了就收手。
fn claim(id: Option<u64>) -> Option<Session> {
    let mut guard = held(&SESSION);
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

/// 这次打开该由谁登场：登记过、是这个应用、还没过期。问一次就用掉。
fn take_armed(slot: &mut Option<Armed>, tool_id: &str, now: Instant) -> Option<String> {
    let armed = slot.take()?;
    (armed.tool_id == tool_id && now.duration_since(armed.at) <= ARMED_FOR).then_some(armed.who)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArmRequest {
    tool_id: String,
    who: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArmResponse {
    /// `armed`：接下来打开这个应用时会有衔接。`unavailable`：没有，照原样打开。
    status: &'static str,
}

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

/// 界面先说一声：接下来要打开的这个应用，由谁登场。只登记，不开窗口。
#[tauri::command]
pub async fn app_handoff_arm_v1(request: ArmRequest) -> Result<ArmResponse, String> {
    let ok = has_handoff(&request.tool_id) && who_is_plain(&request.who);
    *held(&ARMED) = ok.then(|| Armed {
        tool_id: request.tool_id,
        who: request.who,
        at: Instant::now(),
    });
    Ok(ArmResponse {
        status: if ok { "armed" } else { "unavailable" },
    })
}

/// 那次接入或打开结束了：没用上的登记清掉，免得挂到下一次打开上。
#[tauri::command]
pub async fn app_handoff_disarm_v1() -> Result<(), String> {
    *held(&ARMED) = None;
    Ok(())
}

/// 暗场那一页说：我画好了。
#[tauri::command]
pub async fn app_handoff_ready_v1(window: tauri::WebviewWindow) -> Result<(), String> {
    if window.label() == WINDOW_LABEL {
        if let Some(now) = held(&SESSION).as_ref() {
            PAINTED.store(now.id, Ordering::Release);
        }
    }
    Ok(())
}

/// 用户点了跳过：马上撤场，应用已经在后面的话把它提上来。
#[tauri::command]
pub async fn app_handoff_end_v1(app: AppHandle) -> Result<(), String> {
    let path = held(&SESSION).as_ref().map(|now| now.path.clone());
    let seen = match path {
        Some(path) => look(path).await,
        None => None,
    };
    leave(&app, None, LEAVE, seen).await;
    Ok(())
}

/// 一次打开对应的那块暗场。没拉起来的时候是空的，后面的调用什么都不做。
#[must_use]
pub(crate) struct Curtain(Option<u64>);

/// 叫应用打开之前调：登记过由谁登场，就先把暗场拉起来，等它画出来（有上限）。
pub(crate) fn raise(tool_id: &str, path: &Path) -> Curtain {
    let Some(who) = take_armed(&mut held(&ARMED), tool_id, Instant::now()) else {
        return Curtain(None);
    };
    let Some(app) = APP.get() else {
        return Curtain(None);
    };
    let Some(stage) = stage_of_main(app) else {
        return Curtain(None);
    };
    // 上一场还没撤干净就先撤掉，不叠两块暗场。
    claim(None);
    if let Some(old) = app.get_webview_window(WINDOW_LABEL) {
        let _ = old.destroy();
    }
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    *held(&SESSION) = Some(Session {
        id,
        path: path.to_owned(),
    });
    let url = format!("index.html?handoff={tool_id}&who={who}");
    let builder =
        tauri::WebviewWindowBuilder::new(app, WINDOW_LABEL, tauri::WebviewUrl::App(url.into()))
            .title("YesChoy")
            .decorations(false)
            .shadow(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(false)
            .focused(true)
            .visible(false);
    // macOS 上透明窗口要另开私有接口；那边本来也没有衔接。
    #[cfg(not(target_os = "macos"))]
    let builder = builder.transparent(true);
    let shown = builder.build().ok().is_some_and(|window| {
        window
            .set_position(tauri::PhysicalPosition::new(stage.x, stage.y))
            .and_then(|()| {
                window.set_size(tauri::PhysicalSize::new(stage.w as u32, stage.h as u32))
            })
            .and_then(|()| window.show())
            .is_ok()
    });
    if !shown {
        claim(Some(id));
        if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
            let _ = window.destroy();
        }
        return Curtain(None);
    }
    let watchdog = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(HARD_LIMIT).await;
        leave(&watchdog, Some(id), LEAVE, None).await;
    });
    let began = Instant::now();
    while PAINTED.load(Ordering::Acquire) != id && began.elapsed() < PAINT_WAIT {
        std::thread::sleep(Duration::from_millis(20));
    }
    Curtain(Some(id))
}

impl Curtain {
    /// 应用叫开了没有。叫开了就等它的窗口、演衔接；没叫开就撤场。
    pub(crate) fn after(self, launched: bool) {
        let (Some(id), Some(app)) = (self.0, APP.get().cloned()) else {
            return;
        };
        tauri::async_runtime::spawn(async move {
            let seen = if launched {
                present(&app, id).await
            } else {
                None
            };
            leave(&app, Some(id), LEAVE, seen).await;
        });
    }
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
async fn present(app: &AppHandle, id: u64) -> Option<Seen> {
    let path = held(&SESSION)
        .as_ref()
        .filter(|now| now.id == id)?
        .path
        .clone();
    let deadline = Instant::now() + WINDOW_WAIT;
    let mut settle = Settle::default();
    let seen = loop {
        if !is_current(id) || Instant::now() >= deadline {
            return None;
        }
        let seen = look(path.clone()).await;
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
    let held_since = Instant::now();
    while is_current(id) && held_since.elapsed() < HOLD {
        tokio::time::sleep(FRAME * 4).await;
    }
    let _ = app.emit_to(WINDOW_LABEL, LAND_EVENT, ());
    let began = Instant::now();
    while is_current(id) {
        let progress = began.elapsed().as_secs_f64() / GROW.as_secs_f64();
        if !picture.show(between(from, to, progress), 255) || progress >= 1.0 {
            break;
        }
        tokio::time::sleep(FRAME).await;
    }
    // 画面留到暗场淡完：它正好盖在真窗口上，撤的那一下看不出来。
    if is_current(id) {
        let _ = app.emit_to(WINDOW_LABEL, LEAVE_EVENT, ());
        tokio::time::sleep(LEAVE).await;
    }
    Some(seen)
}

/// 撤暗场：先让那一页淡出，把应用提到前面，再关窗口。`id` 给了就只撤那一场。
async fn leave(app: &AppHandle, id: Option<u64>, fade: Duration, seen: Option<Seen>) {
    if claim(id).is_none() {
        return;
    }
    let window = app.get_webview_window(WINDOW_LABEL);
    if window.is_some() && !fade.is_zero() {
        let _ = app.emit_to(WINDOW_LABEL, LEAVE_EVENT, ());
        tokio::time::sleep(fade).await;
    }
    // 趁暗场还在前台时提：窗口一关，系统就不再让我们把别人的窗口提到前面。
    if let Some(seen) = seen {
        sys::bring_forward(seen.handle);
    }
    // `close` 会走「要不要退出野菜」那一套；暗场不是用户开的窗口，直接销毁。
    if let Some(window) = window {
        let _ = window.destroy();
    }
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
        assert_eq!(has_handoff("codex_desktop"), cfg!(target_os = "windows"));
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

    fn armed(tool_id: &str, at: Instant) -> Option<Armed> {
        Some(Armed {
            tool_id: tool_id.into(),
            who: "ds".into(),
            at,
        })
    }

    #[test]
    fn an_arm_is_used_once_by_the_app_it_named_and_expires() {
        let now = Instant::now();
        let mut slot = armed("codex_desktop", now);
        assert_eq!(
            take_armed(&mut slot, "codex_desktop", now).as_deref(),
            Some("ds")
        );
        assert_eq!(take_armed(&mut slot, "codex_desktop", now), None);

        // 别的应用先打开：不借用这次登记，登记也就此作废。
        let mut slot = armed("codex_desktop", now);
        assert_eq!(take_armed(&mut slot, "claude_desktop", now), None);
        assert_eq!(take_armed(&mut slot, "codex_desktop", now), None);

        let mut slot = armed("codex_desktop", now);
        let late = now + ARMED_FOR + Duration::from_secs(1);
        assert_eq!(take_armed(&mut slot, "codex_desktop", late), None);
    }

    #[test]
    fn without_an_arm_opening_an_app_raises_nothing() {
        let curtain = raise("codex_desktop", Path::new("/nowhere/Codex.exe"));
        assert!(curtain.0.is_none());
        curtain.after(true);
    }

    #[test]
    fn screen_rectangles_reach_the_page_in_css_pixels() {
        assert_eq!(
            css(Area::new(150, 300, 1200, 750), 1.5),
            [100.0, 200.0, 800.0, 500.0]
        );
    }
}
