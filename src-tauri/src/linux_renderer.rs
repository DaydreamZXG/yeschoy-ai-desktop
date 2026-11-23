//! Linux 上画界面的是系统自带的 WebKitGTK，它有一个单独的渲染进程。
//!
//! WebKitGTK 2.50 的这个进程会在用显卡画滤镜、模糊时崩掉（Skia 的显卡资源缓存越界，
//! `GrResourceCache` 里的断言），缩放窗口时最容易撞上。它一崩，窗口还在，里面只剩底色，
//! 点什么都没反应，只能关掉重开。
//!
//! 这里做两件事：让它改用不走显卡的画法，避开出错的那段；万一还是崩了，把界面重新载入。
//! 接入、登录这些状态都在本进程里，重新载入后界面照原样回来。

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

pub fn reload_when_renderer_dies(app: &tauri::App) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let wired = window.with_webview(|webview| {
        use webkit2gtk::WebViewExt;
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
