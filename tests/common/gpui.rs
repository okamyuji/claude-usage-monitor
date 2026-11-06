//! GPUI版の画面を、ラベルや役割の名前で部品を探して操作するハーネス。
//! GPUIのテスト窓は読み上げ用の木を作らないため、部品が描くたびに残す記録（`probe`）と目印（`debug_selector`）で探す。
use super::gui::GuiEnv;
use claude_usage_monitor::controllers::gui::app::{GuiController, GuiDeps};
use claude_usage_monitor::views::app::CumonRoot;
use claude_usage_monitor::views::probe::{self, Role};
use gpui::{
    AnyWindowHandle, AppContext as _, Bounds, Entity, Modifiers, MouseButton, Pixels,
    TestAppContext, VisualTestContext, point, px, size,
};

/// 本物の文字の仕組みと同梱のフォントを持つテスト用アプリ。テスト窓の既定の文字の仕組みはフォントも幅も持たず、
/// LinuxのCIにはOSのフォントも無いため。
pub fn gpui_cx() -> TestAppContext {
    #[cfg(target_os = "macos")]
    let text = std::sync::Arc::new(gpui_apple::AppleTextSystem::new());
    #[cfg(target_os = "linux")]
    let text = std::sync::Arc::new(gpui_wgpu::CosmicTextSystem::new_without_system_fonts(
        claude_usage_monitor::views::fonts::BODY_FAMILY,
    ));
    let cx = TestAppContext::build_with_text_system(gpui::TestDispatcher::new(0), None, text);
    cx.update(|cx| {
        gpui_component::init(cx);
        claude_usage_monitor::views::fonts::install(cx).expect("同梱のフォント");
    });
    cx
}

/// GPUIのツールチップが出るまでの待ち（500ms）より少し長い時間。
const TOOLTIP_WAIT: std::time::Duration = std::time::Duration::from_millis(600);

/// 画面の部品1つ。
pub struct Node {
    cx: VisualTestContext,
    bounds: Bounds<Pixels>,
    label: String,
}

impl Node {
    /// 部品の位置。
    pub fn bounds(&self) -> Bounds<Pixels> {
        self.bounds
    }

    /// 部品の名前。
    pub fn label(&self) -> &str {
        &self.label
    }

    /// 中央を押す。
    pub fn click(mut self) {
        self.cx
            .simulate_click(self.bounds.center(), Modifiers::default());
        self.cx.run_until_parked();
    }

    /// 中央にマウスを乗せ、ツールチップが出るまで時計を進める。
    pub fn hover(mut self) {
        self.cx
            .simulate_mouse_move(self.bounds.center(), None, Modifiers::default());
        self.cx.run_until_parked();
        self.cx.executor().advance_clock(TOOLTIP_WAIT);
        self.cx.run_until_parked();
    }

    /// 中央をつかんで横へ`dx`だけ動かし、離す。
    pub fn drag_by(mut self, dx: f32) {
        let from = self.bounds.center();
        let to = from + point(px(dx), px(0.0));
        self.cx
            .simulate_mouse_down(from, MouseButton::Left, Modifiers::default());
        self.cx
            .simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::default());
        self.cx
            .simulate_mouse_up(to, MouseButton::Left, Modifiers::default());
        self.cx.run_until_parked();
    }

    /// 押して入力欄に選び、文字を入れる。
    pub fn type_text(mut self, text: &str) {
        self.cx
            .simulate_click(self.bounds.center(), Modifiers::default());
        self.cx.simulate_input(text);
        self.cx.run_until_parked();
    }
}

/// GPUI版の画面のハーネス。
pub struct GpuiHarness {
    cx: VisualTestContext,
    window: AnyWindowHandle,
    root: Entity<CumonRoot>,
    seen: Vec<(Role, String)>,
}

impl GuiEnv {
    /// 既定の大きさ（1280×820）の窓で動かす。`cumon gui`の初期サイズと同じにするため。
    pub fn gpui_harness(&self) -> GpuiHarness {
        self.gpui_harness_sized(1280.0, 820.0)
    }

    /// 大きさを指定して動かす。伸縮の確認に使う。
    pub fn gpui_harness_sized(&self, w: f32, h: f32) -> GpuiHarness {
        self.gpui_harness_of(self.deps(), w, h)
    }

    /// 依存を差し替えて動かす。読めない自動起動や、返らない取得を作るときに使う。
    pub fn gpui_harness_with(&self, deps: GuiDeps) -> GpuiHarness {
        self.gpui_harness_of(deps, 1280.0, 820.0)
    }

    fn gpui_harness_of(&self, deps: GuiDeps, w: f32, h: f32) -> GpuiHarness {
        let mut cx = gpui_cx();
        let ctl = GuiController::new(deps);
        let mut root = None;
        let window = cx.add_window(|w, cx| {
            let r = cx.new(|cx| CumonRoot::new(ctl, w, cx));
            root = Some(r.clone());
            gpui_component::Root::new(r, w, cx)
        });
        cx.simulate_window_resize(window.into(), size(px(w), px(h)));
        let mut harness = GpuiHarness {
            cx: VisualTestContext::from_window(window.into(), &cx),
            window: window.into(),
            root: root.expect("根のビュー"),
            seen: Vec::new(),
        };
        harness.run();
        harness
    }
}

