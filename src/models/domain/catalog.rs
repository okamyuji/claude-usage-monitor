//! Anthropic公式の料金ページとモデル一覧（Markdown版）から、モデル情報を読み取る。
//!
//! ページ構成の変化に備え、見出し列の名前で表を特定する。読めなかった場合は空を返し、
//! 呼び出し側が既存の値を残せるようにする。
use crate::models::domain::pricing::{ModelInfo, ModelSource};

/// 料金表の1行（USD/MTok）。
#[derive(Debug, Clone, PartialEq)]
pub struct PriceRow {
    /// 表示名（例 `Claude Opus 5.5`）。
    pub display_name: String,
    /// 入力単価。
    pub input: f64,
    /// 5分キャッシュ作成単価。
    pub cache_write_5m: f64,
    /// 1時間キャッシュ作成単価。
    pub cache_write_1h: f64,
    /// キャッシュ読込単価。
    pub cache_read: f64,
    /// 出力単価。
    pub output: f64,
}

/// モデル一覧の1列。
#[derive(Debug, Clone, PartialEq)]
pub struct OverviewRow {
    /// 表示名。
    pub display_name: String,
    /// APIのモデルID。
    pub api_id: String,
    /// コンテキスト長。
    pub context_window: Option<u64>,
}

fn cells(line: &str) -> Vec<String> {
    let t = line.trim();
    t.trim_start_matches('|')
        .trim_end_matches('|')
        .split('|')
        .map(|c| c.trim().to_string())
        .collect()
}

/// 表示名から注記（` ([retired...](...))`など）を除く。
pub fn clean_name(s: &str) -> String {
    s.split(" (").next().unwrap_or(s).trim().to_string()
}

/// `$2 / MTok<sup>3</sup>`のような表記から金額を読む。
pub fn parse_money(s: &str) -> Option<f64> {
    let rest = s.trim().strip_prefix('$')?;
    rest.split_whitespace().next()?.parse().ok()
}

/// `1M tokens`、`200K tokens`からトークン数を読む。
pub fn parse_token_count(s: &str) -> Option<u64> {
    let head = s.split_whitespace().next()?;
    let (num, mul) = if let Some(n) = head.strip_suffix('M') {
        (n, 1_000_000.0)
    } else if let Some(n) = head.strip_suffix('K') {
        (n, 1_000.0)
    } else {
        (head, 1.0)
    };
    num.parse::<f64>().ok().map(|v| (v * mul) as u64)
}

/// 表示名からモデルIDを推定する（`Claude Opus 4.8`→`claude-opus-4-8`）。モデル一覧に載らない旧モデル用。
pub fn display_name_to_id(name: &str) -> String {
    name.to_lowercase().replace(['.', ' '], "-")
}

/// 料金ページから「Model」列と「Base input tokens」列を持つ最初の表を読む。
pub fn parse_pricing_md(md: &str) -> Vec<PriceRow> {
    let mut lines = md.lines().skip_while(|l| {
        let c = cells(l);
        !(l.trim_start().starts_with('|')
            && c.first().is_some_and(|h| h == "Model")
            && c.iter().any(|h| h == "Base input tokens"))
    });
    let Some(header) = lines.next() else {
        return Vec::new();
    };
    let h = cells(header);
    let col = |name: &str| h.iter().position(|c| c.starts_with(name));
    let (Some(i_in), Some(i_5m), Some(i_1h), Some(i_hit), Some(i_out)) = (
        col("Base input"),
        col("5m cache"),
        col("1h cache"),
        col("Cache hits"),
        col("Output"),
    ) else {
        return Vec::new();
    };
    lines
        .skip(1)
        .take_while(|l| l.trim_start().starts_with('|'))
        .filter_map(|l| {
            let c = cells(l);
            Some(PriceRow {
                display_name: clean_name(c.first()?),
                input: parse_money(c.get(i_in)?)?,
                cache_write_5m: parse_money(c.get(i_5m)?)?,
                cache_write_1h: parse_money(c.get(i_1h)?)?,
                cache_read: parse_money(c.get(i_hit)?)?,
                output: parse_money(c.get(i_out)?)?,
            })
        })
        .collect()
}

/// モデル一覧の比較表から、表示名、APIのモデルID、コンテキスト長を読む。
pub fn parse_overview_md(md: &str) -> Vec<OverviewRow> {
    let rows: Vec<Vec<String>> = md
        .lines()
        .filter(|l| l.trim_start().starts_with('|'))
        .map(cells)
        .collect();
    let find =
        |pred: &dyn Fn(&str) -> bool| rows.iter().find(|r| r.first().is_some_and(|c| pred(c)));
    let (Some(names), Some(ids)) = (find(&|c| c == "Feature"), find(&|c| c == "Claude API ID"))
    else {
        return Vec::new();
    };
    let ctx = find(&|c| c.contains("Context window"));
    names
        .iter()
        .enumerate()
        .skip(1)
        .filter_map(|(i, name)| {
            Some(OverviewRow {
                display_name: name.clone(),
                api_id: ids.get(i)?.trim_matches('`').to_string(),
                context_window: ctx
                    .and_then(|r| r.get(i))
                    .and_then(|c| parse_token_count(c)),
            })
        })
        .collect()
}

