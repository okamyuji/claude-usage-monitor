//! 利用者が設定画面で変える値。
//!
//! 保存形式は`settings`表のkey-valueにし、項目を足してもマイグレーションを要らなくする。

use crate::models::domain::activity::ActivityRules;

/// 画面のテーマ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    /// OSに合わせる。
    System,
    /// ライト。
    Light,
    /// ダーク。
    Dark,
}

impl Theme {
    /// 保存用の文字列。
    pub fn as_str(self) -> &'static str {
        match self {
            Theme::System => "system",
            Theme::Light => "light",
            Theme::Dark => "dark",
        }
    }

    /// 保存用の文字列から戻す。
    pub fn parse(s: &str) -> Option<Self> {
        [Theme::System, Theme::Light, Theme::Dark]
            .into_iter()
            .find(|t| t.as_str() == s)
    }
}

/// 設定値。既定値はspecの値にする。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    /// 使用量の取得間隔（秒）。使用量APIは約2分に1回を超えると429を返すため、120〜600に限る。
    pub usage_interval_secs: u64,
    /// 通知と推移グラフの横線に使う閾値（%）。
    pub notify_threshold_percent: f64,
    /// 時系列の保持日数。
    pub retention_days: i64,
    /// ヘッドレスとサブエージェントを稼働中とする秒数。
    pub headless_active_secs: i64,
    /// ジョブを稼働中とする分数。
    pub job_active_mins: i64,
    /// テーマ。
    pub theme: Theme,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            usage_interval_secs: 120,
            notify_threshold_percent: 80.0,
            retention_days: 90,
            headless_active_secs: 120,
            job_active_mins: 10,
            theme: Theme::System,
        }
    }
}

/// 設定値の検証エラー。
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SettingsError {
    /// 範囲外。
    #[error("{name}は{min}〜{max}の範囲で指定してください")]
    OutOfRange {
        /// 項目名。
        name: &'static str,
        /// 下限。
        min: f64,
        /// 上限。
        max: f64,
    },
}

fn check(name: &'static str, v: f64, min: f64, max: f64) -> Result<(), SettingsError> {
    if (min..=max).contains(&v) {
        Ok(())
    } else {
        Err(SettingsError::OutOfRange { name, min, max })
    }
}

impl Settings {
    /// 範囲を検証する。画面で保存する前に呼び、範囲外の値をDBに入れない。
    pub fn validate(&self) -> Result<(), SettingsError> {
        check(
            "取得間隔（秒）",
            self.usage_interval_secs as f64,
            120.0,
            600.0,
        )?;
        check("通知閾値（%）", self.notify_threshold_percent, 1.0, 100.0)?;
        check("保持日数", self.retention_days as f64, 1.0, 3650.0)?;
        check(
            "ヘッドレスの稼働判定（秒）",
            self.headless_active_secs as f64,
            10.0,
            3600.0,
        )?;
        check(
            "ジョブの稼働判定（分）",
            self.job_active_mins as f64,
            1.0,
            1440.0,
        )
    }

    /// 保存済みのkey-valueから作る。読めない値や範囲外の値はその項目だけ既定値に戻し、他の項目は生かす。
    pub fn from_pairs(pairs: &[(String, String)]) -> Self {
        pairs
            .iter()
            .fold(Settings::default(), |s, (k, v)| match s.with(k, v) {
                Some(next) if next.validate().is_ok() => next,
                _ => s,
            })
    }

    fn with(&self, key: &str, value: &str) -> Option<Settings> {
        let mut n = *self;
        match key {
            "usage_interval_secs" => n.usage_interval_secs = value.parse().ok()?,
            "notify_threshold_percent" => n.notify_threshold_percent = value.parse().ok()?,
            "retention_days" => n.retention_days = value.parse().ok()?,
            "headless_active_secs" => n.headless_active_secs = value.parse().ok()?,
            "job_active_mins" => n.job_active_mins = value.parse().ok()?,
            "theme" => n.theme = Theme::parse(value)?,
            _ => return None,
        }
        Some(n)
    }

    /// 保存用のkey-value。
    pub fn to_pairs(&self) -> Vec<(&'static str, String)> {
        vec![
            ("usage_interval_secs", self.usage_interval_secs.to_string()),
            (
                "notify_threshold_percent",
                self.notify_threshold_percent.to_string(),
            ),
            ("retention_days", self.retention_days.to_string()),
            (
                "headless_active_secs",
                self.headless_active_secs.to_string(),
            ),
            ("job_active_mins", self.job_active_mins.to_string()),
            ("theme", self.theme.as_str().to_string()),
        ]
    }

