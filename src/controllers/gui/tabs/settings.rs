//! 設定タブ。取得間隔、通知閾値、保持期間、稼働判定、テーマ、モデル情報。
use crate::controllers::gui::app::GuiDeps;
use crate::models::domain::display::fmt_clock;
use crate::models::domain::pricing::{ModelInfo, ModelSource};
use crate::models::domain::records::FetchResult;
use crate::models::domain::settings::{Settings, Theme};
use crate::models::ports::RepoError;

/// 料金ページ。
pub const PRICING_URL: &str = "https://platform.claude.com/docs/en/about-claude/pricing";
/// モデル一覧。
pub const MODELS_URL: &str = "https://platform.claude.com/docs/en/models/overview";

/// 設定の入力欄。数を文字列で持ち、入力途中の値（空や`1.`）を受け付ける。
#[derive(Debug, Clone, PartialEq)]
pub struct SettingsDraft {
    /// 取得間隔（秒）。
    pub interval: String,
    /// 通知閾値（%）。
    pub threshold: String,
    /// 保持日数。
    pub retention: String,
    /// ヘッドレスの稼働判定（秒）。
    pub headless: String,
    /// ジョブの稼働判定（分）。
    pub job: String,
    /// テーマ。
    pub theme: Theme,
}

impl Default for SettingsDraft {
    fn default() -> Self {
        Self::from_settings(&Settings::default())
    }
}

fn num<T: std::str::FromStr>(v: &str, name: &str) -> Result<T, String> {
    v.trim()
        .parse()
        .map_err(|_| format!("{name}を数で入力してください"))
}

impl SettingsDraft {
    /// 保存済みの設定から作る。
    pub fn from_settings(s: &Settings) -> Self {
        Self {
            interval: s.usage_interval_secs.to_string(),
            threshold: s.notify_threshold_percent.to_string(),
            retention: s.retention_days.to_string(),
            headless: s.headless_active_secs.to_string(),
            job: s.job_active_mins.to_string(),
            theme: s.theme,
        }
    }

    /// 入力を設定に直し、範囲も検証する。
    pub fn parse(&self) -> Result<Settings, String> {
        let s = Settings {
            usage_interval_secs: num(&self.interval, "取得間隔（秒）")?,
            notify_threshold_percent: num(&self.threshold, "通知閾値（%）")?,
            retention_days: num(&self.retention, "保持日数")?,
            headless_active_secs: num(&self.headless, "ヘッドレスの稼働判定（秒）")?,
            job_active_mins: num(&self.job, "ジョブの稼働判定（分）")?,
            theme: self.theme,
        };
        s.validate().map_err(|e| e.to_string())?;
        Ok(s)
    }
}

/// モデル情報の入力欄。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelDraft {
    /// モデルID（前方一致の接頭辞）。
    pub prefix: String,
    /// 表示名。
    pub name: String,
    /// 入力の単価（USD/MTok）。
    pub input: String,
    /// 出力の単価。
    pub output: String,
    /// キャッシュ読込の単価。
    pub cache_read: String,
    /// キャッシュ作成5分の単価。
    pub cache_write_5m: String,
    /// キャッシュ作成1時間の単価。
    pub cache_write_1h: String,
    /// コンテキスト長。空なら不明。
    pub context: String,
}

impl ModelDraft {
    /// 既存のモデル情報から作る。
    pub fn from_model(m: &ModelInfo) -> Self {
        Self {
            prefix: m.model_prefix.clone(),
            name: m.display_name.clone(),
            input: m.input.to_string(),
            output: m.output.to_string(),
            cache_read: m.cache_read.to_string(),
            cache_write_5m: m.cache_write_5m.to_string(),
            cache_write_1h: m.cache_write_1h.to_string(),
            context: m.context_window.map(|c| c.to_string()).unwrap_or_default(),
        }
    }

    /// 入力をモデル情報に直す。利用者の編集なので`source`は`User`にする。
    pub fn parse(&self) -> Result<ModelInfo, String> {
        Ok(ModelInfo {
            model_prefix: self.prefix.trim().to_string(),
            display_name: self.name.trim().to_string(),
            input: num(&self.input, "入力の単価")?,
            output: num(&self.output, "出力の単価")?,
            cache_read: num(&self.cache_read, "キャッシュ読込の単価")?,
            cache_write_5m: num(&self.cache_write_5m, "キャッシュ作成5分の単価")?,
            cache_write_1h: num(&self.cache_write_1h, "キャッシュ作成1時間の単価")?,
            context_window: match self.context.trim() {
                "" => None,
                c => Some(num(c, "コンテキスト長")?),
            },
            source: ModelSource::User,
        })
    }
}

/// モデル1行の表示。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRowView {
    /// モデルID。
    pub prefix: String,
    /// 表示名。
    pub name: String,
    /// 単価（入力 / 出力 / キャッシュ読込 / 作成5分 / 作成1時間）。
    pub prices: String,
    /// コンテキスト長。
    pub context: String,
    /// 取得元（公式、手入力）。
    pub source: String,
}

