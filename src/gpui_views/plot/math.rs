//! グラフの座標と範囲の計算。egui_plot 0.38.0と同じ規則にし、GPUIに依存させない。
//! 画面の座標は左上が原点で下へ伸び、データのyは上へ伸びる。

/// 自動範囲の余白（幅に対する割合）。egui_plotの`margin_fraction`の既定値。
pub const MARGIN_FRACTION: f64 = 0.05;

/// データの範囲。各軸は(最小, 最大)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Range2 {
    /// xの範囲。
    pub x: (f64, f64),
    /// yの範囲。
    pub y: (f64, f64),
}

impl Range2 {
    /// xの幅。
    pub fn width(&self) -> f64 {
        self.x.1 - self.x.0
    }

    /// yの高さ。
    pub fn height(&self) -> f64 {
        self.y.1 - self.y.0
    }

    fn is_valid(&self) -> bool {
        valid(self.x) && valid(self.y)
    }
}

/// グラフを描く画面の矩形。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// 左端。
    pub left: f32,
    /// 上端。
    pub top: f32,
    /// 幅。
    pub width: f32,
    /// 高さ。
    pub height: f32,
}

fn finite(a: (f64, f64)) -> bool {
    a.0.is_finite() && a.1.is_finite()
}

fn valid(a: (f64, f64)) -> bool {
    finite(a) && a.1 - a.0 > 0.0
}

/// 値の最小と最大。NaNは`f64::min`と`f64::max`が捨てる。値が無ければ(+∞, -∞)。
fn span(values: impl Iterator<Item = f64>) -> (f64, f64) {
    values.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
        (lo.min(v), hi.max(v))
    })
}

fn with_margin(a: (f64, f64)) -> (f64, f64) {
    let pad = MARGIN_FRACTION * (a.1 - a.0).max(0.0);
    if pad.is_finite() {
        (a.0 - pad, a.1 + pad)
    } else {
        a
    }
}

/// 幅0の軸は中央から`other`の幅（無効なら1）に広げ、無限の軸は(-1, 1)にする（egui_plotの`PlotTransform::new`）。
fn sanitize(a: (f64, f64), other: (f64, f64)) -> (f64, f64) {
    if !finite(a) {
        return (-1.0, 1.0);
    }
    if a.1 - a.0 > 0.0 {
        return a;
    }
    let size = if valid(other) { other.1 - other.0 } else { 1.0 };
    let c = (a.0 + a.1) / 2.0;
    (c - size / 2.0, c + size / 2.0)
}

/// 全系列の点と`include`の値を含む範囲に、両側へ5%の余白を足す。
pub fn auto_range(series: &[&[[f64; 2]]], include_x: &[f64], include_y: &[f64]) -> Range2 {
    let points = || series.iter().flat_map(|s| s.iter());
    let x = with_margin(span(
        points().map(|p| p[0]).chain(include_x.iter().copied()),
    ));
    let y = with_margin(span(
        points().map(|p| p[1]).chain(include_y.iter().copied()),
    ));
    Range2 {
        x: sanitize(x, y),
        y: sanitize(y, x),
    }
}

fn remap(v: f64, from: (f64, f64), to: (f64, f64)) -> f64 {
    to.0 + (v - from.0) / (from.1 - from.0) * (to.1 - to.0)
}

fn screen_x(rect: Rect) -> (f64, f64) {
    (rect.left as f64, (rect.left + rect.width) as f64)
}

fn screen_y(rect: Rect) -> (f64, f64) {
    ((rect.top + rect.height) as f64, rect.top as f64)
}

/// データの点を画面の位置にする。
pub fn to_screen(r: &Range2, rect: Rect, p: [f64; 2]) -> (f32, f32) {
    (
        remap(p[0], r.x, screen_x(rect)) as f32,
        remap(p[1], r.y, screen_y(rect)) as f32,
    )
}

/// 画面の位置をデータの点にする。
pub fn to_data(r: &Range2, rect: Rect, s: (f32, f32)) -> [f64; 2] {
    [
        remap(s.0 as f64, screen_x(rect), r.x),
        remap(s.1 as f64, screen_y(rect), r.y),
    ]
}

