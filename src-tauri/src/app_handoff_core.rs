//! 打开应用时的衔接，纯计算的那一半：应用的窗口够不够稳、屏幕先摆在哪、怎么放大过去。
//! 这里不碰系统，所有数字都是物理像素。

/// 屏幕上的一块矩形。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Area {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Area {
    pub(crate) const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    fn right(self) -> i64 {
        i64::from(self.x) + i64::from(self.w)
    }

    fn bottom(self) -> i64 {
        i64::from(self.y) + i64::from(self.h)
    }

    /// 小于这个的不是能用的主窗口：启动画面、托盘气泡、还没撑开的空壳。
    fn window_sized(self) -> bool {
        self.w >= 480 && self.h >= 320
    }

    /// `other` 整个落在这块里面。
    pub(crate) fn holds(self, other: Area) -> bool {
        other.w > 0
            && other.h > 0
            && other.x >= self.x
            && other.y >= self.y
            && other.right() <= self.right()
            && other.bottom() <= self.bottom()
    }

    /// 以 `origin` 的左上角为原点重新表示。
    pub(crate) fn within(self, origin: Area) -> Area {
        Area::new(self.x - origin.x, self.y - origin.y, self.w, self.h)
    }
}

/// 连续几次看到同一个位置才算窗口摆好了。应用刚弹出来时常常先出一个小窗，
/// 再跳到上次的位置和大小；那一下不能当成它最后的样子。
pub(crate) const SETTLED_AFTER: u32 = 4;

#[derive(Default)]
pub(crate) struct Settle {
    last: Option<Area>,
    seen: u32,
}

impl Settle {
    /// 每看一次报一次；窗口摆好了就把它的位置交出来。
    pub(crate) fn observe(&mut self, now: Option<Area>) -> Option<Area> {
        let now = now.filter(|area| area.window_sized());
        if now.is_some() && now == self.last {
            self.seen += 1;
        } else {
            self.last = now;
            self.seen = u32::from(now.is_some());
        }
        (self.seen >= SETTLED_AFTER).then_some(self.last).flatten()
    }
}

/// 应用的画面先出现在舞台上的哪一块：偏右、居中，和应用窗口同一个长宽比，
/// 最宽占舞台的四成、最高占一半。
pub(crate) fn slot(stage: Area, target: Area) -> Area {
    let max_w = f64::from(stage.w) * 0.40;
    let max_h = f64::from(stage.h) * 0.50;
    let scale = (max_w / f64::from(target.w.max(1)))
        .min(max_h / f64::from(target.h.max(1)))
        .min(1.0);
    let w = (f64::from(target.w) * scale).round().max(1.0) as i32;
    let h = (f64::from(target.h) * scale).round().max(1.0) as i32;
    let centre_x = f64::from(stage.x) + f64::from(stage.w) * 0.64;
    let centre_y = f64::from(stage.y) + f64::from(stage.h) * 0.50;
    Area::new(
        (centre_x - f64::from(w) / 2.0).round() as i32,
        (centre_y - f64::from(h) / 2.0).round() as i32,
        w,
        h,
    )
}

