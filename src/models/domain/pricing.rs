//! モデルの単価、コンテキスト長、コスト計算。
//!
//! 単価は改定されるため、値はDBの`models`表に持ち、この型はその1行を表す。
//! 初期値`seed_models`は、オフラインで初めて起動したときにもコストを出せるようにするための埋め込み値。

/// 単価の出どころ。利用者が手で直した値を公式の自動取得で上書きしないために区別する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelSource {
    /// 公式ページから取得した値、またはその埋め込み初期値。
    Official,
    /// 利用者が設定画面で編集した値。
    User,
}

impl ModelSource {
    /// DB保存用の文字列。
    pub fn as_str(self) -> &'static str {
        match self {
            ModelSource::Official => "official",
            ModelSource::User => "user",
        }
    }

    /// DBの文字列から戻す。未知の値を黙って既定値にしないため`Option`で返す。
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "official" => Some(ModelSource::Official),
            "user" => Some(ModelSource::User),
            _ => None,
        }
    }
}

/// モデル1種類の単価（USD/MTok）とコンテキスト長。
#[derive(Debug, Clone, PartialEq)]
pub struct ModelInfo {
    /// モデルIDの前方一致キー（例 `claude-opus-5-5`）。日付付きIDにも当てるため前方一致にする。
    pub model_prefix: String,
    /// 公式ページ上の表示名（例 `Claude Opus 5.5`）。
    pub display_name: String,
    /// 入力単価。
    pub input: f64,
    /// 出力単価。
    pub output: f64,
    /// キャッシュ読込単価。
    pub cache_read: f64,
    /// 5分キャッシュ作成単価。
    pub cache_write_5m: f64,
    /// 1時間キャッシュ作成単価。
    pub cache_write_1h: f64,
    /// コンテキスト長（トークン）。不明なら`None`にし、使用率を推測で出さない。
    pub context_window: Option<u64>,
    /// 値の出どころ。
    pub source: ModelSource,
}

/// 1ターンまたは合計のToken数。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenUsage {
    /// キャッシュを使わなかった入力。
    pub input: u64,
    /// 出力。
    pub output: u64,
    /// キャッシュから読んだ入力。
    pub cache_read: u64,
    /// 5分キャッシュへ書いた入力。
    pub cache_write_5m: u64,
    /// 1時間キャッシュへ書いた入力。
    pub cache_write_1h: u64,
}

impl TokenUsage {
    /// そのターンでモデルが読んだ入力の総量。コンテキスト使用率の分子に使う。
    pub fn context_tokens(&self) -> u64 {
        self.input + self.cache_read + self.cache_write_5m + self.cache_write_1h
    }

    /// 2つの合計を返す。集計で元の値を書き換えないため新しい値を返す。
    pub fn plus(&self, o: &TokenUsage) -> TokenUsage {
        TokenUsage {
            input: self.input + o.input,
            output: self.output + o.output,
            cache_read: self.cache_read + o.cache_read,
            cache_write_5m: self.cache_write_5m + o.cache_write_5m,
            cache_write_1h: self.cache_write_1h + o.cache_write_1h,
        }
    }
}

/// モデルIDに当たる行を探す。`-`区切りの境界で前方一致した候補のうち最長のものを採る。
///
/// 最長を採るのは、`claude-opus-5-5`が短い`claude-opus-5`に誤って当たるのを防ぐため。
/// 境界を見るのは、`claude-opus-50`のような別モデルに当てないため。
pub fn find_model<'a>(models: &'a [ModelInfo], model_id: &str) -> Option<&'a ModelInfo> {
    models
        .iter()
        .filter(|m| match model_id.strip_prefix(m.model_prefix.as_str()) {
            Some(rest) => rest.is_empty() || rest.starts_with('-'),
            None => false,
        })
        .max_by_key(|m| m.model_prefix.len())
}

