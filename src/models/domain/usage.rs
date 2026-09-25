//! 使用量API（`/api/oauth/usage`）の応答を、画面と保存に使う型へ変換する。
//!
//! 応答には旧形式（`five_hour`など）と新形式（`limits[]`）が併存している。新しい枠が増えても
//! 表示から漏れないように、汎用の`limits[]`だけを正として読む。
use chrono::{DateTime, Utc};
use serde::Deserialize;

/// 使用量の枠1つ。`kind`を列挙型にせず文字列で持つのは、APIに未知の枠が追加されても落とさず表示するため。
#[derive(Debug, Clone, PartialEq)]
pub struct LimitWindow {
    /// 枠の種類（`session`、`weekly_all`、`weekly_scoped`など）。APIの値そのまま。
    pub kind: String,
    /// 枠のまとまり（`session`、`weekly`）。欠けている場合は空文字。グラフで同じ系列にまとめるために使う。
    pub group: String,
    /// 上限に対する使用率（0〜100）。APIは絶対量を返さないため、画面は常にこの割合で表示する。
    pub percent: f64,
    /// APIが判定した深刻度（`normal`、`warning`など）。欠けている場合は`normal`。色分けをAPIの判断に合わせるため。
    pub severity: String,
    /// 使用率が0に戻る時刻。残り時間と予測の基準になる。
    pub resets_at: Option<DateTime<Utc>>,
    /// モデル別の枠の対象モデル名（例 `Fable`）。全体の枠では`None`。
    pub scope_label: Option<String>,
}

/// 週間消費の用途別内訳の1行。どの用途が枠を使っているかを利用者が判断できるようにするため。
#[derive(Debug, Clone, PartialEq)]
pub struct BreakdownRow {
    /// 用途のキー（`claude_code`など）。保存時の主キーに使う。
    pub key: String,
    /// 画面に出す用途名。
    pub display_name: String,
    /// 週間消費に占める割合（0〜100）。
    pub percent: f64,
}

/// 追加課金枠の使用額。金額を浮動小数にせず最小単位の整数で持つのは、丸め誤差で表示がずれるのを防ぐため。
#[derive(Debug, Clone, PartialEq)]
pub struct Spend {
    /// 使用額（最小単位。USDならセント）。
    pub used_minor: i64,
    /// 上限額（最小単位）。上限がない契約では`None`。
    pub limit_minor: Option<i64>,
    /// 最小単位の桁数（USDなら2）。
    pub exponent: u32,
    /// 通貨コード。
    pub currency: String,
}

/// 1回の取得結果全体。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct UsageSnapshot {
    /// 使用量の枠。APIの並び順を保つ。
    pub limits: Vec<LimitWindow>,
    /// 用途別内訳。
    pub breakdown: Vec<BreakdownRow>,
    /// 追加課金枠。契約にない場合は`None`。
    pub spend: Option<Spend>,
}

