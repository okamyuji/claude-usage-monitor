//! 再生。選んだセッションのターンを時刻順にたどる。
use crate::controllers::gui::app::GuiDeps;
use crate::controllers::gui::dashboard::sessions::{TURN_LIMIT, TurnItem, turn_items};
use crate::models::ports::RepoError;
use chrono::{DateTime, Duration, Utc};
use std::collections::{HashMap, HashSet};

/// 現在位置の直前に並べるターンの数。
const RECENT: usize = 10;

/// 再生の状態。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayState {
    /// 現在のターンの位置（0始まり）。
    pub position: usize,
    /// 再生中か。
    pub playing: bool,
    /// 速度（1、4、16）。
    pub speed: u32,
    /// 最後に進めた時刻。
    pub last_step: Option<DateTime<Utc>>,
}

impl Default for ReplayState {
    fn default() -> Self {
        Self {
            position: 0,
            playing: false,
            speed: 1,
            last_step: None,
        }
    }
}

/// 再生の操作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayAction {
    /// 位置を指定する（スライダー）。
    Seek(usize),
    /// 相対に動かす（前後のボタン）。
    Step(i64),
    /// 再生と一時停止を切り替える。
    TogglePlay,
    /// 速度を変える。
    SetSpeed(u32),
}

/// 再生のViewModel。
#[derive(Debug, Clone, PartialEq)]
pub struct ReplayVm {
    /// 現在位置（範囲内に丸めた値）。
    pub position: usize,
    /// ターン数。
    pub len: usize,
    /// 現在のターン。
    pub current: Option<TurnItem>,
    /// 現在位置までの直近のターン。
    pub recent: Vec<TurnItem>,
    /// 再生中か。
    pub playing: bool,
    /// 速度。
    pub speed: u32,
}

/// 再生の表示を作る。
pub fn build(deps: &GuiDeps, session_id: &str, st: &ReplayState) -> Result<ReplayVm, RepoError> {
    let now = deps.clock.now();
    let ids = [session_id.to_string()];
    let names: HashMap<String, String> = deps
        .sessions
        .subagents(&ids)?
        .into_iter()
        .map(|a| (a.agent_id, a.agent_type.unwrap_or_default()))
        .collect();
    let models = deps.models.all()?;
    let items = turn_items(
        &deps.sessions.turns(session_id, TURN_LIMIT)?,
        &models,
        &names,
        &HashSet::new(),
        now,
        deps.tz,
    );
    let position = st.position.min(items.len().saturating_sub(1));
    Ok(ReplayVm {
        position,
        len: items.len(),
        current: items.get(position).cloned(),
        recent: items
            .iter()
            .take(position + 1)
            .rev()
            .take(RECENT)
            .rev()
            .cloned()
            .collect(),
        playing: st.playing,
        speed: st.speed,
    })
}

/// 操作を状態に反映する。範囲外へ進めた位置は`build`で`len-1`に丸める。
pub fn handle(st: &mut ReplayState, a: ReplayAction) {
    match a {
        ReplayAction::Seek(p) => st.position = p,
        ReplayAction::Step(d) => st.position = (st.position as i64 + d).max(0) as usize,
        ReplayAction::TogglePlay => {
            st.playing = !st.playing;
            st.last_step = None;
        }
        ReplayAction::SetSpeed(s) => st.speed = s,
    }
}

/// 再生中なら、前回からの経過時間×速度だけ位置を進める。最後まで来たら止める。
pub fn advance(st: &mut ReplayState, len: usize, now: DateTime<Utc>) {
    if !st.playing {
        return;
    }
    let Some(last) = st.last_step else {
        st.last_step = Some(now);
        return;
    };
    let steps = (now - last).num_milliseconds() * st.speed as i64 / 1000;
    if steps <= 0 {
        return;
    }
    st.last_step = Some(last + Duration::milliseconds(steps * 1000 / st.speed as i64));
    st.position = (st.position + steps as usize).min(len.saturating_sub(1));
    if st.position + 1 >= len {
        st.playing = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::domain::transcript::SessionKind;
    use crate::test_support::{
        FakeCreds, FakeDaemon, FixedClock, gui_deps, seed_session, seed_turn, temp_store, tokens,
    };
    use chrono::{Duration, TimeZone};
    use std::collections::HashMap;
    use std::sync::Arc;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 26, 3, 0, 0).unwrap()
    }

    fn deps() -> (tempfile::TempDir, tempfile::TempDir, GuiDeps) {
        let (d, s) = temp_store();
        seed_session(&s, "s1", SessionKind::Interactive, None, now());
        for i in 0..20 {
            seed_turn(
                &s,
                "s1",
                "",
                &format!("m{i:02}"),
                now() - Duration::seconds(100 - i),
                None,
                "text",
                tokens(0, 0),
            );
        }
        let home = tempfile::tempdir().unwrap();
        let deps = gui_deps(
            Arc::new(s),
            Arc::new(FixedClock::at(now())),
            home.path(),
            Arc::new(FakeCreds(HashMap::new())),
            Arc::new(FakeDaemon::default()),
        );
        (d, home, deps)
    }

    #[test]
    fn build_shows_current_and_recent_turns() {
        let (_d, _h, deps) = deps();
        let vm = build(
            &deps,
            "s1",
            &ReplayState {
                position: 12,
                ..ReplayState::default()
            },
        )
        .unwrap();
        assert_eq!((vm.len, vm.position), (20, 12));
        assert_eq!(vm.current.unwrap().summary, "summary-m12");
        assert_eq!(
            vm.recent.first().map(|t| t.summary.as_str()),
            Some("summary-m03")
        );
        assert_eq!(vm.recent.len(), 10);
        assert_eq!(
            build(
                &deps,
                "s1",
                &ReplayState {
                    position: 99,
                    ..ReplayState::default()
                }
            )
            .unwrap()
            .position,
            19
        );
        let empty = build(&deps, "none", &ReplayState::default()).unwrap();
        assert!(empty.current.is_none() && empty.len == 0);
    }

    #[test]
    fn advance_steps_by_speed_and_stops_at_end() {
        let mut st = ReplayState {
            playing: true,
            speed: 4,
            last_step: Some(now()),
            ..ReplayState::default()
        };
        advance(&mut st, 20, now() + Duration::milliseconds(1000));
        assert_eq!(st.position, 4);
        advance(&mut st, 20, now() + Duration::milliseconds(1100));
        assert_eq!(st.position, 4);
        st.speed = 16;
        advance(&mut st, 20, now() + Duration::seconds(10));
        assert_eq!((st.position, st.playing), (19, false));
        let mut paused = ReplayState {
            position: 3,
            ..ReplayState::default()
        };
        advance(&mut paused, 20, now());
        assert_eq!(paused.position, 3);
        let mut first = ReplayState {
            playing: true,
            ..ReplayState::default()
        };
        advance(&mut first, 20, now());
        assert_eq!((first.position, first.last_step), (0, Some(now())));
    }
}