/// 設定タブのViewModel。
#[derive(Debug, Clone, PartialEq)]
pub struct SettingsVm {
    /// モデル情報。
    pub models: Vec<ModelRowView>,
    /// 公式情報の最終更新。
    pub catalog: String,
    /// 直前の操作の結果。
    pub message: Option<String>,
    /// 手動更新の実行中か。
    pub refreshing: bool,
    /// 料金ページ。
    pub pricing_url: &'static str,
    /// モデル一覧。
    pub models_url: &'static str,
    /// ログイン時の自動起動。状態を読めなければ`None`。
    pub autostart: Option<bool>,
}

/// 設定タブの操作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsAction {
    /// 設定を保存する。
    Save,
    /// モデル情報の編集を始める。
    EditModel(String),
    /// 編集したモデル情報を保存する。
    SaveModel,
    /// 編集をやめる。
    CancelModel,
    /// 公式ページから今すぐ取り直す。
    RefreshCatalog,
    /// 自動起動を切り替える。
    SetAutostart(bool),
}

/// 設定タブを作る。
pub fn build(
    deps: &GuiDeps,
    message: Option<String>,
    refreshing: bool,
) -> Result<SettingsVm, RepoError> {
    let now = deps.clock.now();
    let catalog = match deps.logs.recent("catalog", 1)?.into_iter().next() {
        None => "まだ取得していません".into(),
        Some(e) if e.result == FetchResult::Ok => {
            format!("{} に更新", fmt_clock(e.at, now, deps.tz))
        }
        Some(e) => format!("{} に失敗: {}", fmt_clock(e.at, now, deps.tz), e.message),
    };
    Ok(SettingsVm {
        models: deps
            .models
            .all()?
            .iter()
            .map(|m| ModelRowView {
                prefix: m.model_prefix.clone(),
                name: m.display_name.clone(),
                prices: format!(
                    "{} / {} / {} / {} / {}",
                    m.input, m.output, m.cache_read, m.cache_write_5m, m.cache_write_1h
                ),
                context: m
                    .context_window
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "不明".into()),
                source: match m.source {
                    ModelSource::Official => "公式".into(),
                    ModelSource::User => "手入力".into(),
                },
            })
            .collect(),
        catalog,
        message,
        refreshing,
        pricing_url: PRICING_URL,
        models_url: MODELS_URL,
        autostart: deps.autostart.is_enabled().ok(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::domain::pricing::{ModelSource, seed_models};

    #[test]
    fn catalog_status_tells_success_from_failure() {
        use crate::models::domain::records::FetchLogEntry;
        use crate::models::ports::FetchLogRepo;
        use crate::test_support::{FakeCreds, FakeDaemon, FixedClock, gui_deps, temp_store};
        use chrono::{Duration, TimeZone, Utc};
        use std::sync::Arc;
        let now = Utc.with_ymd_and_hms(2026, 9, 26, 3, 0, 0).unwrap();
        let (_d, s) = temp_store();
        let s = Arc::new(s);
        let home = tempfile::tempdir().unwrap();
        let deps = gui_deps(
            s.clone(),
            Arc::new(FixedClock::at(now)),
            home.path(),
            Arc::new(FakeCreds(std::collections::HashMap::new())),
            Arc::new(FakeDaemon::default()),
        );
        let log = |at, result, message: &str| {
            s.log(&FetchLogEntry {
                target: "catalog".into(),
                at,
                result,
                http_status: None,
                message: message.into(),
            })
            .unwrap()
        };
        log(now - Duration::minutes(2), FetchResult::Ok, "12件を更新");
        let ok = build(&deps, None, false).unwrap().catalog;
        assert!(ok.ends_with("に更新"), "{ok}");
        log(
            now - Duration::minutes(1),
            FetchResult::Failed,
            "接続できません",
        );
        let ng = build(&deps, None, false).unwrap().catalog;
        assert!(ng.ends_with("に失敗: 接続できません"), "{ng}");
    }

    #[test]
    fn settings_draft_round_trip_and_errors() {
        let s = Settings {
            usage_interval_secs: 120,
            ..Settings::default()
        };
        let d = SettingsDraft::from_settings(&s);
        assert_eq!(d.interval, "120");
        assert_eq!(d.parse().unwrap(), s);
        assert_eq!(
            SettingsDraft::default(),
            SettingsDraft::from_settings(&Settings::default())
        );
        let bad = SettingsDraft {
            interval: "abc".into(),
            ..d.clone()
        };
        assert_eq!(
            bad.parse().unwrap_err(),
            "取得間隔（秒）を数で入力してください"
        );
        let out = SettingsDraft {
            threshold: "0".into(),
            ..d
        };
        assert!(out.parse().unwrap_err().contains("通知閾値"));
    }

    #[test]
    fn model_draft_round_trip_and_errors() {
        let m = seed_models()[0].clone();
        let d = ModelDraft::from_model(&m);
        let back = d.parse().unwrap();
        assert_eq!(
            (
                back.model_prefix,
                back.input,
                back.context_window,
                back.source
            ),
            (
                m.model_prefix.clone(),
                m.input,
                m.context_window,
                ModelSource::User
            )
        );
        let unknown_ctx = ModelDraft {
            context: "".into(),
            ..d.clone()
        };
        assert_eq!(unknown_ctx.parse().unwrap().context_window, None);
        let bad = ModelDraft {
            output: "x".into(),
            ..d
        };
        assert_eq!(bad.parse().unwrap_err(), "出力の単価を数で入力してください");
    }
}