/// 単価からコスト（USD）を計算する。
pub fn cost_usd(model: &ModelInfo, u: &TokenUsage) -> f64 {
    (u.input as f64 * model.input
        + u.output as f64 * model.output
        + u.cache_read as f64 * model.cache_read
        + u.cache_write_5m as f64 * model.cache_write_5m
        + u.cache_write_1h as f64 * model.cache_write_1h)
        / 1_000_000.0
}

/// モデルIDからコストを計算する。単価未登録なら`None`を返し、0円と誤表示しないようにする。
pub fn cost_for(models: &[ModelInfo], model_id: &str, u: &TokenUsage) -> Option<f64> {
    find_model(models, model_id).map(|m| cost_usd(m, u))
}

/// キャッシュヒット率。入力が0のときは割合に意味がないため`None`を返す。
pub fn cache_hit_rate(u: &TokenUsage) -> Option<f64> {
    let denom = u.context_tokens();
    (denom > 0).then(|| u.cache_read as f64 / denom as f64)
}

fn official(prefix: &str, name: &str, p: [f64; 5], ctx: u64) -> ModelInfo {
    ModelInfo {
        model_prefix: prefix.to_string(),
        display_name: name.to_string(),
        input: p[0],
        output: p[1],
        cache_read: p[2],
        cache_write_5m: p[3],
        cache_write_1h: p[4],
        context_window: Some(ctx),
        source: ModelSource::Official,
    }
}

