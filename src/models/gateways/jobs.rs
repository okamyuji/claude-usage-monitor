//! `<config>/jobs/<jobId>/state.json`（バックグラウンドジョブ）と、
//! サブエージェントの`agent-<id>.meta.json`の読み取り。
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::io;
use std::path::Path;

/// ジョブの状態ファイル1つ。
#[derive(Debug, Clone, PartialEq)]
pub struct JobStateFile {
    /// ジョブID（ディレクトリ名）。
    pub job_id: String,
    /// 結び付くセッションID。
    pub session_id: Option<String>,
    /// 表示名。
    pub name: Option<String>,
    /// 状態。
    pub state: String,
    /// 人が読める進捗説明。
    pub detail: Option<String>,
    /// 実行中タスク数。
    pub in_flight_tasks: i64,
    /// Token数。
    pub tokens: Option<i64>,
    /// 作業ディレクトリ。
    pub cwd: Option<String>,
    /// 作成時刻。
    pub created_at: Option<DateTime<Utc>>,
    /// 更新時刻。
    pub updated_at: Option<DateTime<Utc>>,
}

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::to_string)
}

fn time(v: &Value, k: &str) -> Option<DateTime<Utc>> {
    v.get(k)
        .and_then(Value::as_str)
        .and_then(|t| t.parse().ok())
}

/// 全ジョブを読む。`state`のないファイルと壊れたファイルは飛ばす。
pub fn read_jobs(config_dir: &Path) -> io::Result<Vec<JobStateFile>> {
    let dir = config_dir.join("jobs");
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir)? {
        let d = e?.path();
        let Some(job_id) = d.file_name().and_then(|n| n.to_str()).map(str::to_string) else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(d.join("state.json")) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let Some(state) = s(&v, "state") else {
            continue;
        };
        out.push(JobStateFile {
            job_id,
            session_id: s(&v, "sessionId"),
            name: s(&v, "name"),
            state,
            detail: s(&v, "detail"),
            in_flight_tasks: v
                .pointer("/inFlight/tasks")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            tokens: v.get("tokens").and_then(Value::as_i64),
            cwd: s(&v, "cwd"),
            created_at: time(&v, "createdAt"),
            updated_at: time(&v, "updatedAt"),
        });
    }
    Ok(out)
}

/// サブエージェントのメタ情報。
#[derive(Debug, Clone, PartialEq)]
pub struct SubagentMetaFile {
    /// 種類。
    pub agent_type: Option<String>,
    /// 依頼内容の説明。
    pub description: Option<String>,
    /// 親の`Agent`呼び出しID。
    pub tool_use_id: Option<String>,
    /// 入れ子の深さ。記録元が文字列と数値の両方で書くため、どちらも読む。
    pub spawn_depth: Option<i64>,
}

/// `agent-<id>.jsonl`の隣にある`agent-<id>.meta.json`を読む。なければ`None`。
pub fn read_subagent_meta(jsonl_path: &Path) -> Option<SubagentMetaFile> {
    let meta = jsonl_path.with_extension("meta.json");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(meta).ok()?).ok()?;
    let depth = v
        .get("spawnDepth")
        .and_then(|d| d.as_i64().or_else(|| d.as_str()?.parse().ok()));
    Some(SubagentMetaFile {
        agent_type: s(&v, "agentType"),
        description: s(&v, "description"),
        tool_use_id: s(&v, "toolUseId"),
        spawn_depth: depth,
    })
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_job_state() {
        let d = tempfile::tempdir().unwrap();
        let j = d.path().join("jobs/b2732f77");
        std::fs::create_dir_all(&j).unwrap();
        std::fs::write(d.path().join("jobs/pins.json"), "[]").unwrap();
        std::fs::write(j.join("state.json"), r#"{"state":"working","detail":"1/8 done","inFlight":{"tasks":2},"tokens":1200,"sessionId":"s9","name":"実験","cwd":"/w","createdAt":"2026-09-26T00:00:00.000Z","updatedAt":"2026-09-26T00:05:00.000Z"}"#).unwrap();
        let v = read_jobs(d.path()).unwrap();
        assert_eq!(v.len(), 1);
        let job = &v[0];
        assert_eq!(
            (
                job.job_id.as_str(),
                job.state.as_str(),
                job.in_flight_tasks,
                job.tokens
            ),
            ("b2732f77", "working", 2, Some(1200))
        );
        assert_eq!(job.session_id.as_deref(), Some("s9"));
        assert!(job.updated_at.is_some());
    }

    #[test]
    fn job_without_state_field_is_skipped() {
        let d = tempfile::tempdir().unwrap();
        let j = d.path().join("jobs/x");
        std::fs::create_dir_all(&j).unwrap();
        std::fs::write(j.join("state.json"), "{}").unwrap();
        assert!(read_jobs(d.path()).unwrap().is_empty());
    }

    #[test]
    fn reads_subagent_meta_next_to_jsonl() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("agent-a1.jsonl");
        std::fs::write(d.path().join("agent-a1.meta.json"), r#"{"agentType":"go-reviewer","description":"レビュー","toolUseId":"toolu_1","spawnDepth":"1"}"#).unwrap();
        let m = read_subagent_meta(&p).unwrap();
        assert_eq!(
            (
                m.agent_type.as_deref(),
                m.tool_use_id.as_deref(),
                m.spawn_depth
            ),
            (Some("go-reviewer"), Some("toolu_1"), Some(1))
        );
        std::fs::write(d.path().join("agent-a1.meta.json"), r#"{"spawnDepth":2}"#).unwrap();
        assert_eq!(read_subagent_meta(&p).unwrap().spawn_depth, Some(2));
        assert!(read_subagent_meta(&d.path().join("agent-none.jsonl")).is_none());
    }
}
