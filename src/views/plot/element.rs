//! 時系列のグラフの要素。描画と操作を受け持つ。
//!
//! 操作は、左ドラッグで移動、スクロールで移動、Cmd（LinuxではCtrl）＋スクロールで拡大、軸の上のドラッグで片方向の拡大、
//! 右ドラッグで矩形の拡大、ダブルクリックで自動の範囲へ戻す、凡例の項目を押して表示を切り替える、凡例に乗ると線を太くする。
use crate::views::fonts::BODY_FAMILY;
use crate::views::plot::format::tick_label;
use crate::views::plot::math::{
    GRID_SPACING, Range2, auto_color, auto_range, grid_alpha, grid_marks, label_alpha, nearest,
    to_screen,
};
pub use crate::views::plot::state::PlotState;
use crate::views::plot::state::{DragKind, rect, tooltip_origin};
use crate::views::probe::Role;
use crate::views::theme::{BODY, Palette, SMALL, color, with_alpha};
use crate::views::widgets::{tagged, text};
use gpui::{
    App, BorderStyle, Bounds, ContentMask, DispatchPhase, Div, Entity, Hsla,
    InteractiveElement as _, IntoElement, Length, MouseButton, MouseMoveEvent, MouseUpEvent,
    ParentElement as _, PathBuilder, Pixels, Point, RenderOnce, ShapedLine, SharedString,
    StatefulInteractiveElement as _, Styled as _, TextAlign, TextRun, Window, canvas, div, fill,
    font, outline, point, px, size,
};
use std::rc::Rc;

/// x軸とy軸の数字を出す間隔。
const X_LABEL_SPACING: (f32, f32) = (60.0, 80.0);
const Y_LABEL_SPACING: (f32, f32) = (20.0, 30.0);
/// 近い点として拾う半径。
const HOVER_RADIUS: f32 = 5.0;
/// x軸の数字の行の高さ。
const X_AXIS_HEIGHT: f32 = 20.0;
/// y軸の数字の左右の余白と、軸の最小の幅（`min_thickness`）。
const Y_AXIS_MARGIN: f32 = 4.0;
const Y_AXIS_MIN: f32 = 14.0;
/// 線の太さ。凡例に乗ると2倍にする。
const LINE_WIDTH: f32 = 1.5;

struct Line {
    name: SharedString,
    points: Vec<[f64; 2]>,
    color: u32,
}

type XFormatter = Rc<dyn Fn(f64) -> String>;
type HoverFormatter = Rc<dyn Fn(&str, f64, f64) -> String>;

/// 時系列のグラフ。
#[derive(IntoElement)]
pub struct Plot {
    id: SharedString,
    state: Entity<PlotState>,
    palette: &'static Palette,
    height: Length,
    legend: bool,
    include_x: Vec<f64>,
    include_y: Vec<f64>,
    x_formatter: Option<XFormatter>,
    hover_formatter: Option<HoverFormatter>,
    lines: Vec<Line>,
    vlines: Vec<(f64, u32)>,
    hlines: Vec<(f64, u32)>,
}

impl Plot {
    /// `state`に範囲と操作の状態を持つグラフ。
    pub fn new(
        id: impl Into<SharedString>,
        state: Entity<PlotState>,
        palette: &'static Palette,
    ) -> Self {
        Self {
            id: id.into(),
            state,
            palette,
            height: px(180.0).into(),
            legend: false,
            include_x: Vec::new(),
            include_y: Vec::new(),
            x_formatter: None,
            hover_formatter: None,
            lines: Vec::new(),
            vlines: Vec::new(),
            hlines: Vec::new(),
        }
    }

    /// 高さ。
    pub fn height(mut self, h: impl Into<Length>) -> Self {
        self.height = h.into();
        self
    }

    /// 凡例を右上に小さい文字で出す。
    pub fn legend(mut self, on: bool) -> Self {
        self.legend = on;
        self
    }

    /// 自動の範囲に必ず含めるx。
    pub fn include_x(mut self, x: f64) -> Self {
        self.include_x.push(x);
        self
    }

    /// 自動の範囲に必ず含めるy。
    pub fn include_y(mut self, y: f64) -> Self {
        self.include_y.push(y);
        self
    }

    /// x軸の数字の書式。
    pub fn x_formatter(mut self, f: impl Fn(f64) -> String + 'static) -> Self {
        self.x_formatter = Some(Rc::new(f));
        self
    }

    /// 点に乗ったときのツールチップの書式。
    pub fn hover_formatter(mut self, f: impl Fn(&str, f64, f64) -> String + 'static) -> Self {
        self.hover_formatter = Some(Rc::new(f));
        self
    }

    /// 折れ線。色は描く順に自動で決める。
    pub fn line(mut self, name: impl Into<SharedString>, points: Vec<[f64; 2]>) -> Self {
        let color = auto_color(self.lines.len());
        self.lines.push(Line {
            name: name.into(),
            points,
            color,
        });
        self
    }

    /// 縦の目印の線。凡例には出さない。
    pub fn vline(mut self, x: f64, hex: u32) -> Self {
        self.vlines.push((x, hex));
        self
    }