/// 2026-09-26にAnthropic公式の料金ページとモデル一覧から取得した初期値。
///
/// 並びは[入力, 出力, キャッシュ読込, キャッシュ作成5分, キャッシュ作成1時間]。
pub fn seed_models() -> Vec<ModelInfo> {
    vec![
        official(
            "claude-fable-5-1",
            "Claude Fable 5.1",
            [10.0, 50.0, 0.25, 12.5, 20.0],
            1_000_000,
        ),
        official(
            "claude-fable-5",
            "Claude Fable 5",
            [10.0, 50.0, 1.0, 12.5, 20.0],
            1_000_000,
        ),
        official(
            "claude-opus-5-5",
            "Claude Opus 5.5",
            [4.0, 20.0, 0.2, 5.0, 8.0],
            1_000_000,
        ),
        official(
            "claude-opus-5",
            "Claude Opus 5",
            [5.0, 25.0, 0.5, 6.25, 10.0],
            1_000_000,
        ),
        official(
            "claude-sonnet-5",
            "Claude Sonnet 5",
            [2.0, 10.0, 0.2, 2.5, 4.0],
            1_000_000,
        ),
        official(
            "claude-haiku-4-5",
            "Claude Haiku 4.5",
            [1.0, 5.0, 0.1, 1.25, 2.0],
            200_000,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn models() -> Vec<ModelInfo> {
        seed_models()
    }

    #[test]
    fn find_model_prefers_longest_boundary_match() {
        let m = models();
        assert_eq!(
            find_model(&m, "claude-opus-5-5").unwrap().model_prefix,
            "claude-opus-5-5"
        );
        assert_eq!(
            find_model(&m, "claude-opus-5").unwrap().model_prefix,
            "claude-opus-5"
        );
        assert_eq!(
            find_model(&m, "claude-haiku-4-5-20251001")
                .unwrap()
                .model_prefix,
            "claude-haiku-4-5"
        );
    }

    #[test]
    fn prefix_without_hyphen_boundary_does_not_match() {
        assert!(find_model(&models(), "claude-opus-50").is_none());
    }

    #[test]
    fn unknown_model_has_no_price() {
        let u = TokenUsage {
            input: 1_000,
            ..Default::default()
        };
        assert_eq!(cost_for(&models(), "<synthetic>", &u), None);
        assert_eq!(cost_for(&models(), "claude-unknown-9", &u), None);
    }

    #[test]
    fn cost_sums_all_token_kinds() {
        let m = models();
        let fable = find_model(&m, "claude-fable-5-1").unwrap();
        let u = TokenUsage {
            input: 1_000_000,
            output: 1_000_000,
            cache_read: 1_000_000,
            cache_write_5m: 1_000_000,
            cache_write_1h: 1_000_000,
        };
        assert!((cost_usd(fable, &u) - (10.0 + 50.0 + 0.25 + 12.5 + 20.0)).abs() < 1e-9);
    }

    #[test]
    fn each_token_kind_uses_its_own_price() {
        let m = models();
        let opus = find_model(&m, "claude-opus-5-5").unwrap();
        let one = 1_000_000;
        assert_eq!(
            cost_usd(
                opus,
                &TokenUsage {
                    input: one,
                    ..Default::default()
                }
            ),
            4.0
        );
        assert_eq!(
            cost_usd(
                opus,
                &TokenUsage {
                    output: one,
                    ..Default::default()
                }
            ),
            20.0
        );
        assert_eq!(
            cost_usd(
                opus,
                &TokenUsage {
                    cache_read: one,
                    ..Default::default()
                }
            ),
            0.2
        );
        assert_eq!(
            cost_usd(
                opus,
                &TokenUsage {
                    cache_write_5m: one,
                    ..Default::default()
                }
            ),
            5.0
        );
        assert_eq!(
            cost_usd(
                opus,
                &TokenUsage {
                    cache_write_1h: one,
                    ..Default::default()
                }
            ),
            8.0
        );
    }

    #[test]
    fn zero_usage_costs_zero() {
        assert_eq!(
            cost_for(&models(), "claude-sonnet-5", &TokenUsage::default()),
            Some(0.0)
        );
    }

    #[test]
    fn context_tokens_include_cache() {
        let u = TokenUsage {
            input: 1,
            output: 100,
            cache_read: 10,
            cache_write_5m: 5,
            cache_write_1h: 2,
        };
        assert_eq!(u.context_tokens(), 18);
    }

    #[test]
    fn plus_adds_each_field() {
        let a = TokenUsage {
            input: 1,
            output: 3,
            cache_read: 4,
            cache_write_5m: 5,
            cache_write_1h: 6,
        };
        let b = TokenUsage {
            input: 10,
            output: 20,
            cache_read: 30,
            cache_write_5m: 40,
            cache_write_1h: 50,
        };
        assert_eq!(
            a.plus(&b),
            TokenUsage {
                input: 11,
                output: 23,
                cache_read: 34,
                cache_write_5m: 45,
                cache_write_1h: 56
            }
        );
    }

    #[test]
    fn cache_hit_rate_handles_zero_and_ratio() {
        assert_eq!(cache_hit_rate(&TokenUsage::default()), None);
        let u = TokenUsage {
            input: 10,
            cache_read: 30,
            cache_write_5m: 10,
            ..Default::default()
        };
        assert!((cache_hit_rate(&u).unwrap() - 0.6).abs() < 1e-9);
    }

    #[test]
    fn source_round_trips_and_rejects_unknown() {
        assert_eq!(ModelSource::parse("official"), Some(ModelSource::Official));
        assert_eq!(
            ModelSource::parse(ModelSource::User.as_str()),
            Some(ModelSource::User)
        );
        assert_eq!(ModelSource::Official.as_str(), "official");
        assert_eq!(ModelSource::parse("other"), None);
    }

    #[test]
    fn seed_contains_official_prices_of_2026_09_26() {
        let m = models();
        let opus55 = find_model(&m, "claude-opus-5-5").unwrap();
        assert_eq!(
            (opus55.input, opus55.output, opus55.cache_read),
            (4.0, 20.0, 0.2)
        );
        assert_eq!(opus55.display_name, "Claude Opus 5.5");
        assert_eq!(opus55.source, ModelSource::Official);
        assert_eq!(
            find_model(&m, "claude-haiku-4-5").unwrap().context_window,
            Some(200_000)
        );
    }
}
