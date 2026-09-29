//! 画面に出す数値と文言の整形、指標の説明文。
//!
//! 整形はI/Oを持たない純粋関数なので、views（mutationの対象外）ではなくここに置き、変異テストで守る。

use crate::models::domain::projection::Projection;
use chrono::{DateTime, Duration, FixedOffset, Utc};

/// 指標の説明文。ツールチップと画面の注記で同じ文言を使うため、定数にまとめる。
pub mod help {
    /// メモリの表示の意味。
    pub const MEMORY: &str = "子プロセスを含むRSSの合計です。共有メモリを重ねて数えるため、アクティビティモニタの値より大きく出ます";
    /// 入力Token。
    pub const TOKENS_INPUT: &str = "キャッシュを使わずにモデルへ渡した入力の量です";
    /// 出力Token。
    pub const TOKENS_OUTPUT: &str = "モデルが生成した出力の量です";
    /// キャッシュ読込。
    pub const TOKENS_CACHE_READ: &str =
        "以前に保存したキャッシュから読んだ入力の量です。通常の入力より安く計算されます";
    /// キャッシュ作成。
    pub const TOKENS_CACHE_WRITE: &str =
        "次回以降に再利用するため、キャッシュへ書き込んだ入力の量です";
    /// コスト。
    pub const COST: &str = "API単価で換算した金額です。定額プランの請求額ではありません";
    /// コンテキスト使用率。
    pub const CONTEXT: &str =
        "そのターンでモデルが読んだ入力が、モデルのコンテキスト長に占める割合です";
    /// キャッシュヒット率。
    pub const CACHE_HIT: &str =
        "入力のうち、キャッシュから読めた割合です。高いほど安く速くなります";
    /// 予測。
    pub const PROJECTION: &str = "直近30分の使用率の増え方から、100%に届く時刻を求めた値です";
    /// 用途の内訳。
    pub const BREAKDOWN: &str = "週間枠の消費が、どの製品で使われたかの割合です";
    /// 追加課金。
    pub const SPEND: &str = "プランの枠を超えた分の従量課金の使用額と上限です";
    /// 種別。
    pub const RUN_KIND: &str = "対話は画面で操作中のセッション、ヘッドレスはSDKやclaude -pの実行、ジョブはバックグラウンドジョブです";
    /// デーモンのメモリ。
    pub const RSS: &str =
        "デーモンが使っている物理メモリです。右肩上がりが続く場合はリークを疑います";
}

/// Token数を`1.26M`や`14.2k`の形にする。桁数の多い数を一目で比べられるようにするため。
pub fn fmt_tokens(n: u64) -> String {
    const UNITS: [(f64, &str); 3] = [(1e9, "B"), (1e6, "M"), (1e3, "k")];
    let v = n as f64;
    UNITS
        .iter()
        .find(|(base, _)| v >= *base)
        .map(|(base, unit)| format!("{}{unit}", three_digits(v / base)))
        .unwrap_or_else(|| n.to_string())
}

/// 有効数字3桁で表す。
fn three_digits(x: f64) -> String {
    if x >= 100.0 {
        format!("{x:.0}")
    } else if x >= 10.0 {
        format!("{x:.1}")
    } else {
        format!("{x:.2}")
    }
}

/// 経過時間や残り時間を、大きい方から2つの単位で表す。秒単位まで出すと読みにくいため。
pub fn fmt_duration(d: Duration) -> String {
    let s = d.num_seconds().max(0);
    let (days, hours, mins) = (s / 86_400, s % 86_400 / 3_600, s % 3_600 / 60);
    match (days, hours, mins) {
        (0, 0, 0) => format!("{s}秒"),
        (0, 0, m) => format!("{m}分"),
        (0, h, 0) => format!("{h}時間"),
        (0, h, m) => format!("{h}時間{m}分"),
        (d, 0, _) => format!("{d}日"),
        (d, h, _) => format!("{d}日{h}時間"),
    }
}

/// 金額を表す。単価未登録を0円と誤読させないため、`None`は文言で示す。
pub fn fmt_usd(v: Option<f64>) -> String {
    match v {
        None => "単価未登録".to_string(),
        Some(x) if x > 0.0 && x < 0.005 => "<$0.01".to_string(),
        Some(x) => format!("${x:.2}"),
    }
}

/// 使用率を整数の%で表す。
pub fn fmt_percent(p: f64) -> String {
    format!("{p:.0}%")
}

/// 割合。分母が0なら割合を決められないので`None`を返し、画面は「不明」と表示する。
pub fn ratio(num: u64, den: u64) -> Option<f64> {
    (den > 0).then(|| num as f64 / den as f64)
}

