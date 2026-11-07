//! グラフの操作の状態と、操作で範囲を動かす計算。描画はelement.rsに置く。
use crate::views::plot::math::{self, Range2, Rect, to_screen};
use gpui::{
    Bounds, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PinchEvent,
    Pixels, Point, ScrollEvent, point, px,
};
use std::collections::HashSet;

/// ホバーのツールチップとポインタの間。
const TOOLTIP_GAP: f32 = 12.0;
/// クリックとみなす動きの上限。これを超えて動くまで、移動と矩形の拡大を始めない（eguiの`max_click_dist`）。
const MAX_CLICK_DIST: f32 = 6.0;
/// ホイール1行の量（eguiの`line_scroll_speed`）。
const LINE_SCROLL: f32 = 40.0;
/// 修飾キー付きのスクロールで、e倍の拡大になる量（eguiの`scroll_zoom_speed`の逆数）。
const SCROLL_ZOOM_PER_E: f64 = 200.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum DragKind {
    Pan,
    Axis(usize),
    Box,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Drag {
    pub(crate) kind: DragKind,
    pub(crate) start: Point<Pixels>,
    pub(crate) last: Point<Pixels>,
    /// クリックの範囲を超えて動いたか。軸のドラッグは押したときから動いたものとする。
    pub(crate) moved: bool,
}

/// 1つのグラフの状態。描画をまたいで持つ。
#[derive(Debug, Default)]
pub struct PlotState {
    /// 利用者が動かした範囲。`None`は自動。
    pub(crate) range: Option<Range2>,
    pub(crate) hidden: HashSet<String>,
    pub(crate) hover: Option<Point<Pixels>>,
    pub(crate) legend_hover: Option<String>,
    pub(crate) drag: Option<Drag>,
    /// 今の押下がドラッグになったか。凡例は、ドラッグの後に離した操作をクリックにしない。
    pub(crate) dragged: bool,
    /// 前の描画の範囲と枠。操作はこれを基準に範囲を動かす。
    pub(crate) shown: Option<Range2>,
    pub(crate) frame: Option<Bounds<Pixels>>,
    pub(crate) outer: Option<Bounds<Pixels>>,
}

impl PlotState {
    /// 前の描画の範囲。
    pub fn shown(&self) -> Option<Range2> {
        self.shown
    }

    /// 前の描画の枠（軸を除いた、線を描く所）。
    pub fn frame(&self) -> Option<Bounds<Pixels>> {
        self.frame
    }

    /// 範囲が自動か。
    pub fn is_auto(&self) -> bool {
        self.range.is_none()
    }

    /// 系列を隠しているか。
    pub fn is_hidden(&self, name: &str) -> bool {
        self.hidden.contains(name)
    }

    /// データの点の画面の位置。
    pub fn screen_of(&self, p: [f64; 2]) -> Option<Point<Pixels>> {
        let (r, f) = (self.shown?, self.frame?);
        let (x, y) = to_screen(&r, rect(f), p);
        Some(point(px(x), px(y)))
    }

    pub(crate) fn current(&self) -> Option<(Range2, Rect)> {
        Some((self.shown?, rect(self.frame?)))
    }

    fn set(&mut self, r: Range2) {
        self.range = Some(r);
        self.shown = Some(r);
    }

    pub(crate) fn mouse_down(&mut self, ev: &MouseDownEvent) {
        let Some(f) = self.frame else { return };
        let at = ev.position;
        self.dragged = false;
        if ev.button == MouseButton::Left && ev.click_count >= 2 && f.contains(&at) {
            self.range = None;
            self.drag = None;
            return;
        }
        // 軸の領域は枠と同じ幅（x軸）と高さ（y軸）で、左下の角はどちらでもない。
        let in_x = at.x >= f.left() && at.x < f.right();
        let in_y = at.y >= f.top() && at.y < f.bottom();
        let kind = match ev.button {
            MouseButton::Right if f.contains(&at) => DragKind::Box,
            MouseButton::Left if at.y > f.bottom() && in_x => DragKind::Axis(0),
            MouseButton::Left if at.x < f.left() && in_y => DragKind::Axis(1),
            MouseButton::Left if f.contains(&at) => DragKind::Pan,
            _ => return,
        };
        self.drag = Some(Drag {
            kind,
            start: at,
            last: at,
            moved: matches!(kind, DragKind::Axis(_)),
        });
    }

    /// 窓のどこで動いても呼ばれる。表示が変わったかを返す。
    pub(crate) fn mouse_move(&mut self, ev: &MouseMoveEvent) -> bool {
        let before = (self.hover, self.drag, self.range);
        self.track(ev);
        before != (self.hover, self.drag, self.range)
    }

    fn track(&mut self, ev: &MouseMoveEvent) {
        self.hover = self
            .outer
            .filter(|o| o.contains(&ev.position))
            .map(|_| ev.position);
        let (Some(mut d), Some((r, rc))) = (self.drag, self.current()) else {
            return;
        };
        if ev.pressed_button.is_none() {
            self.drag = None;
            return;
        }
        if !d.moved {
            d.moved = beyond_click(d.start, ev.position);
            if !d.moved {
                d.last = ev.position;
                self.drag = Some(d);
                return;
            }
            self.dragged = true;
            // 矩形の始点は、ドラッグと決まった時点の位置（egui_plotの`drag_started`の`hover_pos`）。
            if d.kind == DragKind::Box {
                d.start = ev.position;
            }
        }
        let delta = ev.position - d.last;
        let (dx, dy) = (f32::from(delta.x), f32::from(delta.y));
        match d.kind {
            DragKind::Pan => self.set(math::pan(&r, rc, (dx, dy))),
            DragKind::Axis(axis) => {
                let step = if axis == 0 { dx } else { dy };
                let z = 1.0 + (0.02 * step as f64).clamp(-1.0, 1.0);
                let f = self.frame.unwrap_or_default();
                let mut anchor = (f32::from(f.center().x), f32::from(f.center().y));
                let factor = if axis == 0 {
                    anchor.0 = f32::from(d.start.x);
                    (z, 1.0)
                } else {
                    anchor.1 = f32::from(d.start.y);
                    (1.0, z)
                };
                self.set(math::zoom(&r, rc, anchor, factor));
            }
            DragKind::Box => {}
        }
        d.last = ev.position;
        self.drag = Some(d);
    }

    /// 窓のどこで離しても呼ばれる。ドラッグ中だったかを返す。
    pub(crate) fn mouse_up(&mut self, ev: &MouseUpEvent) -> bool {
        let Some(d) = self.drag.take() else {
            return false;
        };
        if d.kind != DragKind::Box || ev.button != MouseButton::Right || !d.moved {
            return true;
        }
        let Some((r, rc)) = self.current() else {
            return true;
        };
        let corner = |p: Point<Pixels>| (f32::from(p.x), f32::from(p.y));
        if let Some(b) = math::zoom_to_box(&r, rc, corner(d.start), corner(ev.position)) {
            self.set(b);
        }
        true
    }

    /// ホイール。`zoom`（⌘）なら拡大し、そうでなければ動かす。⇧は横だけ、⌥は縦だけにする。
    pub(crate) fn scroll(&mut self, ev: &ScrollEvent, zoom: bool) {
        let Some((r, rc)) = self.current() else {
            return;
        };
        let d = ev.delta.pixel_delta(px(LINE_SCROLL));
        let (dx, dy) = (f32::from(d.x), f32::from(d.y));
        let axis = only_axis(&ev.modifiers);
        let (dx, dy) = match axis {
            Some(0) => (dx + dy, 0.0),
            Some(_) => (0.0, dx + dy),
            None => (dx, dy),
        };
        if zoom {
            let z = (f64::from(dx + dy) / SCROLL_ZOOM_PER_E).exp();
            self.zoom_at(ev.position, z, axis);
        } else {
            self.set(math::pan(&r, rc, (dx, dy)));
        }
    }

    /// トラックパッドのピンチ。広げると`exp(delta)`倍に拡大する。⇧と⌥はホイールと同じ。
    pub(crate) fn pinch(&mut self, ev: &PinchEvent) {
        self.zoom_at(
            ev.position,
            f64::from(ev.delta).exp(),
            only_axis(&ev.modifiers),
        );
    }

    fn zoom_at(&mut self, at: Point<Pixels>, z: f64, axis: Option<usize>) {
        let Some((r, rc)) = self.current() else {
            return;
        };
        let factor = match axis {
            Some(0) => (z, 1.0),
            Some(_) => (1.0, z),
            None => (z, z),
        };
        self.set(math::zoom(
            &r,
            rc,
            (f32::from(at.x), f32::from(at.y)),
            factor,
        ));
    }

    pub(crate) fn toggle(&mut self, name: &str) {
        if !self.hidden.remove(name) {
            self.hidden.insert(name.to_string());
        }
    }

    /// 凡例の⌥クリック。ほかの系列がすべて隠れていれば全部を出し、そうでなければ`name`だけを残す。
    pub(crate) fn focus(&mut self, name: &str, names: &[String]) {
        let others = || names.iter().filter(|n| n.as_str() != name);
        if others().all(|n| self.hidden.contains(n)) {
            self.hidden.clear();
        } else {
            self.hidden = others().cloned().collect();
        }
    }
}

fn beyond_click(a: Point<Pixels>, b: Point<Pixels>) -> bool {
    let off = b - a;
    f32::from(off.x).hypot(f32::from(off.y)) > MAX_CLICK_DIST
}

/// ⇧だけなら横（0）、⌥だけなら縦（1）。
fn only_axis(m: &Modifiers) -> Option<usize> {
    match (m.shift, m.alt) {
        (true, false) => Some(0),
        (false, true) => Some(1),
        _ => None,
    }
}

pub(crate) fn rect(b: Bounds<Pixels>) -> Rect {
    Rect {
        left: f32::from(b.left()),
        top: f32::from(b.top()),
        width: f32::from(b.size.width),
        height: f32::from(b.size.height),
    }
}

/// ツールチップの左上。ポインタの右下に置き、グラフからはみ出すときは内側へ寄せる。
pub fn tooltip_origin(
    outer: Bounds<Pixels>,
    at: Point<Pixels>,
    tip: gpui::Size<Pixels>,
) -> Point<Pixels> {
    let gap = px(TOOLTIP_GAP);
    let x = (at.x + gap)
        .min(outer.right() - tip.width)
        .max(outer.left());
    let y = (at.y + gap)
        .min(outer.bottom() - tip.height)
        .max(outer.top());
    point(x, y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Modifiers, size};

    const R: Range2 = Range2 {
        x: (0.0, 10.0),
        y: (0.0, 100.0),
    };

    /// 線を描く枠は(10,20)から幅200、高さ100。左端10、下端120。
    fn plot() -> PlotState {
        PlotState {
            shown: Some(R),
            frame: Some(Bounds::new(
                point(px(10.0), px(20.0)),
                size(px(200.0), px(100.0)),
            )),
            outer: Some(Bounds::new(
                point(px(0.0), px(0.0)),
                size(px(220.0), px(140.0)),
            )),
            ..Default::default()
        }
    }

    fn down(s: &mut PlotState, button: MouseButton, x: f32, y: f32) -> Option<DragKind> {
        s.mouse_down(&MouseDownEvent {
            button,
            position: point(px(x), px(y)),
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        });
        s.drag.map(|d| d.kind)
    }

    fn drag_to(s: &mut PlotState, x: f32, y: f32) {
        s.mouse_move(&MouseMoveEvent {
            position: point(px(x), px(y)),
            pressed_button: Some(MouseButton::Left),
            modifiers: Modifiers::default(),
        });
    }

    fn up(s: &mut PlotState, button: MouseButton, x: f32, y: f32) -> bool {
        s.mouse_up(&MouseUpEvent {
            button,
            position: point(px(x), px(y)),
            modifiers: Modifiers::default(),
            click_count: 1,
        })
    }

    #[test]
    fn mouse_down_picks_the_drag_by_button_and_place() {
        let at = |b, x, y| down(&mut plot(), b, x, y);
        assert_eq!(at(MouseButton::Right, 100.0, 50.0), Some(DragKind::Box));
        assert_eq!(at(MouseButton::Right, 5.0, 50.0), None);
        assert_eq!(at(MouseButton::Left, 100.0, 130.0), Some(DragKind::Axis(0)));
        assert_eq!(at(MouseButton::Left, 5.0, 50.0), Some(DragKind::Axis(1)));
        assert_eq!(at(MouseButton::Left, 100.0, 50.0), Some(DragKind::Pan));
        assert_eq!(at(MouseButton::Left, 100.0, 10.0), None);
        // 枠の左端と下端の線の上。左端は枠の中、下端は枠の外で軸でもない。
        assert_eq!(at(MouseButton::Left, 10.0, 50.0), Some(DragKind::Pan));
        assert_eq!(at(MouseButton::Left, 100.0, 120.0), None);
        // 左下の角はどちらの軸でもない。
        assert_eq!(at(MouseButton::Left, 5.0, 130.0), None);
    }

    fn scroll(s: &mut PlotState, dx: f32, dy: f32, shift: bool, alt: bool, zoom: bool) {
        let modifiers = Modifiers {
            shift,
            alt,
            ..Default::default()
        };
        s.scroll(
            &ScrollEvent {
                position: point(px(110.0), px(70.0)),
                delta: gpui::ScrollDelta::Pixels(point(px(dx), px(dy))),
                modifiers,
                ..Default::default()
            },
            zoom,
        );
    }

    #[test]
    fn wheel_moves_both_axes_shift_only_x_and_alt_only_y() {
        let rc = rect(plot().frame.unwrap());
        let moved = |shift, alt| {
            let mut s = plot();
            scroll(&mut s, 3.0, 4.0, shift, alt, false);
            s.range
        };
        assert_eq!(moved(false, false), Some(math::pan(&R, rc, (3.0, 4.0))));
        assert_eq!(moved(true, false), Some(math::pan(&R, rc, (7.0, 0.0))));
        assert_eq!(moved(false, true), Some(math::pan(&R, rc, (0.0, 7.0))));
        assert_eq!(moved(true, true), Some(math::pan(&R, rc, (3.0, 4.0))));
    }

    #[test]
    fn zoom_wheel_scales_by_e_per_200_points_on_the_chosen_axes() {
        let rc = rect(plot().frame.unwrap());
        let at = (110.0, 70.0);
        let z = (100.0_f64 / 200.0).exp();
        let zoomed = |shift, alt| {
            let mut s = plot();
            scroll(&mut s, 40.0, 60.0, shift, alt, true);
            s.range
        };
        assert_eq!(zoomed(false, false), Some(math::zoom(&R, rc, at, (z, z))));
        assert_eq!(zoomed(true, false), Some(math::zoom(&R, rc, at, (z, 1.0))));
        assert_eq!(zoomed(false, true), Some(math::zoom(&R, rc, at, (1.0, z))));
    }

    #[test]
    fn pinch_scales_by_exp_of_the_magnification() {
        let rc = rect(plot().frame.unwrap());
        let pinch = |alt| {
            let mut s = plot();
            s.pinch(&PinchEvent {
                position: point(px(110.0), px(70.0)),
                delta: 0.5,
                modifiers: Modifiers {
                    alt,
                    ..Default::default()
                },
                ..Default::default()
            });
            s.range
        };
        let z = f64::from(0.5f32).exp();
        assert_eq!(
            pinch(false),
            Some(math::zoom(&R, rc, (110.0, 70.0), (z, z)))
        );
        assert_eq!(
            pinch(true),
            Some(math::zoom(&R, rc, (110.0, 70.0), (1.0, z)))
        );
    }

    #[test]
    fn pan_starts_only_beyond_six_points_and_skips_the_slack() {
        let rc = rect(plot().frame.unwrap());
        let mut s = plot();
        down(&mut s, MouseButton::Left, 100.0, 70.0);
        drag_to(&mut s, 106.0, 70.0);
        assert_eq!(s.range, None, "ちょうど6ptはクリック");
        drag_to(&mut s, 108.0, 70.0);
        assert_eq!(s.range, Some(math::pan(&R, rc, (2.0, 0.0))));
    }

    #[test]
    fn double_click_resets_only_inside_the_frame() {
        let dbl = |x: f32, y: f32| {
            let mut s = plot();
            s.range = Some(R);
            s.mouse_down(&MouseDownEvent {
                button: MouseButton::Left,
                position: point(px(x), px(y)),
                modifiers: Modifiers::default(),
                click_count: 2,
                first_mouse: false,
            });
            s.is_auto()
        };
        assert!(dbl(100.0, 50.0));
        assert!(!dbl(100.0, 130.0));
    }

    #[test]
    fn alt_click_keeps_only_that_series_then_shows_all_again() {
        let names = ["a".to_string(), "b".to_string(), "c".to_string()];
        let mut s = plot();
        s.focus("a", &names);
        assert!(!s.is_hidden("a") && s.is_hidden("b") && s.is_hidden("c"));
        s.focus("a", &names);
        assert!(s.hidden.is_empty());
    }

    #[test]
    fn pan_drag_moves_the_range_by_the_pointer_delta() {
        let mut s = plot();
        down(&mut s, MouseButton::Left, 100.0, 70.0);
        drag_to(&mut s, 110.0, 75.0);
        let rc = rect(s.frame.unwrap());
        assert_eq!(s.range, Some(math::pan(&R, rc, (10.0, 5.0))));
    }

    #[test]
    fn axis_drag_zooms_that_axis_two_percent_per_point_around_the_start() {
        let mut s = plot();
        down(&mut s, MouseButton::Left, 100.0, 130.0);
        drag_to(&mut s, 110.0, 130.0);
        let rc = rect(s.frame.unwrap());
        let z = 1.0 + 0.02 * 10.0_f64;
        assert_eq!(s.range, Some(math::zoom(&R, rc, (100.0, 70.0), (z, 1.0))));

        let mut s = plot();
        down(&mut s, MouseButton::Left, 5.0, 50.0);
        drag_to(&mut s, 5.0, 60.0);
        assert_eq!(s.range, Some(math::zoom(&R, rc, (110.0, 50.0), (1.0, z))));
    }

    #[test]
    fn box_zooms_only_when_released_with_the_right_button() {
        let mut s = plot();
        down(&mut s, MouseButton::Right, 50.0, 40.0);
        drag_to(&mut s, 150.0, 100.0);
        assert!(up(&mut s, MouseButton::Left, 150.0, 100.0));
        assert_eq!(s.range, None);

        // 始点は6ptを超えて動いた所（50,40から57,40）。
        let mut s = plot();
        down(&mut s, MouseButton::Right, 50.0, 40.0);
        drag_to(&mut s, 57.0, 40.0);
        drag_to(&mut s, 150.0, 100.0);
        assert!(up(&mut s, MouseButton::Right, 150.0, 100.0));
        let rc = rect(s.frame.unwrap());
        assert_eq!(
            s.range,
            math::zoom_to_box(&R, rc, (57.0, 40.0), (150.0, 100.0))
        );
        assert!(s.range.is_some());
    }
}
