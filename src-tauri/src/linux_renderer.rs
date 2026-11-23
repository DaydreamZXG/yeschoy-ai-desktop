//! Linux 上画界面的是系统自带的 WebKitGTK，它有一个单独的渲染进程。
//!
//! WebKitGTK 2.50 的这个进程会在动显卡资源时崩掉（Skia 的显卡资源缓存越界，
//! `GrResourceCache` 里的断言），缩放窗口时最容易撞上。它一崩，窗口还在，里面只剩底色，
//! 点什么都没反应，只能关掉重开。崩溃转储里看到过两条路：
//!
//! - 页面内容用显卡画滤镜、模糊时；
//! - `<canvas>` 用显卡画时：主线程在刷新这块画布，合成线程同时在销毁它上一帧的图像，
//!   两条线程动的是同一个缓存。首页那层星尘和火花就是一块一直在画的 2D 画布。
//!
//! 这里做三件事：页面内容和 2D 画布都改用不走显卡的画法，避开出错的那段；
//! 万一还是崩了，把界面重新载入。接入、登录这些状态都在本进程里，重新载入后界面照原样回来。

use std::time::{Duration, Instant};

use tauri::Manager;

const CPU_PAINTING: &str = "WEBKIT_SKIA_ENABLE_CPU_RENDERING";

/// 必须在创建窗口、起任何线程之前调用。用户自己设过这个变量就听用户的。
pub fn prefer_cpu_painting() {
    if std::env::var_os(CPU_PAINTING).is_none() {
        std::env::set_var(CPU_PAINTING, "1");
    }
}

const RELOADS_ALLOWED: usize = 3;
const RELOAD_WINDOW: Duration = Duration::from_secs(60);

/// 一分钟内最多重新载入三次。一载入就崩的话不跟着无限重试，停下来把窗口留给用户关。
#[derive(Default)]
struct ReloadBudget {
    recent: Vec<Instant>,
}

impl ReloadBudget {
    fn allow(&mut self, now: Instant) -> bool {
        self.recent
            .retain(|at| now.duration_since(*at) < RELOAD_WINDOW);
        if self.recent.len() >= RELOADS_ALLOWED {
            return false;
        }
        self.recent.push(now);
        true
    }
}

const CANVAS_ON_GPU: &str = "enable-2d-canvas-acceleration";

/// 这个开关是 WebKitGTK 2.46 才有的；更早的版本没有它，也没有这条崩溃路径。
fn paint_canvas_on_cpu(view: &webkit2gtk::WebView) {
    use webkit2gtk::glib::prelude::ObjectExt;
    use webkit2gtk::WebViewExt;
    let Some(settings) = view.settings() else {
        return;
    };
    let known = settings.find_property(CANVAS_ON_GPU).is_some();
    if known {
        settings.set_property(CANVAS_ON_GPU, false);
    }
    log::info!("linux_renderer stage=canvas_on_cpu applied={known}");
}

pub fn reload_when_renderer_dies(app: &tauri::App) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let wired = window.with_webview(|webview| {
        use webkit2gtk::WebViewExt;
        paint_canvas_on_cpu(&webview.inner());
        let budget = std::cell::RefCell::new(ReloadBudget::default());
        webview
            .inner()
            .connect_web_process_terminated(move |view, reason| {
                if budget.borrow_mut().allow(Instant::now()) {
                    log::warn!("linux_renderer stage=terminated reason={reason:?} action=reload");
                    view.reload();
                } else {
                    log::error!("linux_renderer stage=terminated reason={reason:?} action=give_up");
                }
            });
    });
    if let Err(error) = wired {
        log::warn!("linux_renderer stage=wire_failed error={error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reloads_three_times_a_minute_then_stops() {
        let mut budget = ReloadBudget::default();
        let start = Instant::now();
        assert!(budget.allow(start));
        assert!(budget.allow(start + Duration::from_secs(1)));
        assert!(budget.allow(start + Duration::from_secs(2)));
        assert!(!budget.allow(start + Duration::from_secs(3)));
    }

    #[test]
    fn a_crash_long_after_the_last_ones_reloads_again() {
        let mut budget = ReloadBudget::default();
        let start = Instant::now();
        for second in 0..3 {
            assert!(budget.allow(start + Duration::from_secs(second)));
        }
        assert!(!budget.allow(start + Duration::from_secs(30)));
        assert!(budget.allow(start + Duration::from_secs(61)));
    }
}
