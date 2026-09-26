//! セッションのJSONLの場所の特定と、末尾から読むための位置の計算。

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// セッションに属するJSONL。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SessionFiles {
    /// 本体。
    pub main: Option<PathBuf>,
    /// サブエージェント（エージェントIDとパス）。
    pub subagents: Vec<(String, PathBuf)>,
    /// ジョブの状態変化。
    pub timeline: Option<PathBuf>,
}

fn dirs_in(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.is_dir())
                .collect()
        })
        .unwrap_or_default()
}

fn subagent_files(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
                .filter_map(|p| {
                    let id = p.file_stem()?.to_str()?.strip_prefix("agent-")?.to_string();
                    Some((id, p))
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// セッションIDからJSONLを探す。プロジェクトのディレクトリ名はcwdの変換規則に依存するため、全プロジェクトを見て探す。
pub fn find_session_files(
    config_dir: &Path,
    session_id: &str,
    job_id: Option<&str>,
) -> SessionFiles {
    let mut f = SessionFiles::default();
    for proj in dirs_in(&config_dir.join("projects")) {
        let main = proj.join(format!("{session_id}.jsonl"));
        if main.is_file() {
            f.main = Some(main);
            f.subagents = subagent_files(&proj.join(session_id).join("subagents"));
            break;
        }
    }
    f.timeline = job_id
        .map(|j| config_dir.join("jobs").join(j).join("timeline.jsonl"))
        .filter(|p| p.is_file());
    f
}

/// 末尾から`max_lines`行を読むための開始位置。ファイル全体を読まずに、末尾から8KBずつさかのぼって改行を数える。
pub fn tail_offset(path: &Path, max_lines: usize) -> io::Result<u64> {
    let mut file = File::open(path)?;
    let mut pos = file.metadata()?.len();
    let mut newlines = 0usize;
    let mut buf = [0u8; 8192];
    while pos > 0 {
        let n = (pos as usize).min(buf.len());
        pos -= n as u64;
        file.seek(SeekFrom::Start(pos))?;
        file.read_exact(&mut buf[..n])?;
        if let Some(i) = buf[..n].iter().rposition(|b| {
            newlines += usize::from(*b == b'\n');
            newlines > max_lines
        }) {
            return Ok(pos + i as u64 + 1);
        }
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn finds_main_subagents_and_timeline() {
        let d = tempfile::tempdir().unwrap();
        let proj = d.path().join("projects/-w");
        std::fs::create_dir_all(proj.join("s1/subagents")).unwrap();
        std::fs::write(proj.join("s1.jsonl"), "").unwrap();
        std::fs::write(proj.join("s1/subagents/agent-a1.jsonl"), "").unwrap();
        std::fs::write(proj.join("s1/subagents/agent-a1.meta.json"), "{}").unwrap();
        std::fs::write(proj.join("other.jsonl"), "").unwrap();
        std::fs::create_dir_all(d.path().join("jobs/j1")).unwrap();
        std::fs::write(d.path().join("jobs/j1/timeline.jsonl"), "").unwrap();
        let f = find_session_files(d.path(), "s1", Some("j1"));
        assert_eq!(f.main, Some(proj.join("s1.jsonl")));
        assert_eq!(
            f.subagents,
            [("a1".to_string(), proj.join("s1/subagents/agent-a1.jsonl"))]
        );
        assert_eq!(f.timeline, Some(d.path().join("jobs/j1/timeline.jsonl")));
        let none = find_session_files(d.path(), "missing", Some("nojob"));
        assert_eq!(
            (none.main, none.subagents.len(), none.timeline),
            (None, 0, None)
        );
        assert_eq!(
            find_session_files(&d.path().join("nothing"), "s1", None).main,
            None
        );
    }

    #[test]
    fn tail_starts_near_end_of_large_file() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("big.jsonl");
        let mut f = std::fs::File::create(&p).unwrap();
        for i in 0..20_000 {
            writeln!(f, "line-{i:05}").unwrap();
        }
        let off = tail_offset(&p, 3).unwrap();
        let mut got = vec![];
        crate::models::gateways::jsonl::read_new_lines(&p, off, |l| got.push(l.to_string()))
            .unwrap();
        assert_eq!(got, ["line-19997", "line-19998", "line-19999"]);
        assert_eq!(tail_offset(&p, 100_000).unwrap(), 0);
    }

    #[test]
    fn tail_of_small_or_empty_file_is_zero() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("s.jsonl");
        std::fs::write(&p, "a\nb\n").unwrap();
        assert_eq!(tail_offset(&p, 5).unwrap(), 0);
        assert_eq!(tail_offset(&p, 1).unwrap(), 2);
        std::fs::write(&p, "").unwrap();
        assert_eq!(tail_offset(&p, 5).unwrap(), 0);
        assert!(tail_offset(&d.path().join("none"), 5).is_err());
    }
}