impl GpuiHarness {
    /// `cumon gui`のタイマーと同じく`tick`を呼んで描き直し、描いた部品の記録を取り直す。
    pub fn run(&mut self) {
        self.root.update(&mut self.cx, |r, cx| {
            r.controller_mut().tick();
            cx.notify();
        });
        self.cx.run_until_parked();
        let seen = probe::take();
        if !seen.is_empty() {
            self.seen = seen;
        }
    }

    /// 1回描き直す。
    pub fn step(&mut self) {
        self.run();
    }

    /// controllerを読む。
    pub fn with_state<R>(&mut self, f: impl FnOnce(&GuiController) -> R) -> R {
        self.root.read_with(&self.cx, |r, _| f(r.controller()))
    }

    /// controllerを書き換える。
    pub fn with_state_mut<R>(&mut self, f: impl FnOnce(&mut GuiController) -> R) -> R {
        self.root.update(&mut self.cx, |r, _| f(r.controller_mut()))
    }

    /// 再生位置のスライダーの値。
    pub fn replay_slider_value(&mut self) -> Option<f32> {
        self.root
            .read_with(&self.cx, |r, cx| r.replay_slider_value(cx))
    }

    /// `n`番目の文字の入力欄の値。
    pub fn input_value(&mut self, n: usize) -> String {
        self.root
            .read_with(&self.cx, |r, cx| r.input_values(cx))
            .into_iter()
            .nth(n)
            .unwrap_or_else(|| panic!("{n}番目の入力欄が無い"))
    }

    /// キーを押す（`escape`など、GPUIのキーの名前）。
    pub fn press_key(&mut self, key: &str) {
        self.cx.simulate_keystrokes(key);
        self.cx.run_until_parked();
    }

    /// 窓の中の位置でホイールを`dy`だけ回す（正の値で上へ戻る）。
    pub fn scroll_at(&mut self, x: f32, y: f32, dy: f32) {
        self.cx.simulate_event(gpui::ScrollEvent {
            position: point(px(x), px(y)),
            delta: gpui::ScrollDelta::Pixels(point(px(0.0), px(dy))),
            ..Default::default()
        });
        self.run();
    }

    /// 窓の中の位置を押す。
    pub fn click_at(&mut self, x: f32, y: f32) {
        self.cx
            .simulate_click(point(px(x), px(y)), Modifiers::default());
        self.cx.run_until_parked();
    }

    /// 窓の中の位置にマウスを乗せる。
    pub fn hover_at(&mut self, x: f32, y: f32) {
        self.cx
            .simulate_mouse_move(point(px(x), px(y)), None, Modifiers::default());
        self.run();
    }

    fn node(&self, role: Role, label: &str, nth: usize) -> Option<Node> {
        let key: &'static str = Box::leak(probe::selector(role, label, nth).into_boxed_str());
        let mut cx = VisualTestContext::from_window(self.window, &self.cx);
        let bounds = cx.debug_bounds(key)?;
        Some(Node {
            cx,
            bounds,
            label: label.to_string(),
        })
    }

    /// 記録のうち`keep`に合う部品を、描いた順に返す。
    fn find(&self, keep: impl Fn(Role, &str) -> bool) -> Vec<Node> {
        let mut out = Vec::new();
        for (i, (role, label)) in self.seen.iter().enumerate() {
            if !keep(*role, label) {
                continue;
            }
            let nth = self.seen[..i]
                .iter()
                .filter(|(r, l)| r == role && l == label)
                .count();
            out.extend(self.node(*role, label, nth));
        }
        out
    }

    fn missing(&self, what: &str) -> ! {
        let names: Vec<&str> = self.seen.iter().map(|(_, l)| l.as_str()).collect();
        panic!("{what} が見つからない。描いた部品: {names:?}")
    }

    /// 描いた部品の名前を、描いた順（読み上げの順）に返す。
    pub fn labels(&self) -> Vec<&str> {
        self.seen.iter().map(|(_, l)| l.as_str()).collect()
    }

    /// 名前が完全に一致する部品。
    pub fn get_by_label(&self, label: &str) -> Node {
        self.query_by_label(label)
            .unwrap_or_else(|| self.missing(label))
    }

    /// 名前が完全に一致する部品。無ければ`None`。
    pub fn query_by_label(&self, label: &str) -> Option<Node> {
        self.find(|_, l| l == label).into_iter().next()
    }

    /// 名前に`part`を含む部品。
    pub fn get_by_label_contains(&self, part: &str) -> Node {
        self.query_by_label_contains(part)
            .unwrap_or_else(|| self.missing(part))
    }

    /// 名前に`part`を含む部品。無ければ`None`。
    pub fn query_by_label_contains(&self, part: &str) -> Option<Node> {
        self.find(|_, l| l.contains(part)).into_iter().next()
    }

    /// 名前が完全に一致する部品のすべて。
    pub fn get_all_by_label(&self, label: &str) -> impl Iterator<Item = Node> {
        self.find(|_, l| l == label).into_iter()
    }

    /// 名前に`part`を含む部品のすべて。
    pub fn get_all_by_label_contains(&self, part: &str) -> impl Iterator<Item = Node> {
        self.find(|_, l| l.contains(part)).into_iter()
    }

    /// 役割が一致する部品のすべて。
    pub fn query_all_by_role(&self, role: Role) -> impl Iterator<Item = Node> {
        self.find(|r, _| r == role).into_iter()
    }

    /// 役割と名前が一致する部品。
    pub fn get_by_role_and_label(&self, role: Role, label: &str) -> Node {
        self.find(|r, l| r == role && l == label)
            .into_iter()
            .next()
            .unwrap_or_else(|| self.missing(label))
    }
}
