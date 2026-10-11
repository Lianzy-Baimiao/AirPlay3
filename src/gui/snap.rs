//! 磁性吸附与自动对齐：照搬原版 AirPlay3（逆向自其 WM_WINDOWPOSCHANGING 处理）。
//!
//! - 每个面板参与比较的"吸附矩形"是外框内缩 1 像素（右、下为闭区间）：`(x+1, y+1, x+w-2, y+h-2)`；
//!   屏幕工作区、任务栏用其原始矩形（右、下为开区间）。默认布局下相邻面板的吸附矩形正好共用一条边。
//! - 移动 / 调整时，边与其它矩形的边距离在 12 像素内（且不相等）就对齐过去，取距离最小的（相同时取后面的）。
//!   依次与任务栏、工作区、其它可见面板比较。
//! - 拖播放条时，和它直接或间接"贴着"的面板一起移动；拖单独的面板时只移动它自己（脱离）。
//! - 调整大小时，被拖动的边吸附到其它面板 / 屏幕的边上（自动对齐）。

use super::canvas::R;

/// 吸附距离（原版常量 12）
pub const DIST: i32 = 12;

/// 拖动方向（与原版相同的位定义）：0 = 移动
pub const LEFT: u32 = 0x1;
pub const RIGHT: u32 = 0x2;
pub const TOP: u32 = 0x4;
pub const BOTTOM: u32 = 0x8;
pub const BOTTOM_LEFT: u32 = 0x40;
pub const BOTTOM_RIGHT: u32 = 0x80;

fn has_left(e: u32) -> bool {
    e & (LEFT | 0x10 | BOTTOM_LEFT) != 0
}
fn has_right(e: u32) -> bool {
    e & (RIGHT | 0x20 | BOTTOM_RIGHT) != 0
}
fn has_top(e: u32) -> bool {
    e & (TOP | 0x10 | 0x20) != 0
}
fn has_bottom(e: u32) -> bool {
    e & (BOTTOM | BOTTOM_LEFT | BOTTOM_RIGHT) != 0
}

/// 边坐标（闭区间）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edges {
    pub l: i32,
    pub t: i32,
    pub r: i32,
    pub b: i32,
}

/// 面板的吸附矩形
pub fn pane_edges(r: R) -> Edges {
    Edges { l: r.x + 1, t: r.y + 1, r: r.x + r.w - 2, b: r.y + r.h - 2 }
}

/// 工作区 / 任务栏：原始矩形
pub fn screen_edges(r: R) -> Edges {
    Edges { l: r.x, t: r.y, r: r.x + r.w, b: r.y + r.h }
}

/// 吸附环境：任务栏矩形 + 按矩形中心取所在显示器工作区
pub struct Env<'a> {
    pub taskbar: Option<R>,
    pub work_area: &'a dyn Fn(R) -> R,
}

/// 各方向找到的最小位移
#[derive(Default)]
struct Best {
    dx: Option<i32>,
    dy: Option<i32>,
    dl: Option<i32>,
    dt: Option<i32>,
    dr: Option<i32>,
    db: Option<i32>,
}

fn offer(slot: &mut Option<i32>, v: i32) {
    // 原版：新的绝对值不大于旧的就替换（相同时取后面的）
    if slot.is_none_or(|b| v.abs() <= b.abs()) {
        *slot = Some(v);
    }
}

/// 一条边与目标的两条边比较（不相等且距离不超过 12）
fn edge(slot: &mut Option<i32>, s: i32, o1: i32, o2: i32) {
    for o in [o1, o2] {
        if s != o && (s - o).abs() <= DIST {
            offer(slot, o - s);
        }
    }
}

/// 收集矩形 r（拖动方向 e）相对目标 o 的吸附位移
fn collect(best: &mut Best, r: R, e: u32, o: Edges) {
    let s = pane_edges(r);
    // 两个矩形（各外扩 12）不相交时不吸附
    if s.l > o.r + DIST || s.r < o.l - DIST || s.t > o.b + DIST || s.b < o.t - DIST {
        return;
    }
    edge(if has_left(e) { &mut best.dl } else { &mut best.dx }, s.l, o.l, o.r);
    edge(if has_right(e) { &mut best.dr } else { &mut best.dx }, s.r, o.l, o.r);
    edge(if has_top(e) { &mut best.dt } else { &mut best.dy }, s.t, o.t, o.b);
    edge(if has_bottom(e) { &mut best.db } else { &mut best.dy }, s.b, o.t, o.b);
}