    /// 横の目印の線。凡例には出さない。
    pub fn hline(mut self, y: f64, hex: u32) -> Self {
        self.hlines.push((y, hex));
        self
    }
}

fn run(text: &str, c: Hsla) -> TextRun {
    TextRun {
        len: text.len(),
        font: font(BODY_FAMILY),
        color: c,
        background_color: None,
        underline: None,
        strikethrough: None,
    }
}

/// 描く前に決める値。
struct Layout {
    range: Range2,
    frame: Bounds<Pixels>,
    x_marks: Vec<(f64, f64)>,
    y_marks: Vec<(f64, f64)>,
}

/// 凡例。押すと系列を隠し、⌥で押すとその系列だけを残す。乗せた系列の線は太くなる。
fn legend_box(lines: &[Line], state: &Entity<PlotState>, p: &Palette, cx: &App) -> Div {
    let names: Vec<String> = lines
        .iter()
        .filter(|l| !l.name.is_empty())
        .map(|l| l.name.to_string())
        .collect();
    let entries = lines.iter().filter(|l| !l.name.is_empty()).map(|l| {
        let name = l.name.clone();
        let names = names.clone();
        let hidden = state.read(cx).is_hidden(&name);
        let (toggle_state, hover_state) = (state.clone(), state.clone());
        let (toggle_name, hover_name) = (name.clone(), name.clone());
        tagged(
            SharedString::from(format!("legend-{name}")),
            Role::CheckBox,
            &name,
        )
        .aria_toggled(if hidden {
            gpui::Toggled::False
        } else {
            gpui::Toggled::True
        })
        .flex()
        .items_center()
        .gap(px(3.0))
        .text_size(SMALL)
        .text_color(color(if hidden { p.weak } else { p.text }))
        .cursor_pointer()
        .child(
            div()
                .size(px(10.0))
                .rounded_full()
                .bg(color(p.track))
                .flex()
                .items_center()
                .justify_center()
                .children((!hidden).then(|| div().size(px(6.0)).rounded_full().bg(color(l.color)))),
        )
        .child(name.to_string())
        // 凡例はクリックだけを受ける。ドラッグは下のグラフへ渡し、ダブルクリックは渡さない（eguiの`hit_test`）。
        .on_mouse_down(MouseButton::Left, |ev, _, cx| {
            if ev.click_count >= 2 {
                cx.stop_propagation()
            }
        })
        .on_click(move |ev, _, cx| {
            let alt = ev.modifiers().alt;
            toggle_state.update(cx, |s, cx| {
                if s.dragged {
                    return;
                }
                if alt {
                    s.focus(&toggle_name, &names);
                } else {
                    s.toggle(&toggle_name);
                }
                cx.notify();
            })
        })
        .on_hover(move |on, _, cx| {
            hover_state.update(cx, |s, cx| {
                s.legend_hover = on.then(|| hover_name.to_string());
                cx.notify();
            })
        })
    });
    div()
        .absolute()
        .top(px(4.0))
        .right(px(4.0))
        .p(px(4.0))
        .rounded(px(4.0))
        .bg(with_alpha(p.card, 0.75))
        .flex()
        .flex_col()
        .children(entries)
}

impl RenderOnce for Plot {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = self.palette;
        let st = self.state.read(cx);
        let visible: Vec<&Line> = self
            .lines
            .iter()
            .filter(|l| !st.is_hidden(&l.name))
            .collect();
        let series: Vec<&[[f64; 2]]> = visible.iter().map(|l| l.points.as_slice()).collect();
        let auto = auto_range(&series, &self.include_x, &self.include_y);
        let range = st.range.unwrap_or(auto);
        let hover = st.hover;
        let legend_hover = st.legend_hover.clone();
        let near = match (hover, st.current()) {
            (Some(h), Some((r, rc))) => nearest(
                &series,
                &r,
                rc,
                (f32::from(h.x), f32::from(h.y)),
                HOVER_RADIUS,
            )
            .map(|(i, j)| (visible[i].name.clone(), visible[i].points[j])),
            _ => None,
        };
        let tooltip = match (&self.hover_formatter, &near) {
            (Some(f), Some((name, pt))) => Some(f(name, pt[0], pt[1])),
            _ => None,
        };
        let outer = st.outer;
        let draw_lines: Vec<(Vec<[f64; 2]>, u32, f32)> = visible
            .iter()
            .map(|l| {
                let w = if legend_hover.as_deref() == Some(&*l.name) {
                    LINE_WIDTH * 2.0
                } else {
                    LINE_WIDTH
                };
                (l.points.clone(), l.color, w)
            })
            .collect();
        let state = self.state.clone();
        let (vlines, hlines) = (self.vlines.clone(), self.hlines.clone());
        let x_fmt = self.x_formatter.clone();
        let cross = near.as_ref().map(|(_, pt)| *pt);
        let drag_box = st
            .drag
            .filter(|d| d.kind == DragKind::Box && d.moved)
            .map(|d| (d.start, hover.unwrap_or(d.last)));

