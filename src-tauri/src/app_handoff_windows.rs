//! 打开应用时的衔接，Windows 上碰系统的那一半：应用的窗口现在在哪、把它的
//! 实时画面贴进我们的舞台、演完把它提到最前面。
//!
//! 只读别人窗口的位置和状态，不挪、不改、不藏别人的窗口。

use windows::{
    core::BOOL,
    Win32::{
        Foundation::{HWND, POINT, RECT},
        Graphics::{
            Dwm::{
                DwmGetWindowAttribute, DwmQueryThumbnailSourceSize, DwmRegisterThumbnail,
                DwmUnregisterThumbnail, DwmUpdateThumbnailProperties, DWMWA_CLOAKED,
                DWMWA_EXTENDED_FRAME_BOUNDS, DWM_THUMBNAIL_PROPERTIES, DWM_TNP_OPACITY,
                DWM_TNP_RECTDESTINATION, DWM_TNP_RECTSOURCE, DWM_TNP_SOURCECLIENTAREAONLY,
                DWM_TNP_VISIBLE,
            },
            Gdi::ClientToScreen,
        },
        UI::WindowsAndMessaging::{
            GetWindow, GetWindowLongW, GetWindowRect, IsHungAppWindow, IsIconic, IsWindow,
            IsWindowVisible, SetForegroundWindow, GWL_EXSTYLE, GW_OWNER, WS_EX_TOOLWINDOW,
        },
    },
};

use crate::app_handoff_core::{visible_part, Area};

/// 应用的一个窗口：`outer` 带着系统那圈看不见的拖拽边，`visible` 是眼睛看到的。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Seen {
    pub handle: isize,
    pub outer: Area,
    pub visible: Area,
}

fn hwnd(handle: isize) -> HWND {
    HWND(handle as *mut core::ffi::c_void)
}

fn area(rect: RECT) -> Area {
    Area::new(
        rect.left,
        rect.top,
        rect.right - rect.left,
        rect.bottom - rect.top,
    )
}

fn rect(area: Area) -> RECT {
    RECT {
        left: area.x,
        top: area.y,
        right: area.x + area.w,
        bottom: area.y + area.h,
    }
}

/// 能拿来衔接的窗口：看得见、没最小化、没卡死、不是别的窗口带出来的小窗。
fn look(handle: isize) -> Option<Seen> {
    let window = hwnd(handle);
    // SAFETY: 这些调用只读窗口的状态；句柄失效时它们返回假或失败，不会越界。
    unsafe {
        if !IsWindow(Some(window)).as_bool()
            || !IsWindowVisible(window).as_bool()
            || IsIconic(window).as_bool()
            || IsHungAppWindow(window).as_bool()
        {
            return None;
        }
        if GetWindow(window, GW_OWNER).is_ok_and(|owner| !owner.is_invalid()) {
            return None;
        }
        if GetWindowLongW(window, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0 {
            return None;
        }
        // 系统自己藏起来的窗口（别的虚拟桌面上的、挂起的）不算看得见。
        let mut cloaked = 0u32;
        let asked = DwmGetWindowAttribute(
            window,
            DWMWA_CLOAKED,
            (&mut cloaked as *mut u32).cast(),
            std::mem::size_of::<u32>() as u32,
        );
        if asked.is_ok() && cloaked != 0 {
            return None;
        }
        let mut outer = RECT::default();
        GetWindowRect(window, &mut outer).ok()?;
        let mut visible = RECT::default();
        let framed = DwmGetWindowAttribute(
            window,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&mut visible as *mut RECT).cast(),
            std::mem::size_of::<RECT>() as u32,
        );
        let outer = area(outer);
        let visible = if framed.is_ok() { area(visible) } else { outer };
        (visible.w > 0 && visible.h > 0).then_some(Seen {
            handle,
            outer,
            visible,
        })
    }
}

/// 应用的这些窗口里，现在摆在桌面上最大的那一个。
pub(crate) fn pick(handles: &[isize]) -> Option<Seen> {
    handles
        .iter()
        .filter_map(|handle| look(*handle))
        .max_by_key(|seen| i64::from(seen.visible.w) * i64::from(seen.visible.h))
}

/// 我们自己的窗口里面那块能画东西的地方，左上角在屏幕上的位置。
pub(crate) fn inside_origin(own: isize) -> Option<(i32, i32)> {
    let mut point = POINT::default();
    // SAFETY: `point` 在调用期间一直有效。
    unsafe { ClientToScreen(hwnd(own), &mut point) }
        .as_bool()
        .then_some((point.x, point.y))
}

/// 贴在我们窗口里的、应用窗口的实时画面。丢掉它，画面就撤掉。
pub(crate) struct Picture {
    id: isize,
    cut: Option<Area>,
}

impl Picture {
    /// 把 `source` 的画面接到我们自己的窗口 `own` 上。系统不肯接（没开桌面合成、
    /// 那个窗口不让看）就返回空，调用的人改成直接淡出。
    pub(crate) fn of(own: isize, source: Seen) -> Option<Self> {
        // SAFETY: 两个句柄都只交给系统去认；认不出来时调用失败。
        let id = unsafe { DwmRegisterThumbnail(hwnd(own), hwnd(source.handle)) }.ok()?;
        // SAFETY: `id` 是刚拿到的。
        let size = unsafe { DwmQueryThumbnailSourceSize(id) }.ok();
        let cut =
            size.and_then(|size| visible_part(source.outer, source.visible, (size.cx, size.cy)));
        Some(Self { id, cut })
    }

    /// 画在我们窗口里的 `place` 这一块（以窗口里面的左上角为原点），`opacity` 是 0 到 255。
    pub(crate) fn show(&self, place: Area, opacity: u8) -> bool {
        let mut flags = DWM_TNP_VISIBLE
            | DWM_TNP_OPACITY
            | DWM_TNP_RECTDESTINATION
            | DWM_TNP_SOURCECLIENTAREAONLY;
        if self.cut.is_some() {
            flags |= DWM_TNP_RECTSOURCE;
        }
        let properties = DWM_THUMBNAIL_PROPERTIES {
            dwFlags: flags,
            rcDestination: rect(place),
            rcSource: self.cut.map(rect).unwrap_or_default(),
            opacity,
            fVisible: BOOL(1),
            fSourceClientAreaOnly: BOOL(0),
        };
        // SAFETY: `properties` 在调用期间一直有效。
        unsafe { DwmUpdateThumbnailProperties(self.id, &properties) }.is_ok()
    }
}

impl Drop for Picture {
    fn drop(&mut self) {
        // SAFETY: `id` 只在这里注销一次。
        let _ = unsafe { DwmUnregisterThumbnail(self.id) };
    }
}

/// 演完了，把应用的窗口提到最前面。系统不让抢焦点时窗口也已经在那儿了，不算失败。
pub(crate) fn bring_forward(handle: isize) {
    // SAFETY: 句柄失效时系统直接返回假。
    let _ = unsafe { SetForegroundWindow(hwnd(handle)) };
}