fn apply(r: &mut R, b: &Best) {
    if let Some(v) = b.dx {
        r.x += v;
    }
    if let Some(v) = b.dy {
        r.y += v;
    }
    if let Some(v) = b.dl {
        r.x += v;
        r.w -= v;
    }
    if let Some(v) = b.dt {
        r.y += v;
        r.h -= v;
    }
    if let Some(v) = b.dr {
        r.w += v;
    }
    if let Some(v) = b.db {
        r.h += v;
    }
}

fn pass(r: &mut R, e: u32, targets: impl IntoIterator<Item = Edges>) {
    let mut best = Best::default();
    for o in targets {
        collect(&mut best, *r, e, o);
    }
    apply(r, &best);
}

/// 把矩形 r 依次吸附到任务栏、工作区、其它面板（`others` 为其它可见面板的外框）
pub fn snap(mut r: R, e: u32, env: &Env, others: &[R]) -> R {
    if let Some(t) = env.taskbar {
        pass(&mut r, e, [screen_edges(t)]);
    }
    let wa = (env.work_area)(r);
    pass(&mut r, e, [screen_edges(wa)]);
    pass(&mut r, e, others.iter().map(|o| pane_edges(*o)));
    r
}

/// 吸附矩形 b 内缩 1 像素的四个角是否落在 a 内（闭区间）
fn corners_inside(a: Edges, b: Edges) -> bool {
    let inside = |x: i32, y: i32| x >= a.l && x <= a.r && y >= a.t && y <= a.b;
    [(b.l + 1, b.t + 1), (b.r - 1, b.t + 1), (b.l + 1, b.b - 1), (b.r - 1, b.b - 1)].iter().any(|&(x, y)| inside(x, y))
}

/// 两个面板"贴着"：吸附矩形相交、没有深度重叠，并且至少有一条边坐标相同
pub fn touching(a: R, b: R) -> bool {
    let (a, b) = (pane_edges(a), pane_edges(b));
    if corners_inside(a, b) || corners_inside(b, a) {
        return false;
    }
    if a.l > b.r || a.r < b.l || a.t > b.b || a.b < b.t {
        return false;
    }
    a.l == b.l || a.l == b.r || a.r == b.l || a.r == b.r || a.t == b.t || a.t == b.b || a.b == b.t || a.b == b.b
}

/// 与 lead 直接或间接贴着的面板（含 lead 自己）
pub fn group(rects: &[R], lead: usize) -> Vec<bool> {
    let mut g = vec![false; rects.len()];
    g[lead] = true;
    loop {
        let mut grew = false;
        for i in 0..rects.len() {
            if !g[i] && (0..rects.len()).any(|j| g[j] && touching(rects[i], rects[j])) {
                g[i] = true;
                grew = true;
            }
        }
        if !grew {
            return g;
        }
    }
}

/// 移动：`rects` 为按下时各面板的位置，`moving` 为一起移动的面板，`p` 为 lead 跟随鼠标的新位置。
/// 返回 lead 吸附后的位置（其它一起移动的面板平移同样的距离）。
/// `main_bar` 为 true（拖的是播放条）时，一起移动的每个面板也分别吸附，并带动整组（原版行为）。
pub fn group_move(rects: &[R], visible: &[bool], moving: &[bool], lead: usize, p: R, main_bar: bool, env: &Env) -> R {
    let others: Vec<R> = (0..rects.len()).filter(|&i| visible[i] && !moving[i]).map(|i| rects[i]).collect();
    let mut p = snap(p, 0, env, &others);
    if !main_bar {
        return p;
    }
    for i in 0..rects.len() {
        // 隐藏的面板只跟着平移，不参与吸附（否则整组会被看不见的面板吸过去）
        if !moving[i] || !visible[i] {
            continue;
        }
        let (dx, dy) = (p.x - rects[lead].x, p.y - rects[lead].y);
        let q = R::new(rects[i].x + dx, rects[i].y + dy, rects[i].w, rects[i].h);
        let s = snap(q, 0, env, &others);
        p.x += s.x - q.x;
        p.y += s.y - q.y;
    }
    p
}

/// 调整大小：被拖动的边吸附到其它可见面板 / 屏幕的边
pub fn resize(rects: &[R], visible: &[bool], me: usize, p: R, edges: u32, env: &Env) -> R {
    let others: Vec<R> = (0..rects.len()).filter(|&i| i != me && visible[i]).map(|i| rects[i]).collect();
    snap(p, edges, env, &others)
}