        let layout_state = self.state.clone();
        let events = state.clone();
        let paint = canvas(
            move |bounds, window, cx| {
                let frame_h = (f32::from(bounds.size.height) - X_AXIS_HEIGHT).max(1.0);
                let y_base = range.height() / frame_h as f64 * GRID_SPACING.0 as f64;
                let y_marks = grid_marks(range.y.0, range.y.1, y_base);
                let y_width = y_marks
                    .iter()
                    .filter(|m| (m.1 / range.height()) as f32 * frame_h > Y_LABEL_SPACING.0)
                    .map(|m| {
                        let t = tick_label(m.0, m.1);
                        let w = window
                            .text_system()
                            .shape_line(t.clone().into(), BODY, &[run(&t, Hsla::default())], None)
                            .width;
                        f32::from(w)
                    })
                    .fold(0.0f32, f32::max);
                let y_axis = (y_width + 2.0 * Y_AXIS_MARGIN).max(Y_AXIS_MIN);
                let frame = Bounds::new(
                    point(bounds.left() + px(y_axis), bounds.top()),
                    size((bounds.size.width - px(y_axis)).max(px(1.0)), px(frame_h)),
                );
                let x_base =
                    range.width() / f32::from(frame.size.width) as f64 * GRID_SPACING.0 as f64;
                let x_marks = grid_marks(range.x.0, range.x.1, x_base);
                layout_state.update(cx, |s, _| {
                    s.shown = Some(range);
                    s.frame = Some(frame);
                    s.outer = Some(bounds);
                });
                Layout {
                    range,
                    frame,
                    x_marks,
                    y_marks,
                }
            },
            move |_, l, window, cx| {
                // 移動と離す操作は窓全体で受ける。グラフの外へ出たホバーを消し、外へ出てもドラッグを続けるため。
                let s = events.clone();
                window.on_mouse_event(move |ev: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble {
                        s.update(cx, |s, cx| {
                            if s.mouse_move(ev) {
                                cx.notify()
                            }
                        });
                    }
                });
                let s = events.clone();
                window.on_mouse_event(move |ev: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble {
                        s.update(cx, |s, cx| {
                            if s.mouse_up(ev) {
                                cx.notify()
                            }
                        });
                    }
                });
                paint_plot(
                    &l,
                    p,
                    &draw_lines,
                    &vlines,
                    &hlines,
                    cross,
                    drag_box,
                    hover,
                    x_fmt.as_deref(),
                    window,
                    cx,
                );
            },
        )
        .absolute()
        .size_full();

        let legend = self
            .legend
            .then(|| legend_box(&self.lines, &self.state, p, cx));

        let tip = tooltip.map(|t| {
            let lines: Vec<&str> = t.lines().collect();
            let tip_size = size(px(150.0), px(18.0 * lines.len() as f32 + 8.0));
            let at = match (outer, hover) {
                (Some(o), Some(h)) => tooltip_origin(o, h, tip_size) - o.origin,
                _ => point(px(0.0), px(0.0)),
            };
            div()
                .absolute()
                .left(at.x)
                .top(at.y)
                .p(px(4.0))
                .rounded(px(4.0))
                .bg(color(p.card))
                .border_1()
                .border_color(color(p.border))
                .child(text(&t, BODY, p.text))
        });

        let (s1, s2, s3) = (state.clone(), state.clone(), state.clone());
        div()
            .id(self.id)
            .relative()
            .w_full()
            .h(self.height)
            .child(paint)
            .children(legend)
            .children(tip)
            .on_mouse_down(MouseButton::Left, move |ev, _, cx| {
                s1.update(cx, |s, cx| {
                    s.mouse_down(ev);
                    cx.notify();
                })
            })
            .on_mouse_down(MouseButton::Right, move |ev, _, cx| {
                s2.update(cx, |s, cx| {
                    s.mouse_down(ev);
                    cx.notify();
                })
            })
            .on_scroll(move |ev, _, cx| {
                // eguiの`Modifiers::COMMAND`は、macOSでも⌘と⌃のどちらでも拡大にする。
                let zoom = ev.modifiers.secondary(cx) || ev.modifiers.control;
                s3.update(cx, |s, cx| {
                    s.scroll(ev, zoom);
                    cx.notify();
                })
            })
            .on_pinch(move |ev, _, cx| {
                state.update(cx, |s, cx| {
                    s.pinch(ev);
                    cx.notify();
                })
            })
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_plot(
    l: &Layout,
    p: &Palette,
    lines: &[(Vec<[f64; 2]>, u32, f32)],
    vlines: &[(f64, u32)],
    hlines: &[(f64, u32)],
    cross: Option<[f64; 2]>,
    drag_box: Option<(Point<Pixels>, Point<Pixels>)>,
    hover: Option<Point<Pixels>>,
    x_fmt: Option<&dyn Fn(f64) -> String>,
    window: &mut Window,
    cx: &mut App,
) {
    let f = l.frame;
    window.paint_quad(fill(f, color(p.card)));
    window.with_content_mask(Some(ContentMask { bounds: f }), |window| {
        paint_grid(l, p, window);
        paint_lines(l, lines, vlines, hlines, window);
        let cursor = cross
            .map(|pt| at(l, pt))
            .or(hover.filter(|h| f.contains(h)));
        paint_cursor(f, p, cursor, drag_box, window);
    });
    window.paint_quad(outline(f, color(p.border), BorderStyle::Solid));
    paint_axis_labels(l, p, x_fmt, window, cx);
}

