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
const TREND_RATIO: f32 = 0.25;
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

/// 推移グラフの高さ。ダッシュボードの高さの25%を140から320に収める。
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
        assert_eq!(trend_height(800.0), 200.0);
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
}