/// 料金表とモデル一覧を合わせる。IDはモデル一覧の値を優先し、日付接尾辞を落として前方一致キーにする。
pub fn merge_catalog(prices: &[PriceRow], overview: &[OverviewRow]) -> Vec<ModelInfo> {
    prices
        .iter()
        .map(|p| {
            let ov = overview.iter().find(|o| o.display_name == p.display_name);
            let prefix = match ov {
                Some(o) => strip_date_suffix(&o.api_id),
                None => display_name_to_id(&p.display_name),
            };
            ModelInfo {
                model_prefix: prefix,
                display_name: p.display_name.clone(),
                input: p.input,
                output: p.output,
                cache_read: p.cache_read,
                cache_write_5m: p.cache_write_5m,
                cache_write_1h: p.cache_write_1h,
                context_window: ov.and_then(|o| o.context_window),
                source: ModelSource::Official,
            }
        })
        .collect()
}

fn strip_date_suffix(id: &str) -> String {
    match id.rsplit_once('-') {
        Some((head, tail)) if tail.len() == 8 && tail.chars().all(|c| c.is_ascii_digit()) => {
            head.to_string()
        }
        _ => id.to_string(),
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    const PRICING: &str = include_str!("../../../tests/fixtures/pricing.md");
    const OVERVIEW: &str = include_str!("../../../tests/fixtures/overview.md");

    #[test]
    fn parses_model_pricing_table_only() {
        let rows = parse_pricing_md(PRICING);
        let fable = rows
            .iter()
            .find(|r| r.display_name == "Claude Fable 5.1")
            .unwrap();
        assert_eq!(
            (
                fable.input,
                fable.cache_write_5m,
                fable.cache_write_1h,
                fable.cache_read,
                fable.output
            ),
            (10.0, 12.5, 20.0, 0.25, 50.0)
        );
        let opus = rows
            .iter()
            .find(|r| r.display_name == "Claude Opus 5.5")
            .unwrap();
        assert_eq!((opus.input, opus.output), (4.0, 20.0));
        assert!(rows.iter().any(|r| r.display_name == "Claude Opus 4.1"));
        assert_eq!(
            rows.iter()
                .filter(|r| r.display_name == "Claude Opus 5.5")
                .count(),
            1
        );
    }

    #[test]
    fn parses_ids_and_context_windows() {
        let rows = parse_overview_md(OVERVIEW);
        let haiku = rows
            .iter()
            .find(|r| r.display_name == "Claude Haiku 4.5")
            .unwrap();
        assert_eq!(haiku.api_id, "claude-haiku-4-5-20251001");
        assert_eq!(haiku.context_window, Some(200_000));
        assert_eq!(
            rows.iter()
                .find(|r| r.display_name == "Claude Opus 5.5")
                .unwrap()
                .context_window,
            Some(1_000_000)
        );
    }

    #[test]
    fn merge_uses_overview_id_prefix_and_derives_others() {
        let models = merge_catalog(&parse_pricing_md(PRICING), &parse_overview_md(OVERVIEW));
        let haiku = models
            .iter()
            .find(|m| m.display_name == "Claude Haiku 4.5")
            .unwrap();
        assert_eq!(haiku.model_prefix, "claude-haiku-4-5");
        assert_eq!(haiku.context_window, Some(200_000));
        let old = models
            .iter()
            .find(|m| m.display_name == "Claude Opus 4.8")
            .unwrap();
        assert_eq!(
            (old.model_prefix.as_str(), old.context_window),
            ("claude-opus-4-8", None)
        );
        assert!(models.iter().all(|m| m.source == ModelSource::Official));
    }

    #[test]
    fn helpers_parse_money_tokens_and_names() {
        assert_eq!(parse_money("$2 / MTok<sup>3</sup>"), Some(2.0));
        assert_eq!(parse_money("$0.25 / MTok"), Some(0.25));
        assert_eq!(parse_money("n/a"), None);
        assert_eq!(parse_token_count("1M tokens"), Some(1_000_000));
        assert_eq!(parse_token_count("200K tokens"), Some(200_000));
        assert_eq!(parse_token_count("?"), None);
        assert_eq!(display_name_to_id("Claude Opus 4.8"), "claude-opus-4-8");
        assert_eq!(clean_name("Claude Opus 4 ([retired](x))"), "Claude Opus 4");
    }

    #[test]
    fn pricing_table_is_found_after_similar_tables() {
        let md = "\
| Model | Batch input |
|---|---|
| Claude X | $1 / MTok |

| Feature | Base input tokens |
|---|---|
| Claude Y | $2 / MTok |

| Model | Base input tokens | 5m cache writes | 1h cache writes | Cache hits & refreshes | Output tokens |
|---|---|---|---|---|---|
| Claude Opus 5.5 | $4 / MTok | $5 / MTok | $8 / MTok | $0.20 / MTok | $20 / MTok |
";
        let rows = parse_pricing_md(md);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].display_name, "Claude Opus 5.5");
        assert_eq!((rows[0].input, rows[0].output), (4.0, 20.0));
    }

    #[test]
    fn unrelated_markdown_yields_nothing() {
        assert!(parse_pricing_md("# 見出し\n| a | b |\n|---|---|\n| 1 | 2 |").is_empty());
        assert!(parse_overview_md("").is_empty());
    }
}
