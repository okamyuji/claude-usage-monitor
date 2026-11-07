//! 伸縮のレイアウト計算（spec 7.3節）。描画を持たない純粋関数にして、ユニットテストで確かめる。

/// カード1枚の最小幅。
pub const MIN_CARD: f32 = 320.0;
/// 中段の左右の最小幅。
pub const MIN_PANE: f32 = 360.0;
/// 部品の間隔。
pub const GAP: f32 = 12.0;
/// 中段の境界のつまみの幅。
pub const SPLITTER: f32 = 6.0;
/// 推移を折りたたんだときの高さ（見出しの行だけ）。
pub const TREND_COLLAPSED: f32 = 40.0;
/// 中段の最小の高さ。
pub const MIN_MIDDLE: f32 = 200.0;
const TREND_RATIO: f32 = 0.2;
const MIN_TREND: f32 = 140.0;
const MAX_TREND: f32 = 320.0;

/// カードの中の使用率の1本の最小幅。
pub const MIN_LIMIT: f32 = 240.0;

/// 1行に何個置くかと、1個の幅。空き幅を等分し、最小幅を下回るときは次の行へ折り返す。
pub fn columns(avail: f32, n: usize, min: f32) -> (usize, f32) {
    let n = n.max(1);
    let fit = ((avail + GAP) / (min + GAP)).floor().max(1.0) as usize;
    let cols = fit.min(n);
    let width = (avail - GAP * (cols - 1) as f32) / cols as f32;
    (cols, width.max(0.0))
}

/// カードを1行に何枚置くかと、1枚の幅。
pub fn card_columns(avail: f32, n: usize) -> (usize, f32) {
    columns(avail, n, MIN_CARD)
}

/// 中段の左右の幅。比率を掛け、左右とも最小幅を守る。両方の最小幅が入らないときは半分ずつにする。
pub fn split_widths(avail: f32, ratio: f32) -> (f32, f32) {
    let usable = (avail - SPLITTER).max(0.0);
    if usable < 2.0 * MIN_PANE {
        let half = usable / 2.0;
        return (half, half);
    }
    let left = (usable * ratio).clamp(MIN_PANE, usable - MIN_PANE);
    (left, usable - left)
}

/// つまみを`dx`だけ動かした後の比率。
pub fn drag_ratio(avail: f32, left: f32, dx: f32) -> f32 {
    let usable = (avail - SPLITTER).max(1.0);
    ((left + dx) / usable).clamp(0.0, 1.0)
}

/// 推移グラフの高さ。ダッシュボードの高さの20%を140から320に収める。
/// 25%では、既定のウィンドウの高さで詳細のライブログが3行しか見えなかった。
pub fn trend_height(total: f32) -> f32 {
    (total * TREND_RATIO).clamp(MIN_TREND, MAX_TREND)
}

/// 中段の高さ。カードを描いた後の残りから、推移と、中段の前後の間隔2つを引き、最小200にする。
pub fn middle_height(remaining: f32, trend: f32) -> f32 {
    (remaining - trend - 2.0 * GAP).max(MIN_MIDDLE)
}

/// 伸びる列の最小幅。
pub const MIN_FLEX: f32 = 80.0;

/// 表の列幅。`Some`は固定幅、`None`は残りを受け取る列（1つだけ）。列の間は`GAP / 2`空ける。
pub fn flex_columns(avail: f32, fixed: &[Option<f32>]) -> Vec<f32> {
    let gaps = GAP / 2.0 * fixed.len().saturating_sub(1) as f32;
    let used: f32 = fixed.iter().flatten().sum();
    let rest = (avail - used - gaps).max(MIN_FLEX);
    fixed.iter().map(|w| w.unwrap_or(rest)).collect()
}

/// 日の列の左端に置く濃淡の幅。
pub const DENSITY_PX: f32 = 6.0;