/// 割合（0〜1）を%で表す。求められないときは「不明」とし、0%と区別する。
pub fn fmt_ratio(r: Option<f64>) -> String {
    r.map(|x| fmt_percent(x * 100.0))
        .unwrap_or_else(|| "不明".to_string())
}

/// 時刻を現地時刻の`HH:MM`で表す。今日でなければ日付を付け、翌日のリセットを今日と誤読させない。
pub fn fmt_clock(t: DateTime<Utc>, now: DateTime<Utc>, tz: FixedOffset) -> String {
    let (lt, ln) = (t.with_timezone(&tz), now.with_timezone(&tz));
    if lt.date_naive() == ln.date_naive() {
        lt.format("%H:%M").to_string()
    } else {
        lt.format("%-m/%-d %H:%M").to_string()
    }
}

/// バイト数を2進接頭辞で表す。アクティビティモニタの表示と桁を揃えるため。
pub fn fmt_bytes(n: u64) -> String {
    const UNITS: [(u64, &str); 3] = [(1 << 30, "GB"), (1 << 20, "MB"), (1 << 10, "KB")];
    UNITS
        .iter()
        .find(|(base, _)| n >= *base)
        .map(|(base, unit)| format!("{:.1}{unit}", n as f64 / *base as f64))
        .unwrap_or_else(|| format!("{n}B"))
}

/// 使用量APIの`severity`の段階。色分けに使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// 通常。
    Normal,
    /// 注意（黄）。
    Warning,
    /// 危険（赤）。未知の値もここに入れ、見落としを防ぐ。
    Critical,
}

/// `severity`の文字列を段階に直す。
pub fn severity(s: &str) -> Severity {
    match s {
        "normal" => Severity::Normal,
        "warning" => Severity::Warning,
        _ => Severity::Critical,
    }
}

/// 枠の見出し。未知の`kind`はAPIの名前のまま出し、新しい枠を表示から落とさない。
pub fn limit_title(kind: &str, scope_label: Option<&str>) -> String {
    match kind {
        "session" => "5時間枠".to_string(),
        "weekly_all" => "週間枠".to_string(),
        "weekly_scoped" => format!("週間枠（{}）", scope_label.unwrap_or("モデル別")),
        other => other.to_string(),
    }
}

/// 枠の説明文。
pub fn limit_help(kind: &str) -> &'static str {
    match kind {
        "session" => "直近5時間の利用量の上限に対する割合です。リセット時刻に0%へ戻ります",
        "weekly_all" => "直近7日間の、全モデル合計の利用量の上限に対する割合です",
        "weekly_scoped" => "直近7日間の、このモデルだけの利用量の上限に対する割合です",
        _ => "使用量APIが返した枠です。意味はAPIの名前のまま表示しています",
    }
}

/// コンテキスト使用率。コンテキスト長が不明なモデルでは求めない。
pub fn context_ratio(tokens: u64, window: Option<u64>) -> Option<f64> {
    window.filter(|w| *w > 0).map(|w| tokens as f64 / w as f64)
}