fn at(l: &Layout, pt: [f64; 2]) -> Point<Pixels> {
    let (x, y) = to_screen(&l.range, rect(l.frame), pt);
    point(px(x), px(y))
}

fn x_spacing(l: &Layout, step: f64) -> f32 {
    (step / l.range.width()) as f32 * f32::from(l.frame.size.width)
}

fn y_spacing(l: &Layout, step: f64) -> f32 {
    (step / l.range.height()) as f32 * f32::from(l.frame.size.height)
}

fn paint_grid(l: &Layout, p: &Palette, window: &mut Window) {
    let f = l.frame;
    for &(v, step) in &l.x_marks {
        if let Some(a) = grid_alpha(x_spacing(l, step)) {
            let x = at(l, [v, l.range.y.0]).x;
            window.paint_quad(fill(
                Bounds::new(point(x, f.top()), size(px(1.0), f.size.height)),
                with_alpha(p.text, a),
            ));
        }
    }
    for &(v, step) in &l.y_marks {
        if let Some(a) = grid_alpha(y_spacing(l, step)) {
            let y = at(l, [l.range.x.0, v]).y;
            window.paint_quad(fill(
                Bounds::new(point(f.left(), y), size(f.size.width, px(1.0))),
                with_alpha(p.text, a),
            ));
        }
    }
}

fn segment(window: &mut Window, a: Point<Pixels>, b: Point<Pixels>, c: Hsla) {
    let mut path = PathBuilder::stroke(px(1.0));
    path.move_to(a);
    path.line_to(b);
    if let Ok(path) = path.build() {
        window.paint_path(path, c);
    }
}

fn paint_lines(
    l: &Layout,
    lines: &[(Vec<[f64; 2]>, u32, f32)],
    vlines: &[(f64, u32)],
    hlines: &[(f64, u32)],
    window: &mut Window,
) {
    for &(x, c) in vlines {
        segment(
            window,
            at(l, [x, l.range.y.0]),
            at(l, [x, l.range.y.1]),
            color(c).into(),
        );
    }
    for &(y, c) in hlines {
        segment(
            window,
            at(l, [l.range.x.0, y]),
            at(l, [l.range.x.1, y]),
            color(c).into(),
        );
    }
    for (pts, c, w) in lines {
        let mut finite = pts
            .iter()
            .filter(|pt| pt[0].is_finite() && pt[1].is_finite());
        let Some(first) = finite.next() else {
            continue;
        };
        let mut path = PathBuilder::stroke(px(*w));
        path.move_to(at(l, *first));
        for pt in finite {
            path.line_to(at(l, *pt));
        }
        if let Ok(path) = path.build() {
            window.paint_path(path, color(*c));
        }
    }
}

fn paint_cursor(
    f: Bounds<Pixels>,
    p: &Palette,
    cursor: Option<Point<Pixels>>,
    drag_box: Option<(Point<Pixels>, Point<Pixels>)>,
    window: &mut Window,
) {
    if let Some(c) = cursor {
        let weak: Hsla = color(p.weak).into();
        segment(window, point(c.x, f.top()), point(c.x, f.bottom()), weak);
        segment(window, point(f.left(), c.y), point(f.right(), c.y), weak);
    }
    if let Some((a, b)) = drag_box {
        let r = Bounds::from_corners(
            point(a.x.min(b.x), a.y.min(b.y)),
            point(a.x.max(b.x), a.y.max(b.y)),
        );
        window.paint_quad(outline(r, color(0x00008b), BorderStyle::Solid).border_widths(px(4.0)));
        window.paint_quad(outline(r, color(0xffffff), BorderStyle::Solid).border_widths(px(2.0)));
    }
}

fn shape(window: &Window, t: &str, c: Hsla) -> ShapedLine {
    window
        .text_system()
        .shape_line(t.to_string().into(), BODY, &[run(t, c)], None)
}