    /// 稼働中の判定に使う時間。
    pub fn rules(&self) -> ActivityRules {
        ActivityRules {
            headless_active: chrono::Duration::seconds(self.headless_active_secs),
            job_active: chrono::Duration::minutes(self.job_active_mins),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(kv: &[(&str, &str)]) -> Vec<(String, String)> {
        kv.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn defaults_match_spec() {
        let s = Settings::default();
        assert_eq!(
            (
                s.usage_interval_secs,
                s.notify_threshold_percent,
                s.retention_days,
                s.headless_active_secs,
                s.job_active_mins,
                s.theme
            ),
            (120, 80.0, 90, 120, 10, Theme::System)
        );
        assert!(s.validate().is_ok());
    }

    #[test]
    fn validate_reports_the_field_out_of_range() {
        let cases = [
            Settings {
                usage_interval_secs: 119,
                ..Settings::default()
            },
            Settings {
                usage_interval_secs: 601,
                ..Settings::default()
            },
            Settings {
                notify_threshold_percent: 0.5,
                ..Settings::default()
            },
            Settings {
                retention_days: 0,
                ..Settings::default()
            },
            Settings {
                headless_active_secs: 9,
                ..Settings::default()
            },
            Settings {
                job_active_mins: 1441,
                ..Settings::default()
            },
        ];
        let names: Vec<String> = cases
            .iter()
            .map(|s| s.validate().unwrap_err().to_string())
            .collect();
        assert!(
            names[0].contains("取得間隔") && names[0].contains("120〜600"),
            "{names:?}"
        );
        assert!(names[1].contains("取得間隔"));
        assert!(names[2].contains("通知閾値"));
        assert!(names[3].contains("保持日数"));
        assert!(names[4].contains("ヘッドレス"));
        assert!(names[5].contains("ジョブ"));
        assert!(
            Settings {
                usage_interval_secs: 120,
                notify_threshold_percent: 100.0,
                retention_days: 3650,
                headless_active_secs: 3600,
                job_active_mins: 1,
                ..Settings::default()
            }
            .validate()
            .is_ok()
        );
        assert!(
            Settings {
                usage_interval_secs: 600,
                ..Settings::default()
            }
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn pairs_round_trip() {
        let s = Settings {
            usage_interval_secs: 300,
            notify_threshold_percent: 90.0,
            retention_days: 30,
            headless_active_secs: 300,
            job_active_mins: 5,
            theme: Theme::Dark,
        };
        let stored: Vec<(String, String)> = s
            .to_pairs()
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        assert_eq!(Settings::from_pairs(&stored), s);
    }

    #[test]
    fn stored_old_default_falls_back_to_new_default() {
        let old = Settings::from_pairs(&pairs(&[("usage_interval_secs", "60")]));
        assert_eq!(old.usage_interval_secs, 120);
        let kept = Settings::from_pairs(&pairs(&[("usage_interval_secs", "300")]));
        assert_eq!(kept.usage_interval_secs, 300);
    }

    #[test]
    fn broken_or_unknown_values_fall_back_per_field() {
        let s = Settings::from_pairs(&pairs(&[
            ("usage_interval_secs", "abc"),
            ("notify_threshold_percent", "500"),
            ("retention_days", "30"),
            ("theme", "purple"),
            ("unknown_key", "1"),
        ]));
        assert_eq!(s.usage_interval_secs, 120);
        assert_eq!(s.notify_threshold_percent, 80.0);
        assert_eq!(s.retention_days, 30);
        assert_eq!(s.theme, Theme::System);
    }

    #[test]
    fn theme_strings() {
        for t in [Theme::System, Theme::Light, Theme::Dark] {
            assert_eq!(Theme::parse(t.as_str()), Some(t));
        }
        assert_eq!(Theme::parse("x"), None);
    }

    #[test]
    fn rules_follow_settings() {
        let r = Settings {
            headless_active_secs: 300,
            job_active_mins: 20,
            ..Settings::default()
        }
        .rules();
        assert_eq!(r.headless_active, chrono::Duration::seconds(300));
        assert_eq!(r.job_active, chrono::Duration::minutes(20));
    }
}