/// 先慢、中间快、最后慢。`progress` 在 0 到 1 之外的按两头算。
pub(crate) fn ease(progress: f64) -> f64 {
    let t = progress.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

/// 从 `from` 走到 `to` 的路上，走了 `progress`（0 到 1）时的位置。
pub(crate) fn between(from: Area, to: Area, progress: f64) -> Area {
    let t = ease(progress);
    let mix = |a: i32, b: i32| (f64::from(a) + (f64::from(b) - f64::from(a)) * t).round() as i32;
    Area::new(
        mix(from.x, to.x),
        mix(from.y, to.y),
        mix(from.w, to.w),
        mix(from.h, to.h),
    )
}

/// 系统给的窗口画面有时带着一圈看不见的拖拽边。`outer` 是带那圈边的窗口，
/// `visible` 是眼睛看到的那一块，`source` 是系统给的画面有多大。
/// 画面带着那圈边时，返回要从画面里裁出来的那一块；画面本来就只有看得见的部分时不用裁。
pub(crate) fn visible_part(outer: Area, visible: Area, source: (i32, i32)) -> Option<Area> {
    if outer == visible || source == (visible.w, visible.h) {
        return None;
    }
    if source != (outer.w, outer.h) || !outer.holds(visible) {
        return None;
    }
    Some(visible.within(outer))
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAGE: Area = Area::new(0, 0, 1920, 1080);

    #[test]
    fn a_window_counts_only_after_it_stops_moving() {
        let first = Area::new(700, 300, 500, 400);
        let restored = Area::new(100, 80, 1400, 900);
        let mut settle = Settle::default();
        assert_eq!(settle.observe(None), None);
        assert_eq!(settle.observe(Some(first)), None);
        assert_eq!(settle.observe(Some(first)), None);
        // 跳到上次的位置：重新数。
        assert_eq!(settle.observe(Some(restored)), None);
        assert_eq!(settle.observe(Some(restored)), None);
        assert_eq!(settle.observe(Some(restored)), None);
        assert_eq!(settle.observe(Some(restored)), Some(restored));
        // 中途不见了（被最小化、关掉）：从头来。
        assert_eq!(settle.observe(None), None);
        assert_eq!(settle.observe(Some(restored)), None);
    }

    #[test]
    fn a_splash_sized_window_never_settles() {
        let splash = Area::new(800, 400, 320, 240);
        let mut settle = Settle::default();
        for _ in 0..SETTLED_AFTER * 3 {
            assert_eq!(settle.observe(Some(splash)), None);
        }
    }

    #[test]
    fn the_slot_keeps_the_window_shape_and_stays_on_stage() {
        for target in [
            Area::new(100, 80, 1400, 900),
            Area::new(0, 0, 1920, 1040),
            Area::new(300, 0, 600, 1000),
            Area::new(900, 500, 500, 400),
        ] {
            let slot = slot(STAGE, target);
            assert!(STAGE.holds(slot), "{slot:?}");
            assert!(slot.w <= 768 && slot.h <= 540, "{slot:?}");
            assert!(slot.w <= target.w && slot.h <= target.h, "{slot:?}");
            let shape = f64::from(target.w) / f64::from(target.h);
            let kept = f64::from(slot.w) / f64::from(slot.h);
            assert!((shape - kept).abs() < 0.01, "{target:?} -> {slot:?}");
        }
    }

    #[test]
    fn the_slot_follows_a_stage_that_is_not_at_the_origin() {
        let second_monitor = Area::new(-1920, 200, 1920, 1080);
        let slot = slot(second_monitor, Area::new(-1800, 300, 1400, 900));
        assert!(second_monitor.holds(slot), "{slot:?}");
    }

    #[test]
    fn the_way_over_starts_and_ends_exactly() {
        let from = Area::new(900, 300, 700, 450);
        let to = Area::new(100, 80, 1400, 900);
        assert_eq!(between(from, to, 0.0), from);
        assert_eq!(between(from, to, 1.0), to);
        assert_eq!(between(from, to, 7.0), to);
        assert_eq!(between(from, to, -1.0), from);
        let mut last = from.w;
        for step in 1..=20 {
            let width = between(from, to, f64::from(step) / 20.0).w;
            assert!(width >= last, "width went back at step {step}");
            last = width;
        }
    }

    #[test]
    fn only_a_picture_with_the_unseen_border_is_cut() {
        let outer = Area::new(92, 80, 1416, 908);
        let visible = Area::new(100, 80, 1400, 900);
        assert_eq!(
            visible_part(outer, visible, (1416, 908)),
            Some(Area::new(8, 0, 1400, 900))
        );
        assert_eq!(visible_part(outer, visible, (1400, 900)), None);
        assert_eq!(visible_part(visible, visible, (1400, 900)), None);
        // 对不上的大小：不猜，整张用。
        assert_eq!(visible_part(outer, visible, (1000, 700)), None);
    }

    #[test]
    fn a_window_on_another_monitor_is_not_held_by_the_stage() {
        assert!(STAGE.holds(Area::new(100, 80, 1400, 900)));
        assert!(!STAGE.holds(Area::new(1920, 0, 1400, 900)));
        assert!(!STAGE.holds(Area::new(-8, -8, 1936, 1056)));
        assert!(!STAGE.holds(Area::new(10, 10, 0, 0)));
    }
}