fn paint_axis_labels(
    l: &Layout,
    p: &Palette,
    x_fmt: Option<&dyn Fn(f64) -> String>,
    window: &mut Window,
    cx: &mut App,
) {
    let f = l.frame;
    for &(v, step) in &l.x_marks {
        let t = x_fmt.map_or_else(|| tick_label(v, step), |fmt| fmt(v));
        if t.is_empty() {
            continue;
        }
        let width = f32::from(shape(window, &t, Hsla::default()).width);
        let Some(a) = label_alpha(x_spacing(l, step), width, X_LABEL_SPACING) else {
            continue;
        };
        let line = shape(window, &t, with_alpha(p.text, a));
        let x = at(l, [v, l.range.y.0]).x - line.width / 2.0;
        let _ = line.paint(
            point(x, f.bottom() + px(1.0)),
            px(18.0),
            TextAlign::Left,
            None,
            window,
            cx,
        );
    }
    for &(v, step) in &l.y_marks {
        // y軸は数字の高さ（行の高さ）が間隔に収まるかを見る。
        let Some(a) = label_alpha(y_spacing(l, step), 18.0, Y_LABEL_SPACING) else {
            continue;
        };
        let line = shape(window, &tick_label(v, step), with_alpha(p.text, a));
        let x = f.left() - px(Y_AXIS_MARGIN) - line.width;
        let y = at(l, [l.range.x.0, v]).y - px(9.0);
        let _ = line.paint(point(x, y), px(18.0), TextAlign::Left, None, window, cx);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::probe::{self, Role};
    use crate::views::theme::LIGHT;
    use crate::views::widgets::frame_start;
    use gpui::{
        AppContext as _, Context, Modifiers, MouseButton, MouseDownEvent, Render, ScrollDelta,
        ScrollEvent, TestAppContext, VisualTestContext, Window, div, point, px, size,
    };

    struct View {
        state: Entity<PlotState>,
    }

    impl Render for View {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            frame_start();
            div().w(px(400.0)).h(px(300.0)).child(
                Plot::new("p", self.state.clone(), &LIGHT)
                    .height(px(200.0))
                    .legend(true)
                    .include_y(0.0)
                    .include_y(100.0)
                    .x_formatter(|x| format!("x{x}"))
                    .hover_formatter(|name, x, y| format!("{name} {x} {y}"))
                    .line("a", vec![[0.0, 10.0], [10.0, 50.0], [20.0, 90.0]])
                    .line("b", vec![[0.0, 30.0], [20.0, 40.0]])
                    .vline(5.0, LIGHT.weak)
                    .hline(80.0, LIGHT.warn),
            )
        }
    }

    fn open(cx: &mut TestAppContext) -> (Entity<PlotState>, &mut VisualTestContext) {
        let mut state = None;
        let (_, vcx) = cx.add_window_view(|_, cx| {
            let s = cx.new(|_| PlotState::default());
            state = Some(s.clone());
            View { state: s }
        });
        vcx.run_until_parked();
        (state.unwrap(), vcx)
    }

    fn shown(state: &Entity<PlotState>, vcx: &mut VisualTestContext) -> Range2 {
        state.read_with(vcx, |s, _| s.shown().expect("描いた範囲"))
    }

    fn frame_center(state: &Entity<PlotState>, vcx: &mut VisualTestContext) -> Point<Pixels> {
        state.read_with(vcx, |s, _| s.frame().expect("描いた枠").center())
    }

    fn mouse_down(
        vcx: &mut VisualTestContext,
        at: Point<Pixels>,
        button: MouseButton,
        clicks: usize,
    ) {
        vcx.simulate_event(MouseDownEvent {
            button,
            position: at,
            modifiers: Modifiers::default(),
            click_count: clicks,
            first_mouse: false,
        });
    }

    #[test]
    fn auto_range_covers_lines_and_include_values() {
        let mut cx = crate::test_support::gpui_cx();
        let (state, vcx) = open(&mut cx);
        let r = shown(&state, vcx);
        assert!(
            r.x.0 < 0.0 && r.x.1 > 20.0 && r.y.0 < 0.0 && r.y.1 > 100.0,
            "{r:?}"
        );
        assert!(state.read_with(vcx, |s, _| s.is_auto()));
    }

    #[test]
    fn plot_drag_pans_and_double_click_restores_auto_range() {
        let mut cx = crate::test_support::gpui_cx();
        let (state, vcx) = open(&mut cx);
        let before = shown(&state, vcx);
        let c = frame_center(&state, vcx);
        mouse_down(vcx, c, MouseButton::Left, 1);
        vcx.simulate_mouse_move(
            c + point(px(40.0), px(0.0)),
            Some(MouseButton::Left),
            Modifiers::default(),
        );
        vcx.simulate_mouse_up(
            c + point(px(40.0), px(0.0)),
            MouseButton::Left,
            Modifiers::default(),
        );
        vcx.run_until_parked();
        let moved = shown(&state, vcx);
        assert!(
            moved.x.0 < before.x.0,
            "右へドラッグすると範囲は左へずれる: {before:?} -> {moved:?}"
        );
        assert!((moved.width() - before.width()).abs() < 1e-9);
        assert!(!state.read_with(vcx, |s, _| s.is_auto()));
        mouse_down(vcx, c, MouseButton::Left, 2);
        vcx.simulate_mouse_up(c, MouseButton::Left, Modifiers::default());
        vcx.run_until_parked();
        assert!(state.read_with(vcx, |s, _| s.is_auto()));
        assert_eq!(shown(&state, vcx), before);
    }

    #[test]
    fn plot_cmd_scroll_zooms_at_cursor_and_plain_scroll_pans() {
        let mut cx = crate::test_support::gpui_cx();
        let (state, vcx) = open(&mut cx);
        let before = shown(&state, vcx);
        let c = frame_center(&state, vcx);
        vcx.simulate_mouse_move(c, None, Modifiers::default());
        let zoom_key = vcx.update(|_, cx| Modifiers::secondary_key(cx));
        vcx.simulate_event(ScrollEvent {
            position: c,
            delta: ScrollDelta::Pixels(point(px(0.0), px(100.0))),
            modifiers: zoom_key,
            ..Default::default()
        });
        vcx.run_until_parked();
        let zoomed = shown(&state, vcx);
        assert!(
            zoomed.width() < before.width() && zoomed.height() < before.height(),
            "{zoomed:?}"
        );
        vcx.simulate_event(ScrollEvent {
            position: c,
            delta: ScrollDelta::Pixels(point(px(30.0), px(0.0))),
            ..Default::default()
        });
        vcx.run_until_parked();
        let panned = shown(&state, vcx);
        assert!((panned.width() - zoomed.width()).abs() < 1e-9 && panned.x != zoomed.x);
    }

    #[test]
    fn plot_right_drag_box_zooms() {
        let mut cx = crate::test_support::gpui_cx();
        let (state, vcx) = open(&mut cx);
        let before = shown(&state, vcx);
        let c = frame_center(&state, vcx);
        let end = c + point(px(40.0), px(30.0));
        mouse_down(vcx, c, MouseButton::Right, 1);
        vcx.simulate_mouse_move(
            c + point(px(7.0), px(0.0)),
            Some(MouseButton::Right),
            Modifiers::default(),
        );
        vcx.simulate_mouse_move(end, Some(MouseButton::Right), Modifiers::default());
        vcx.simulate_mouse_up(end, MouseButton::Right, Modifiers::default());
        vcx.run_until_parked();
        let boxed = shown(&state, vcx);
        assert!(
            boxed.width() < before.width() / 2.0 && boxed.height() < before.height() / 2.0,
            "{boxed:?}"
        );
    }

    #[test]
    fn axis_drag_zooms_only_that_axis() {
        let mut cx = crate::test_support::gpui_cx();
        let (state, vcx) = open(&mut cx);
        let before = shown(&state, vcx);
        let f = state.read_with(vcx, |s, _| s.frame().unwrap());
        let on_x_axis = point(f.center().x, f.bottom() + px(8.0));
        mouse_down(vcx, on_x_axis, MouseButton::Left, 1);
        vcx.simulate_mouse_move(
            on_x_axis + point(px(10.0), px(0.0)),
            Some(MouseButton::Left),
            Modifiers::default(),
        );
        vcx.simulate_mouse_up(
            on_x_axis + point(px(10.0), px(0.0)),
            MouseButton::Left,
            Modifiers::default(),
        );
        vcx.run_until_parked();
        let z = shown(&state, vcx);
        assert!(z.width() < before.width(), "{before:?} -> {z:?}");
        assert_eq!(z.y, before.y);
    }

    #[test]
    fn legend_click_hides_series_and_lists_only_named_lines() {
        let mut cx = crate::test_support::gpui_cx();
        let (state, vcx) = open(&mut cx);
        let seen = probe::take();
        assert!(seen.contains(&(Role::CheckBox, "a".into())));
        assert!(seen.contains(&(Role::CheckBox, "b".into())));
        assert_eq!(
            seen.iter().filter(|(r, _)| *r == Role::CheckBox).count(),
            2,
            "縦横の目印の線は凡例に出さない"
        );
        let b = vcx.debug_bounds("checkbox:a").expect("凡例の項目");
        vcx.simulate_click(b.center(), Modifiers::default());
        vcx.run_until_parked();
        assert!(state.read_with(vcx, |s, _| s.is_hidden("a")));
        // aを隠すと、自動の範囲はbとincludeの値だけを含む。
        let r = shown(&state, vcx);
        assert!(r.x.1 < 22.0 && r.y.0 < 0.0, "{r:?}");
    }

    #[test]
    fn hover_shows_formatted_label_near_a_point_only() {
        let mut cx = crate::test_support::gpui_cx();
        let (state, vcx) = open(&mut cx);
        let at = state.read_with(vcx, |s, _| s.screen_of([10.0, 50.0]).unwrap());
        vcx.simulate_mouse_move(at, None, Modifiers::default());
        vcx.run_until_parked();
        assert!(probe::take().contains(&(Role::Label, "a 10 50".into())));
        vcx.simulate_mouse_move(at + point(px(30.0), px(30.0)), None, Modifiers::default());
        vcx.run_until_parked();
        assert!(
            !probe::take()
                .iter()
                .any(|(_, l)| l.starts_with("a ") || l.starts_with("b "))
        );
    }

    #[test]
    fn hover_clears_when_the_pointer_leaves_the_plot() {
        let mut cx = crate::test_support::gpui_cx();
        let (state, vcx) = open(&mut cx);
        let at = state.read_with(vcx, |s, _| s.screen_of([10.0, 50.0]).unwrap());
        vcx.simulate_mouse_move(at, None, Modifiers::default());
        vcx.run_until_parked();
        probe::take();
        // 窓の中で、グラフ（高さ200）より下へ出る。
        vcx.simulate_mouse_move(point(at.x, px(280.0)), None, Modifiers::default());
        vcx.run_until_parked();
        assert!(!probe::take().contains(&(Role::Label, "a 10 50".into())));
        assert_eq!(state.read_with(vcx, |s, _| s.hover), None);
    }

    #[test]
    fn pan_keeps_following_the_pointer_outside_the_plot() {
        let mut cx = crate::test_support::gpui_cx();
        let (state, vcx) = open(&mut cx);
        let before = shown(&state, vcx);
        let c = frame_center(&state, vcx);
        mouse_down(vcx, c, MouseButton::Left, 1);
        let inside = c + point(px(20.0), px(0.0));
        vcx.simulate_mouse_move(inside, Some(MouseButton::Left), Modifiers::default());
        vcx.run_until_parked();
        let mid = shown(&state, vcx);
        let outside = point(inside.x + px(40.0), px(280.0));
        vcx.simulate_mouse_move(outside, Some(MouseButton::Left), Modifiers::default());
        vcx.simulate_mouse_up(outside, MouseButton::Left, Modifiers::default());
        vcx.run_until_parked();
        let after = shown(&state, vcx);
        assert!(mid.x.0 < before.x.0, "{before:?} {mid:?}");
        assert!(after.x.0 < mid.x.0, "外でも動き続ける: {mid:?} {after:?}");
        assert!(
            state.read_with(vcx, |s, _| s.drag.is_none()),
            "外で離したら終わる"
        );
    }

    #[test]
    fn tooltip_stays_inside_the_plot() {
        let outer = Bounds::new(point(px(0.0), px(0.0)), size(px(200.0), px(100.0)));
        let tip = size(px(80.0), px(50.0));
        assert_eq!(
            tooltip_origin(outer, point(px(10.0), px(10.0)), tip),
            point(px(22.0), px(22.0))
        );
        assert_eq!(
            tooltip_origin(outer, point(px(190.0), px(90.0)), tip),
            point(px(120.0), px(50.0))
        );
    }

    #[test]
    fn click_with_small_jitter_keeps_the_auto_range() {
        let mut cx = crate::test_support::gpui_cx();
        let (state, vcx) = open(&mut cx);
        let c = frame_center(&state, vcx);
        let j = c + point(px(2.0), px(1.0));
        mouse_down(vcx, c, MouseButton::Left, 1);
        vcx.simulate_mouse_move(j, Some(MouseButton::Left), Modifiers::default());
        vcx.simulate_mouse_up(j, MouseButton::Left, Modifiers::default());
        vcx.run_until_parked();
        assert!(
            state.read_with(vcx, |s, _| s.is_auto()),
            "2pxの手ぶれで自動の範囲から外れた"
        );
    }

    #[test]
    fn drag_from_the_legend_pans_the_plot_without_toggling() {
        let mut cx = crate::test_support::gpui_cx();
        let (state, vcx) = open(&mut cx);
        let before = shown(&state, vcx);
        let b = vcx.debug_bounds("checkbox:a").expect("legend");
        let end = b.center() - point(px(80.0), px(0.0));
        mouse_down(vcx, b.center(), MouseButton::Left, 1);
        vcx.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
        vcx.run_until_parked();
        assert_ne!(shown(&state, vcx), before, "凡例から動かすとグラフが動く");
        vcx.simulate_mouse_move(b.center(), Some(MouseButton::Left), Modifiers::default());
        vcx.simulate_mouse_up(b.center(), MouseButton::Left, Modifiers::default());
        vcx.run_until_parked();
        assert!(
            !state.read_with(vcx, |s, _| s.is_hidden("a")),
            "動かした後に同じ項目で離しても隠さない"
        );
    }

    #[test]
    fn double_click_on_the_legend_keeps_the_zoom() {
        let mut cx = crate::test_support::gpui_cx();
        let (state, vcx) = open(&mut cx);
        let c = frame_center(&state, vcx);
        mouse_down(vcx, c, MouseButton::Left, 1);
        vcx.simulate_mouse_move(
            c + point(px(40.0), px(0.0)),
            Some(MouseButton::Left),
            Modifiers::default(),
        );
        vcx.simulate_mouse_up(
            c + point(px(40.0), px(0.0)),
            MouseButton::Left,
            Modifiers::default(),
        );
        vcx.run_until_parked();
        assert!(!state.read_with(vcx, |s, _| s.is_auto()));
        let b = vcx.debug_bounds("checkbox:a").expect("legend");
        mouse_down(vcx, b.center(), MouseButton::Left, 1);
        vcx.simulate_mouse_up(b.center(), MouseButton::Left, Modifiers::default());
        mouse_down(vcx, b.center(), MouseButton::Left, 2);
        vcx.simulate_mouse_up(b.center(), MouseButton::Left, Modifiers::default());
        vcx.run_until_parked();
        assert!(
            !state.read_with(vcx, |s, _| s.is_auto()),
            "凡例のダブルクリックで範囲が戻った"
        );
    }

    #[test]
    fn double_click_on_the_x_axis_keeps_the_zoom() {
        let mut cx = crate::test_support::gpui_cx();
        let (state, vcx) = open(&mut cx);
        let c = frame_center(&state, vcx);
        mouse_down(vcx, c, MouseButton::Left, 1);
        vcx.simulate_mouse_move(
            c + point(px(40.0), px(0.0)),
            Some(MouseButton::Left),
            Modifiers::default(),
        );
        vcx.simulate_mouse_up(
            c + point(px(40.0), px(0.0)),
            MouseButton::Left,
            Modifiers::default(),
        );
        vcx.run_until_parked();
        let f = state.read_with(vcx, |s, _| s.frame().unwrap());
        let ax = point(f.center().x, f.bottom() + px(8.0));
        mouse_down(vcx, ax, MouseButton::Left, 2);
        vcx.simulate_mouse_up(ax, MouseButton::Left, Modifiers::default());
        vcx.run_until_parked();
        assert!(
            !state.read_with(vcx, |s, _| s.is_auto()),
            "x軸のダブルクリックで範囲が戻った"
        );
    }

    #[test]
    fn one_wheel_line_pans_forty_points() {
        let mut cx = crate::test_support::gpui_cx();
        let (state, vcx) = open(&mut cx);
        let before = shown(&state, vcx);
        let f = state.read_with(vcx, |s, _| s.frame().unwrap());
        let c = f.center();
        vcx.simulate_mouse_move(c, None, Modifiers::default());
        vcx.simulate_event(ScrollEvent {
            position: c,
            delta: ScrollDelta::Lines(point(0.0, 1.0)),
            ..Default::default()
        });
        vcx.run_until_parked();
        let after = shown(&state, vcx);
        let moved_px =
            (after.y.0 - before.y.0) / before.height() * f64::from(f32::from(f.size.height));
        assert!(
            (moved_px.abs() - 40.0).abs() < 0.5,
            "ホイール1行で{moved_px}pt動いた（40ptのはず）"
        );
    }

    #[test]
    fn trackpad_pinch_zooms_in() {
        let mut cx = crate::test_support::gpui_cx();
        let (state, vcx) = open(&mut cx);
        let before = shown(&state, vcx);
        let c = frame_center(&state, vcx);
        vcx.simulate_mouse_move(c, None, Modifiers::default());
        vcx.simulate_event(gpui::PinchEvent {
            position: c,
            delta: 0.5,
            ..Default::default()
        });
        vcx.run_until_parked();
        let after = shown(&state, vcx);
        assert!(
            after.width() < before.width(),
            "ピンチで拡大しない: {before:?} -> {after:?}"
        );
    }

    #[test]
    fn alt_click_on_the_legend_shows_only_that_series() {
        let mut cx = crate::test_support::gpui_cx();
        let (state, vcx) = open(&mut cx);
        let b = vcx.debug_bounds("checkbox:a").expect("legend");
        let alt = Modifiers {
            alt: true,
            ..Default::default()
        };
        vcx.simulate_click(b.center(), alt);
        vcx.run_until_parked();
        let (a, bb) = state.read_with(vcx, |s, _| (s.is_hidden("a"), s.is_hidden("b")));
        assert!(!a && bb, "⌥クリックでaだけが残らない: a={a} b={bb}");
    }

    #[test]
    fn cmd_shift_scroll_zooms_only_the_x_axis() {
        let mut cx = crate::test_support::gpui_cx();
        let (state, vcx) = open(&mut cx);
        let before = shown(&state, vcx);
        let c = frame_center(&state, vcx);
        vcx.simulate_mouse_move(c, None, Modifiers::default());
        let mut m = vcx.update(|_, cx| Modifiers::secondary_key(cx));
        m.shift = true;
        vcx.simulate_event(ScrollEvent {
            position: c,
            delta: ScrollDelta::Pixels(point(px(0.0), px(100.0))),
            modifiers: m,
            ..Default::default()
        });
        vcx.run_until_parked();
        let z = shown(&state, vcx);
        assert!(
            z.width() < before.width() && z.y == before.y,
            "⌘⇧スクロールでx軸だけが拡大しない: {before:?} -> {z:?}"
        );
    }

    #[test]
    fn tiny_right_drag_does_not_box_zoom() {
        let mut cx = crate::test_support::gpui_cx();
        let (state, vcx) = open(&mut cx);
        let before = shown(&state, vcx);
        let c = frame_center(&state, vcx);
        let e = c + point(px(3.0), px(2.0));
        mouse_down(vcx, c, MouseButton::Right, 1);
        vcx.simulate_mouse_move(e, Some(MouseButton::Right), Modifiers::default());
        vcx.simulate_mouse_up(e, MouseButton::Right, Modifiers::default());
        vcx.run_until_parked();
        let z = shown(&state, vcx);
        assert!(
            state.read_with(vcx, |s, _| s.is_auto()),
            "3x2pxの右ドラッグで拡大した: {before:?} -> {z:?}"
        );
    }
}