#[cfg(test)]
mod tests {
    use super::*;

    // 原版默认布局（外框，屏幕坐标），顺序与原版吸附对象表相同：播放条、音乐列表、歌词图片、均衡器
    const BAR: R = R::new(338, 98, 304, 68);
    const LIST: R = R::new(639, 98, 324, 431);
    const COVER: R = R::new(338, 163, 304, 225);
    const EQ: R = R::new(338, 385, 304, 144);

    fn env(w: i32, h: i32) -> (Option<R>, impl Fn(R) -> R) {
        (None, move |_| R::new(0, 0, w, h))
    }

    fn layout() -> [R; 4] {
        [BAR, LIST, COVER, EQ]
    }

    #[test]
    fn default_layout_is_one_group() {
        let r = layout();
        assert!(touching(BAR, COVER) && touching(COVER, EQ) && touching(BAR, LIST) && touching(EQ, LIST));
        assert_eq!(group(&r, 0), vec![true; 4]);
    }

    /// 与在沙盒里对原版做的实验结果一致：歌词图片下移 7 吸回；移开 40 脱离；再移回 35 又吸上
    #[test]
    fn panel_moves_match_original() {
        let (tb, wa) = env(3840, 2120);
        let env = Env { taskbar: tb, work_area: &wa };
        let r = layout();
        let vis = [true; 4];
        let solo = [false, false, true, false];
        let mv = |p: R| group_move(&r, &vis, &solo, 2, p, false, &env);
        assert_eq!(mv(R::new(338, 170, 304, 225)), COVER);
        let far = R::new(378, 203, 304, 225);
        assert_eq!(mv(far), far);
        let back = R::new(343, 168, 304, 225);
        assert_eq!(mv(back), COVER);
    }

    /// 音乐列表底边：拖短 7 像素时吸回与均衡器底边对齐，拖长 30 像素时不吸
    #[test]
    fn resize_aligns_edges_like_original() {
        let (tb, wa) = env(3840, 2120);
        let env = Env { taskbar: tb, work_area: &wa };
        let r = layout();
        let vis = [true; 4];
        assert_eq!(resize(&r, &vis, 1, R::new(639, 98, 324, 424), BOTTOM, &env), LIST);
        let long = R::new(639, 98, 324, 461);
        assert_eq!(resize(&r, &vis, 1, long, BOTTOM, &env), long);
        // 歌词图片变高时底边先吸在均衡器顶边，超过 12 像素后才越过去
        let c = resize(&r, &vis, 2, R::new(338, 163, 304, 233), BOTTOM, &env);
        assert_eq!(c, COVER);
    }

    #[test]
    fn whole_group_snaps_to_screen_edge() {
        let (tb, wa) = env(1920, 1080);
        let env = Env { taskbar: tb, work_area: &wa };
        let r = layout();
        let vis = [true; 4];
        let all = group(&r, 0);
        // 播放条拖到离左边 5 像素：吸到左边（吸附矩形贴边，外框在 -1）
        let p = group_move(&r, &vis, &all, 0, R::new(5, 98, 304, 68), true, &env);
        assert_eq!(p, R::new(-1, 98, 304, 68));
        // 拖到音乐列表右边离屏幕右边 5 像素：整组被列表带着贴到右边
        let p = group_move(&r, &vis, &all, 0, R::new(338 + 952, 98, 304, 68), true, &env);
        let list_right = pane_edges(R::new(LIST.x + p.x - BAR.x, LIST.y, LIST.w, LIST.h)).r;
        assert_eq!(list_right, 1920);
    }

    #[test]
    fn detached_panel_not_in_group() {
        let mut r = layout();
        r[2] = R::new(1200, 700, 304, 225);
        let g = group(&r, 0);
        assert_eq!(g, vec![true, true, false, true]);
        // 深度重叠不算贴着
        assert!(!touching(R::new(338, 163, 304, 260), EQ));
    }

    #[test]
    fn untouched_drag_stays_put() {
        let (tb, wa) = env(3840, 2120);
        let env = Env { taskbar: tb, work_area: &wa };
        let r = layout();
        let all = group(&r, 0);
        let p = R::new(BAR.x + 100, BAR.y + 50, BAR.w, BAR.h);
        assert_eq!(group_move(&r, &[true; 4], &all, 0, p, true, &env), p);
    }
}