/// `anchor`の下のデータの点を動かさずに、各軸を`factor`倍に拡大する。範囲が壊れるときは元のまま。
pub fn zoom(r: &Range2, rect: Rect, anchor: (f32, f32), factor: (f64, f64)) -> Range2 {
    let c = to_data(r, rect, anchor);
    let z = |a: (f64, f64), c: f64, f: f64| (c + (a.0 - c) / f, c + (a.1 - c) / f);
    let out = Range2 {
        x: z(r.x, c[0], factor.0),
        y: z(r.y, c[1], factor.1),
    };
    if out.is_valid() { out } else { *r }
}

/// ドラッグで中身を`delta`（画面の点）だけ動かす。
pub fn pan(r: &Range2, rect: Rect, delta: (f32, f32)) -> Range2 {
    let dx = -(delta.0 as f64) * r.width() / rect.width as f64;
    let dy = (delta.1 as f64) * r.height() / rect.height as f64;
    Range2 {
        x: (r.x.0 + dx, r.x.1 + dx),
        y: (r.y.0 + dy, r.y.1 + dy),
    }
}

/// 画面の2点を対角とする箱の範囲。幅か高さが0なら`None`。
pub fn zoom_to_box(r: &Range2, rect: Rect, a: (f32, f32), b: (f32, f32)) -> Option<Range2> {
    let (p, q) = (to_data(r, rect, a), to_data(r, rect, b));
    let out = Range2 {
        x: (p[0].min(q[0]), p[0].max(q[0])),
        y: (p[1].min(q[1]), p[1].max(q[1])),
    };
    out.is_valid().then_some(out)
}

/// 目盛の(値, 刻み)。egui_plotの`log_grid_spacer(10)`と同じく、`base_step`を10の累乗に切り上げた刻みとその10倍、100倍で刻み、
/// 近すぎる目盛は大きい刻みのほうを残す。`base_step`は線どうしの最小の間隔をデータの単位にした値。
pub fn grid_marks(lo: f64, hi: f64, base_step: f64) -> Vec<(f64, f64)> {
    if base_step.abs() < f64::EPSILON {
        return Vec::new();
    }
    let unit = 10f64.powi(base_step.abs().log10().ceil() as i32);
    let steps = [unit, unit * 10.0, unit * 100.0];
    let mut marks: Vec<(f64, f64)> = steps
        .iter()
        .flat_map(|&step| {
            let first = (lo / step).ceil() as i64;
            let last = (hi / step).ceil() as i64;
            (first..last).map(move |i| (i as f64 * step, step))
        })
        .collect();
    marks.sort_by(|a, b| a.0.total_cmp(&b.0));
    let eps = 0.1 * unit;
    let mut out: Vec<(f64, f64)> = Vec::with_capacity(marks.len());
    for m in marks {
        match out.last_mut() {
            Some(last) if (last.0 - m.0).abs() < eps => {
                if last.1 < m.1 {
                    *last = m;
                }
            }
            _ => out.push(m),
        }
    }
    out
}

/// `i`番目の線の自動の色（0xRRGGBB）。egui_plotの`auto_color`と同じく、色相を黄金比ずつ回したHSV（彩度0.85、明度0.5）を
/// 線形のRGBとみなしてsRGBへ変換する。
pub fn auto_color(i: usize) -> u32 {
    let h = i as f32 * (std::f32::consts::GOLDEN_RATIO - 1.0);
    let [r, g, b] = rgb_from_hsv(h, 0.85, 0.5);
    u32::from_be_bytes([0, gamma_u8(r), gamma_u8(g), gamma_u8(b)])
}