/// 予測結果の文言。
pub fn projection_text(p: &Projection, now: DateTime<Utc>, tz: FixedOffset) -> String {
    match p {
        Projection::NotEnoughData => "予測に必要なデータが不足しています".to_string(),
        Projection::NotIncreasing => "使用率は増えていません".to_string(),
        Projection::NotBeforeReset => "リセットまで上限に届きません".to_string(),
        Projection::ReachesAt(t) => format!("このペースだと {} に上限", fmt_clock(*t, now, tz)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratio_is_none_only_for_zero_denominator() {
        assert_eq!(ratio(1, 4), Some(0.25));
        assert_eq!(ratio(0, 1), Some(0.0));
        assert_eq!(ratio(3, 0), None);
    }
    use crate::models::domain::projection::Projection;
    use chrono::{Duration, FixedOffset, TimeZone, Utc};

    fn jst() -> FixedOffset {
        FixedOffset::east_opt(9 * 3600).unwrap()
    }

    #[test]
    fn tokens_are_shortened_by_unit() {
        assert_eq!(fmt_tokens(0), "0");
        assert_eq!(fmt_tokens(999), "999");
        assert_eq!(fmt_tokens(1_000), "1.00k");
        assert_eq!(fmt_tokens(14_200), "14.2k");
        assert_eq!(fmt_tokens(999_999), "1000k");
        assert_eq!(fmt_tokens(1_258_115), "1.26M");
        assert_eq!(fmt_tokens(123_400_000), "123M");
        assert_eq!(fmt_tokens(2_500_000_000), "2.50B");
    }

    #[test]
    fn durations_use_the_two_largest_units() {
        assert_eq!(fmt_duration(Duration::seconds(-5)), "0秒");
        assert_eq!(fmt_duration(Duration::seconds(0)), "0秒");
        assert_eq!(fmt_duration(Duration::seconds(59)), "59秒");
        assert_eq!(fmt_duration(Duration::seconds(60)), "1分");
        assert_eq!(fmt_duration(Duration::minutes(59)), "59分");
        assert_eq!(fmt_duration(Duration::minutes(60)), "1時間");
        assert_eq!(fmt_duration(Duration::minutes(108)), "1時間48分");
        assert_eq!(fmt_duration(Duration::hours(24)), "1日");
        assert_eq!(fmt_duration(Duration::hours(51)), "2日3時間");
    }

    #[test]
    fn usd_none_is_unpriced() {
        assert_eq!(fmt_usd(None), "単価未登録");
        assert_eq!(fmt_usd(Some(0.0)), "$0.00");
        assert_eq!(fmt_usd(Some(0.004)), "<$0.01");
        assert_eq!(fmt_usd(Some(0.005)), "$0.01");
        assert_eq!(fmt_usd(Some(12.346)), "$12.35");
    }

    #[test]
    fn percent_and_ratio() {
        assert_eq!(fmt_percent(13.4), "13%");
        assert_eq!(fmt_percent(80.6), "81%");
        assert_eq!(fmt_ratio(Some(0.321)), "32%");
        assert_eq!(fmt_ratio(None), "不明");
    }

    #[test]
    fn clock_shows_date_only_on_other_days() {
        let now = Utc.with_ymd_and_hms(2026, 9, 26, 3, 0, 0).unwrap();
        assert_eq!(fmt_clock(now + Duration::minutes(10), now, jst()), "12:10");
        assert_eq!(
            fmt_clock(now + Duration::hours(12), now, jst()),
            "9/27 00:00"
        );
    }

    #[test]
    fn bytes_use_binary_units() {
        assert_eq!(fmt_bytes(512), "512B");
        assert_eq!(fmt_bytes(2048), "2.0KB");
        assert_eq!(fmt_bytes(15 * 1024 * 1024), "15.0MB");
        assert_eq!(fmt_bytes(3 * 1024 * 1024 * 1024), "3.0GB");
    }

    #[test]
    fn severity_maps_unknown_to_critical() {
        assert_eq!(severity("normal"), Severity::Normal);
        assert_eq!(severity("warning"), Severity::Warning);
        assert_eq!(severity("critical"), Severity::Critical);
        assert_eq!(severity("new_level"), Severity::Critical);
    }

    #[test]
    fn limit_titles_and_help() {
        assert_eq!(limit_title("session", None), "5時間枠");
        assert_eq!(limit_title("weekly_all", None), "週間枠");
        assert_eq!(
            limit_title("weekly_scoped", Some("Fable")),
            "週間枠（Fable）"
        );
        assert_eq!(limit_title("weekly_scoped", None), "週間枠（モデル別）");
        assert_eq!(limit_title("monthly_new", None), "monthly_new");
        assert!(limit_help("session").contains("5時間"));
        assert!(limit_help("weekly_all").contains("全モデル"));
        assert!(limit_help("weekly_scoped").contains("このモデル"));
        assert!(limit_help("monthly_new").contains("APIの名前"));
    }

    #[test]
    fn context_ratio_unknown_window_is_none() {
        assert_eq!(context_ratio(50_000, Some(200_000)), Some(0.25));
        assert_eq!(context_ratio(50_000, None), None);
        assert_eq!(context_ratio(50_000, Some(0)), None);
    }

    #[test]
    fn projection_texts() {
        let now = Utc.with_ymd_and_hms(2026, 9, 26, 3, 0, 0).unwrap();
        assert_eq!(
            projection_text(&Projection::NotEnoughData, now, jst()),
            "予測に必要なデータが不足しています"
        );
        assert_eq!(
            projection_text(&Projection::NotIncreasing, now, jst()),
            "使用率は増えていません"
        );
        assert_eq!(
            projection_text(&Projection::NotBeforeReset, now, jst()),
            "リセットまで上限に届きません"
        );
        assert_eq!(
            projection_text(
                &Projection::ReachesAt(now + Duration::minutes(130)),
                now,
                jst()
            ),
            "このペースだと 14:10 に上限"
        );
    }
}
