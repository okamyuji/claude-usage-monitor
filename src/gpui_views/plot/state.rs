//! グラフの操作の状態と、操作で範囲を動かす計算。描画はelement.rsに置く。
use crate::gpui_views::plot::math::{self, Range2, Rect, to_screen};
use gpui::{
    Bounds, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, ScrollEvent,
    point, px,
};
use std::collections::HashSet;

/// ホバーのツールチップとポインタの間（egui_plotの`gap`）。
const TOOLTIP_GAP: f32 = 12.0;

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
        if ev.button == MouseButton::Left && ev.click_count >= 2 {
            self.range = None;
            self.drag = None;
            return;
        }
        let at = ev.position;
        let kind = match ev.button {
            MouseButton::Right if f.contains(&at) => DragKind::Box,
            MouseButton::Left if at.y > f.bottom() => DragKind::Axis(0),
            MouseButton::Left if at.x < f.left() => DragKind::Axis(1),
            MouseButton::Left if f.contains(&at) => DragKind::Pan,
            _ => return,
        };
        self.drag = Some(Drag {
            kind,
            start: at,
            last: at,
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
        if d.kind != DragKind::Box || ev.button != MouseButton::Right {
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

    pub(crate) fn scroll(&mut self, ev: &ScrollEvent, zoom: bool) {
        let Some((r, rc)) = self.current() else {
            return;
        };
        let d = ev.delta.pixel_delta(px(16.0));
        let (dx, dy) = (f32::from(d.x), f32::from(d.y));
        if zoom {
            // eguiは修飾キー付きのスクロールを、200pxでe倍になる拡大に読み替える（`scroll_zoom_speed`）。
            let f = ((dy as f64) / 200.0).exp();
            let at = (f32::from(ev.position.x), f32::from(ev.position.y));
            self.set(math::zoom(&r, rc, at, (f, f)));
        } else {
            self.set(math::pan(&r, rc, (dx, dy)));
        }
    }

    pub(crate) fn toggle(&mut self, name: &str) {
        if !self.hidden.remove(name) {
            self.hidden.insert(name.to_string());
        }
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