/// 帯の外側（割合）と内側（ピクセル）の置き方。
/// 式は`left = x + DENSITY + 1 + lane_w * lane`、`width = lane_w - 1`、`lane_w = (col - DENSITY - 2) / lanes`で、
/// 列の幅に比例する部分（外側の割合）と、ピクセルの部分（内側の左右の寄せ）に分ける。
/// 戻り値は（外側の左端と幅の割合、内側の左と右の寄せ）。
pub fn band_place(day: usize, lane: usize, lanes: usize) -> ((f32, f32), (f32, f32)) {
    let lanes = lanes.max(1) as f32;
    let lane = lane as f32;
    let col = 1.0 / 7.0;
    let outer = (col * day as f32 + col * lane / lanes, col / lanes);
    let shrink = (DENSITY_PX + 2.0) / lanes;
    let left = DENSITY_PX + 1.0 - lane * shrink;
    // 内側の幅は 外側の幅 - shrink - 1 になる。右の寄せは残り。
    let right = shrink + 1.0 - left;
    (outer, (left, right))
}

/// バーの塗りの割合。0から1に収める。
pub fn bar_fill(ratio: f32) -> f32 {
    ratio.clamp(0.0, 1.0)
}

/// 操作が無くても描き直す間隔。5秒ごとの読み直しと経過時間の表示を進めるため。再生中は短くする。
pub fn redraw_every(playing: bool) -> std::time::Duration {
    std::time::Duration::from_millis(if playing { 50 } else { 1000 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_columns_wrap_below_min_width() {
        assert_eq!(card_columns(1000.0, 2), (2, 494.0));
        assert_eq!(card_columns(600.0, 2), (1, 600.0));
        let (cols, w) = card_columns(2000.0, 3);
        assert_eq!(cols, 3);
        assert!((w - (2000.0 - 24.0) / 3.0).abs() < 1e-3);
        assert_eq!(card_columns(1000.0, 0), (1, 1000.0));
        assert_eq!(card_columns(200.0, 2), (1, 200.0));
        assert_eq!(card_columns(652.0, 5), (2, 320.0));
    }

    #[test]
    fn limits_sit_side_by_side_in_wide_cards() {
        assert_eq!(columns(1232.0, 3, MIN_LIMIT), (3, (1232.0 - 24.0) / 3.0));
        assert_eq!(columns(500.0, 3, MIN_LIMIT), (2, 244.0));
        assert_eq!(columns(300.0, 3, MIN_LIMIT), (1, 300.0));
        assert_eq!(columns(1000.0, 0, MIN_LIMIT), (1, 1000.0));
    }

    #[test]
    fn split_keeps_min_pane_width() {
        assert_eq!(split_widths(1006.0, 0.45), (450.0, 550.0));
        assert_eq!(split_widths(1006.0, 0.1), (360.0, 640.0));
        assert_eq!(split_widths(1006.0, 0.9), (640.0, 360.0));
        assert_eq!(split_widths(606.0, 0.45), (300.0, 300.0));
        assert_eq!(split_widths(726.0, 0.45), (360.0, 360.0));
        assert_eq!(split_widths(0.0, 0.45), (0.0, 0.0));
    }

    #[test]
    fn drag_ratio_moves_and_clamps() {
        assert_eq!(drag_ratio(1006.0, 450.0, 50.0), 0.5);
        assert_eq!(drag_ratio(1006.0, 450.0, -1000.0), 0.0);
        assert_eq!(drag_ratio(1006.0, 450.0, 1000.0), 1.0);
        assert_eq!(drag_ratio(0.0, 0.5, 0.0), 0.5);
    }

    #[test]
    fn trend_height_is_clamped() {
        assert_eq!(trend_height(400.0), 140.0);
        assert_eq!(trend_height(800.0), 160.0);
        assert_eq!(trend_height(2000.0), 320.0);
    }

    #[test]
    fn middle_height_keeps_minimum() {
        assert_eq!(middle_height(700.0, 200.0), 476.0);
        assert_eq!(middle_height(300.0, 200.0), 200.0);
    }

    #[test]
    fn flex_columns_give_rest_to_flexible_column() {
        assert_eq!(
            flex_columns(500.0, &[Some(60.0), None, Some(40.0)]),
            [60.0, 388.0, 40.0]
        );
        assert_eq!(
            flex_columns(100.0, &[Some(60.0), None, Some(40.0)]),
            [60.0, MIN_FLEX, 40.0]
        );
        assert_eq!(flex_columns(300.0, &[Some(60.0)]), [60.0]);
    }

    proptest::proptest! {
        #[test]
        fn prop_split_widths_sum_to_usable(avail in 0.0f32..4000.0, ratio in 0.0f32..=1.0) {
            let (l, r) = split_widths(avail, ratio);
            let usable = (avail - SPLITTER).max(0.0);
            proptest::prop_assert!((l + r - usable).abs() < 1e-2, "{l} + {r} != {usable}");
        }

        #[test]
        fn prop_split_widths_respect_min_pane(avail in 0.0f32..4000.0, ratio in 0.0f32..=1.0) {
            let (l, r) = split_widths(avail, ratio);
            if avail - SPLITTER >= 2.0 * MIN_PANE {
                proptest::prop_assert!(l >= MIN_PANE - 1e-3 && r >= MIN_PANE - 1e-3, "{l} {r}");
            } else {
                proptest::prop_assert_eq!(l, r);
            }
        }

        #[test]
        fn prop_columns_fit_avail(avail in 0.0f32..4000.0, n in 0usize..12, min in 1.0f32..600.0) {
            let (cols, w) = columns(avail, n, min);
            proptest::prop_assert!(cols >= 1 && cols <= n.max(1));
            let used = w * cols as f32 + GAP * (cols - 1) as f32;
            proptest::prop_assert!((used - avail).abs() < 1e-2, "{used} != {avail}");
            if cols > 1 {
                proptest::prop_assert!(w >= min - 1e-3, "{w} < {min}");
            }
        }

        #[test]
        fn prop_drag_ratio_in_unit_range(
            avail in -100.0f32..4000.0,
            left in -1000.0f32..4000.0,
            dx in -4000.0f32..4000.0,
        ) {
            let r = drag_ratio(avail, left, dx);
            proptest::prop_assert!((0.0..=1.0).contains(&r), "{r}");
        }
    }

    #[test]
    fn bar_ratio_is_clamped() {
        assert_eq!(bar_fill(1.5), 1.0);
        assert_eq!(bar_fill(-0.2), 0.0);
        assert_eq!(bar_fill(0.25), 0.25);
    }

    #[test]
    fn redraw_interval_is_short_only_while_replaying() {
        assert_eq!(redraw_every(false), std::time::Duration::from_millis(1000));
        assert_eq!(redraw_every(true), std::time::Duration::from_millis(50));
    }

    /// 実装とは独立に書いた式で、列の幅`col`での帯の左端と幅を出して実装を確かめる。
    fn reference_band(day: usize, lane: usize, lanes: usize, col: f32) -> (f32, f32) {
        let inner = (col - DENSITY_PX - 2.0).max(1.0);
        let lane_w = inner / lanes.max(1) as f32;
        let left = col * day as f32 + DENSITY_PX + 1.0 + lane_w * lane as f32;
        (left, (lane_w - 1.0).max(1.0))
    }

    proptest::proptest! {
        #[test]
        fn prop_band_place_matches_reference(
            day in 0usize..7,
            lanes in 1usize..5,
            lane_seed in 0usize..5,
            col in 40.0f32..400.0,
        ) {
            let lane = lane_seed % lanes;
            let ((x, w), (l, r)) = band_place(day, lane, lanes);
            let width = col * 7.0;
            let left = x * width + l;
            let right = (x + w) * width - r;
            let (el, ew) = reference_band(day, lane, lanes, col);
            proptest::prop_assert!((left - el).abs() < 1e-3, "{left} {el}");
            proptest::prop_assert!((right - left - ew).abs() < 1e-3, "{} {ew}", right - left);
        }
    }
}