fn rgb_from_hsv(h: f32, s: f32, v: f32) -> [f32; 3] {
    let h = (h.fract() + 1.0).fract();
    let f = h * 6.0 - (h * 6.0).floor();
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let t = v * (1.0 - (1.0 - f) * s);
    match (h * 6.0).floor() as i32 % 6 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

fn gamma_u8(l: f32) -> u8 {
    let x = if l <= 0.0031308 {
        3294.6 * l
    } else {
        269.025 * l.powf(1.0 / 2.4) - 14.025
    };
    (x + 0.5).floor().clamp(0.0, 255.0) as u8
}

/// 間隔`spacing`の濃さ。`min`以下で0、`max`以上で1。
pub fn strength(spacing: f32, min: f32, max: f32) -> f32 {
    ((spacing - min) / (max - min)).clamp(0.0, 1.0)
}

/// 軸の数字の濃さ。間隔`spacing`が`limits`の下限以下か、数字の幅`width`が間隔より広ければ出さない（`None`）。
/// 濃さは間隔で0から1に上げた値の平方根（egui_plotの`add_tick_labels`）。
pub fn label_alpha(spacing: f32, width: f32, limits: (f32, f32)) -> Option<f32> {
    if spacing <= limits.0 || spacing < width {
        return None;
    }
    Some(strength(spacing, limits.0, limits.1).sqrt())
}

/// `cursor`から半径`radius`の中にある点。各系列でいちばん近い点を取り、半径の中にある系列のうち最後のもの（上に描いた系列）を返す。
pub fn nearest(
    series: &[&[[f64; 2]]],
    r: &Range2,
    rect: Rect,
    cursor: (f32, f32),
    radius: f32,
) -> Option<(usize, usize)> {
    let dist_sq = |p: &[f64; 2]| {
        let s = to_screen(r, rect, *p);
        (s.0 - cursor.0).powi(2) + (s.1 - cursor.1).powi(2)
    };
    series
        .iter()
        .enumerate()
        .filter_map(|(i, pts)| {
            pts.iter()
                .enumerate()
                .map(|(j, p)| (j, dist_sq(p)))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(j, d)| (i, j, d))
        })
        .rfind(|(_, _, d)| *d <= radius * radius)
        .map(|(i, j, _)| (i, j))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const RECT: Rect = Rect {
        left: 10.0,
        top: 20.0,
        width: 200.0,
        height: 100.0,
    };

    fn r(x: (f64, f64), y: (f64, f64)) -> Range2 {
        Range2 { x, y }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
    }

    #[test]
    fn auto_range_includes_points_and_include_values_with_five_percent_margin() {
        let pts: &[[f64; 2]] = &[[10.0, 20.0], [30.0, 50.0]];
        let a = auto_range(&[pts], &[0.0], &[0.0, 100.0]);
        // xは0から30の幅30に5%（1.5）、yは0から100の幅100に5%（5）を両側に足す。
        assert_eq!(a, r((-1.5, 31.5), (-5.0, 105.0)));
    }

    #[test]
    fn auto_range_of_no_points_is_minus_one_to_one() {
        assert_eq!(auto_range(&[], &[], &[]), r((-1.0, 1.0), (-1.0, 1.0)));
    }

    #[test]
    fn auto_range_widens_a_thin_axis_around_its_center() {
        let pts: &[[f64; 2]] = &[[5.0, 2.0]];
        // 点1つは幅も高さも0。どちらも他の軸が無効なので1に広げる。
        assert_eq!(auto_range(&[pts], &[], &[]), r((4.5, 5.5), (1.5, 2.5)));
        let flat: &[[f64; 2]] = &[[0.0, 3.0], [10.0, 3.0]];
        // 高さ0の軸は、有効なxの幅（10に余白1を足した11）に広げる。
        assert_eq!(auto_range(&[flat], &[], &[]), r((-0.5, 10.5), (-2.5, 8.5)));
    }

    #[test]
    fn auto_range_ignores_nan_points() {
        let pts: &[[f64; 2]] = &[[f64::NAN, 1.0], [0.0, f64::NAN], [10.0, 5.0], [0.0, 1.0]];
        assert_eq!(auto_range(&[pts], &[], &[]), r((-0.5, 10.5), (0.8, 5.2)));
    }

    #[test]
    fn screen_y_grows_downward_and_corners_map_to_the_rect() {
        let g = r((0.0, 10.0), (0.0, 100.0));
        assert_eq!(to_screen(&g, RECT, [0.0, 0.0]), (10.0, 120.0));
        assert_eq!(to_screen(&g, RECT, [10.0, 100.0]), (210.0, 20.0));
        assert_eq!(to_data(&g, RECT, (110.0, 70.0)), [5.0, 50.0]);
    }

    #[test]
    fn zoom_keeps_anchor_fixed() {
        let g = r((0.0, 10.0), (0.0, 100.0));
        let z = zoom(&g, RECT, (60.0, 70.0), (2.0, 4.0));
        assert_eq!(z, r((1.25, 6.25), (37.5, 62.5)));
    }

    #[test]
    fn zoom_that_would_break_the_range_is_ignored() {
        let g = r((0.0, 10.0), (0.0, 100.0));
        assert_eq!(zoom(&g, RECT, (60.0, 70.0), (f64::INFINITY, 1.0)), g);
        assert_eq!(zoom(&g, RECT, (60.0, 70.0), (0.0, 1.0)), g);
    }

    #[test]
    fn pan_keeps_span_and_follows_the_pointer() {
        let g = r((0.0, 10.0), (0.0, 100.0));
        // 右へ20px、下へ10pxドラッグすると、中身が右下へ動くので、範囲は左と上へずれる。
        assert_eq!(pan(&g, RECT, (20.0, 10.0)), r((-1.0, 9.0), (10.0, 110.0)));
    }

    #[test]
    fn box_zoom_maps_corners_in_any_drag_direction() {
        let g = r((0.0, 10.0), (0.0, 100.0));
        let want = Some(r((2.5, 5.0), (25.0, 75.0)));
        assert_eq!(zoom_to_box(&g, RECT, (60.0, 45.0), (110.0, 95.0)), want);
        assert_eq!(zoom_to_box(&g, RECT, (110.0, 95.0), (60.0, 45.0)), want);
        assert_eq!(
            zoom_to_box(&g, RECT, (60.0, 45.0), (60.0, 95.0)),
            None,
            "幅0の箱は無視する"
        );
    }

    #[test]
    fn grid_marks_match_egui_plot_generate_marks() {
        // egui_plot 0.38.0 grid.rsのtest_generate_marksと同じ入力と期待値。
        // 最小の間隔0.005は10の累乗に切り上げて0.01になり、刻みは0.01、0.1、1になる。
        let marks = grid_marks(2.855, 3.015, 0.005);
        let want = [
            (2.86, 0.01),
            (2.87, 0.01),
            (2.88, 0.01),
            (2.89, 0.01),
            (2.90, 0.1),
            (2.91, 0.01),
            (2.92, 0.01),
            (2.93, 0.01),
            (2.94, 0.01),
            (2.95, 0.01),
            (2.96, 0.01),
            (2.97, 0.01),
            (2.98, 0.01),
            (2.99, 0.01),
            (3.00, 1.0),
            (3.01, 0.01),
        ];
        assert_eq!(marks.len(), want.len());
        for (m, w) in marks.iter().zip(want) {
            assert!(close(m.0, w.0) && m.1 == w.1, "{m:?} != {w:?}");
        }
    }

    #[test]
    fn grid_marks_of_zero_step_are_empty() {
        assert!(grid_marks(0.0, 10.0, 0.0).is_empty());
    }

    #[test]
    fn strength_fades_between_the_spacing_limits() {
        assert_eq!(strength(8.0, 8.0, 300.0), 0.0);
        assert_eq!(strength(154.0, 8.0, 300.0), 0.5);
        assert_eq!(strength(1000.0, 8.0, 300.0), 1.0);
    }

    #[test]
    fn label_shows_only_when_spaced_enough_and_wide_enough() {
        // xの数字は60ptを超える間隔で出し、80ptで濃さが最大になる。数字の幅が間隔より広ければ出さない。
        assert_eq!(label_alpha(60.0, 10.0, (60.0, 80.0)), None);
        assert_eq!(label_alpha(70.0, 80.0, (60.0, 80.0)), None);
        assert_eq!(label_alpha(70.0, 70.0, (60.0, 80.0)), Some(0.5f32.sqrt()));
        assert_eq!(label_alpha(200.0, 70.0, (60.0, 80.0)), Some(1.0));
    }

    #[test]
    fn auto_color_matches_egui_plot() {
        // egui_plot 0.38.0のauto_colorがegui 0.37.0で出す色を書き写した値。
        assert_eq!(
            (0..6).map(auto_color).collect::<Vec<_>>(),
            [0xbc4d4d, 0x4d7bbc, 0x9abc4d, 0xbc4db2, 0x4dbcaf, 0xbc964d]
        );
    }

    #[test]
    fn nearest_finds_point_within_radius_and_none_outside() {
        let g = r((0.0, 10.0), (0.0, 100.0));
        let a: &[[f64; 2]] = &[[0.0, 0.0], [5.0, 50.0]];
        let b: &[[f64; 2]] = &[[5.0, 52.0]];
        // (5,50)は画面の(110,70)。bの点は(110,68)で、どちらも半径5の中にある。上に描くbを選ぶ。
        assert_eq!(nearest(&[a, b], &g, RECT, (110.0, 70.0), 5.0), Some((1, 0)));
        assert_eq!(nearest(&[a], &g, RECT, (110.0, 70.0), 5.0), Some((0, 1)));
        assert_eq!(nearest(&[a, b], &g, RECT, (150.0, 70.0), 5.0), None);
        assert_eq!(nearest(&[], &g, RECT, (110.0, 70.0), 5.0), None);
    }

    fn range() -> impl Strategy<Value = Range2> {
        (-1e6..1e6f64, 1e-3..1e6f64, -1e6..1e6f64, 1e-3..1e6f64)
            .prop_map(|(x0, w, y0, h)| r((x0, x0 + w), (y0, y0 + h)))
    }

    fn inside() -> impl Strategy<Value = (f32, f32)> {
        (10.0..210.0f32, 20.0..120.0f32)
    }

    proptest! {
        #[test]
        fn prop_to_data_inverts_to_screen(g in range(), s in inside()) {
            let back = to_screen(&g, RECT, to_data(&g, RECT, s));
            prop_assert!((back.0 - s.0).abs() < 1e-2 && (back.1 - s.1).abs() < 1e-2, "{s:?} -> {back:?}");
        }

        #[test]
        fn prop_zoom_anchor_is_fixed(g in range(), s in inside(), fx in 0.1..10.0f64, fy in 0.1..10.0f64) {
            let before = to_data(&g, RECT, s);
            let after = to_data(&zoom(&g, RECT, s, (fx, fy)), RECT, s);
            prop_assert!(close(before[0], after[0]) || (before[0] - after[0]).abs() < 1e-6 * g.width());
            prop_assert!(close(before[1], after[1]) || (before[1] - after[1]).abs() < 1e-6 * g.height());
        }

        #[test]
        fn prop_pan_preserves_span(g in range(), dx in -500.0..500.0f32, dy in -500.0..500.0f32) {
            let p = pan(&g, RECT, (dx, dy));
            prop_assert!(close(p.width(), g.width()) || (p.width() - g.width()).abs() < 1e-6 * g.width());
            prop_assert!(close(p.height(), g.height()) || (p.height() - g.height()).abs() < 1e-6 * g.height());
        }

        #[test]
        fn prop_auto_range_contains_all_points(
            pts in prop::collection::vec((-1e6..1e6f64, -1e6..1e6f64), 0..20),
            ix in prop::collection::vec(-1e6..1e6f64, 0..3),
            iy in prop::collection::vec(-1e6..1e6f64, 0..3),
        ) {
            let pts: Vec<[f64; 2]> = pts.into_iter().map(|(x, y)| [x, y]).collect();
            let a = auto_range(&[&pts], &ix, &iy);
            prop_assert!(a.x.0.is_finite() && a.x.1.is_finite() && a.y.0.is_finite() && a.y.1.is_finite());
            prop_assert!(a.x.0 < a.x.1 && a.y.0 < a.y.1, "{a:?}");
            for p in &pts {
                prop_assert!(a.x.0 <= p[0] && p[0] <= a.x.1 && a.y.0 <= p[1] && p[1] <= a.y.1);
            }
            for x in &ix { prop_assert!(a.x.0 <= *x && *x <= a.x.1); }
            for y in &iy { prop_assert!(a.y.0 <= *y && *y <= a.y.1); }
        }

        #[test]
        fn prop_grid_marks_are_increasing_and_within_bounds(lo in -1e6..1e6f64, w in 1e-3..1e6f64, k in 1e-4..1.0f64) {
            let marks = grid_marks(lo, lo + w, w * k);
            prop_assert!(marks.windows(2).all(|m| m[0].0 < m[1].0));
            prop_assert!(marks.iter().all(|m| lo <= m.0 + m.1 * 1e-9 && m.0 <= lo + w + m.1 * 1e-9));
        }
    }
}