/// 応答の解析失敗。
#[derive(Debug, thiserror::Error)]
pub enum UsageParseError {
    /// JSONとして読めない、または必須項目が欠けている。
    #[error("使用量APIの応答を解析できません: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Deserialize)]
struct RawUsage {
    #[serde(default)]
    limits: Vec<RawLimit>,
    seven_day_breakdown: Option<RawBreakdown>,
    spend: Option<RawSpend>,
}

#[derive(Deserialize)]
struct RawLimit {
    kind: String,
    group: Option<String>,
    percent: f64,
    severity: Option<String>,
    resets_at: Option<DateTime<Utc>>,
    scope: Option<RawScope>,
}

#[derive(Deserialize)]
struct RawScope {
    model: Option<RawScopeModel>,
}

#[derive(Deserialize)]
struct RawScopeModel {
    display_name: Option<String>,
}

#[derive(Deserialize)]
struct RawBreakdown {
    #[serde(default)]
    rows: Vec<RawRow>,
}

#[derive(Deserialize)]
struct RawRow {
    key: String,
    display_name: String,
    percent: f64,
}

#[derive(Deserialize)]
struct RawSpend {
    used: RawMoney,
    limit: Option<RawMoney>,
}

#[derive(Deserialize)]
struct RawMoney {
    amount_minor: i64,
    currency: String,
    exponent: u32,
}

/// 応答本文を解析する。未知のフィールドは無視し、欠けた任意項目は空として扱う。
pub fn parse_usage(body: &str) -> Result<UsageSnapshot, UsageParseError> {
    let raw: RawUsage = serde_json::from_str(body)?;
    let limits = raw
        .limits
        .into_iter()
        .map(|l| LimitWindow {
            kind: l.kind,
            group: l.group.unwrap_or_default(),
            percent: l.percent,
            severity: l.severity.unwrap_or_else(|| "normal".to_string()),
            resets_at: l.resets_at,
            scope_label: l.scope.and_then(|s| s.model).and_then(|m| m.display_name),
        })
        .collect();
    let breakdown = raw
        .seven_day_breakdown
        .map(|b| b.rows)
        .unwrap_or_default()
        .into_iter()
        .map(|r| BreakdownRow {
            key: r.key,
            display_name: r.display_name,
            percent: r.percent,
        })
        .collect();
    let spend = raw.spend.map(|s| Spend {
        used_minor: s.used.amount_minor,
        limit_minor: s.limit.map(|l| l.amount_minor),
        exponent: s.used.exponent,
        currency: s.used.currency,
    });
    Ok(UsageSnapshot {
        limits,
        breakdown,
        spend,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const OK: &str = include_str!("../../../tests/fixtures/usage_ok.json");

    #[test]
    fn parses_limits_in_api_order() {
        let s = parse_usage(OK).unwrap();
        let kinds: Vec<&str> = s.limits.iter().map(|l| l.kind.as_str()).collect();
        assert_eq!(kinds, ["session", "weekly_all", "weekly_scoped"]);
        assert_eq!(s.limits[0].percent, 13.0);
        assert_eq!(s.limits[0].severity, "normal");
        assert_eq!(
            s.limits[0].resets_at.unwrap().to_rfc3339(),
            "2026-09-26T00:19:59.834414+00:00"
        );
    }

    #[test]
    fn scoped_limit_uses_model_display_name() {
        let s = parse_usage(OK).unwrap();
        assert_eq!(s.limits[2].scope_label.as_deref(), Some("Fable"));
        assert_eq!(s.limits[0].scope_label, None);
    }

    #[test]
    fn parses_breakdown_and_spend() {
        let s = parse_usage(OK).unwrap();
        assert_eq!(s.breakdown.len(), 2);
        assert_eq!(s.breakdown[0].display_name, "Claude Code");
        let spend = s.spend.unwrap();
        assert_eq!(
            (spend.used_minor, spend.limit_minor, spend.exponent),
            (1850, Some(10000), 2)
        );
        assert_eq!(spend.currency, "USD");
    }

    #[test]
    fn missing_optional_sections_yield_empty() {
        let s = parse_usage(r#"{"limits":[]}"#).unwrap();
        assert!(s.limits.is_empty() && s.breakdown.is_empty() && s.spend.is_none());
    }

    #[test]
    fn missing_limits_field_yields_empty_limits() {
        assert!(parse_usage("{}").unwrap().limits.is_empty());
    }

    #[test]
    fn unknown_kind_and_null_fields_are_kept() {
        let body = r#"{"limits":[{"kind":"brand_new","percent":5,"resets_at":null,"scope":null}]}"#;
        let s = parse_usage(body).unwrap();
        assert_eq!(s.limits[0].kind, "brand_new");
        assert_eq!(s.limits[0].group, "");
        assert_eq!(s.limits[0].severity, "normal");
        assert!(s.limits[0].resets_at.is_none());
    }

    #[test]
    fn invalid_json_is_error() {
        assert!(matches!(
            parse_usage("not json"),
            Err(UsageParseError::Json(_))
        ));
    }
}
