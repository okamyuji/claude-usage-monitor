//! 失敗時の再試行間隔。429や通信エラーのときにAPIを叩き続けないため、間隔を倍々に延ばす。
use chrono::{DateTime, Duration, Utc};

/// 指数バックオフの状態。
#[derive(Debug, Clone, PartialEq)]
pub struct Backoff {
    base: Duration,
    max: Duration,
    delay: Option<Duration>,
    next_at: Option<DateTime<Utc>>,
}

impl Backoff {
    /// 最初の待ち時間と上限を指定して作る。
    pub fn new(base: Duration, max: Duration) -> Self {
        Self {
            base,
            max,
            delay: None,
            next_at: None,
        }
    }

    /// 今試してよいか。
    pub fn ready(&self, now: DateTime<Utc>) -> bool {
        self.next_at.is_none_or(|t| now >= t)
    }

    /// 失敗を記録し、次の試行時刻を延ばす。
    pub fn on_failure(&mut self, now: DateTime<Utc>) {
        let next = match self.delay {
            None => self.base,
            Some(d) => (d * 2).min(self.max),
        };
        self.delay = Some(next);
        self.next_at = Some(now + next);
    }

    /// 成功を記録し、待ちを解く。
    pub fn on_success(&mut self) {
        self.delay = None;
        self.next_at = None;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn t(s: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 26, 0, 0, 0).unwrap() + chrono::Duration::seconds(s)
    }

    #[test]
    fn doubles_until_max_and_resets_on_success() {
        let mut b = Backoff::new(Duration::seconds(60), Duration::seconds(600));
        assert!(b.ready(t(0)));
        b.on_failure(t(0));
        assert!(!b.ready(t(59)));
        assert!(b.ready(t(60)));
        b.on_failure(t(60));
        assert!(!b.ready(t(179)) && b.ready(t(180)));
        for _ in 0..10 {
            b.on_failure(t(1000));
        }
        assert!(b.ready(t(1600)) && !b.ready(t(1599)));
        b.on_success();
        assert!(b.ready(t(0)));
        b.on_failure(t(0));
        assert!(b.ready(t(60)));
    }
}
