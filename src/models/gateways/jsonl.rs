//! セッションJSONLを差分だけ読む。
//!
//! JSONLは数十MBに育つため、全体をメモリに読まず1行ずつコールバックへ渡す。
//! 常駐プロセスのメモリをファイルサイズに比例させないため。
use std::fs::File;
use std::io::{self, BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// 1回の読み取り結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadOutcome {
    /// 次回の読み始め位置。改行で終わった行の直後を指す。
    pub new_offset: u64,
    /// ファイルが縮んでいたため先頭から読み直したか。
    pub reset: bool,
    /// 渡した行数（空行を除く）。
    pub lines: usize,
}

/// ファイルを開き、読み始め位置へ移動する。ファイルが`offset`より短ければ縮んだとみなして先頭から読む。
fn open_at(path: &Path, offset: u64) -> io::Result<(BufReader<File>, u64, bool)> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let (start, reset) = if len < offset {
        (0, true)
    } else {
        (offset, false)
    };
    file.seek(SeekFrom::Start(start))?;
    Ok((BufReader::new(file), start, reset))
}

/// `offset`から改行で終わる行だけを読み、1行ずつ`on_line`に渡す。
///
/// 改行で終わらない最終行は書き込み途中の可能性があるため読まず、オフセットも進めない。
pub fn read_new_lines(
    path: &Path,
    offset: u64,
    mut on_line: impl FnMut(&str),
) -> io::Result<ReadOutcome> {
    let (mut reader, mut pos, reset) = open_at(path, offset)?;
    let mut buf = Vec::new();
    let mut lines = 0;
    loop {
        buf.clear();
        let read = reader.read_until(b'\n', &mut buf)?;
        if buf.last() != Some(&b'\n') {
            break;
        }
        pos += read as u64;
        let text = String::from_utf8_lossy(&buf[..read - 1]);
        let text = text.trim_end_matches('\r');
        if !text.is_empty() {
            on_line(text);
            lines += 1;
        }
    }
    Ok(ReadOutcome {
        new_offset: pos,
        reset,
        lines,
    })
}

/// サイズと更新時刻（ミリ秒）。前回から変化がなければ開かずに済ませるために使う。
pub fn file_signature(path: &Path) -> io::Result<(u64, i64)> {
    let m = std::fs::metadata(path)?;
    let mtime = m
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    Ok((m.len(), mtime))
}

/// 取り込み対象のJSONL1つ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptFile {
    /// パス。
    pub path: PathBuf,
    /// サブエージェントのログならファイル名から取ったID。
    pub agent_id: Option<String>,
}

fn jsonl_in(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        if p.is_file() && p.extension().is_some_and(|x| x == "jsonl") {
            out.push(p);
        }
    }
    Ok(out)
}

fn subagent_files(project: &Path, out: &mut Vec<TranscriptFile>) -> io::Result<()> {
    for session in std::fs::read_dir(project)? {
        let sub = session?.path().join("subagents");
        if !sub.is_dir() {
            continue;
        }
        for p in jsonl_in(&sub)? {
            let id = p
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| s.strip_prefix("agent-"))
                .map(str::to_string);
            out.push(TranscriptFile {
                path: p,
                agent_id: id,
            });
        }
    }
    Ok(())
}

/// 設定ディレクトリ配下の本体ログ（`projects/*/*.jsonl`）とサブエージェントのログ
/// （`projects/*/<sessionId>/subagents/agent-*.jsonl`）を列挙する。
pub fn transcript_files(config_dir: &Path) -> io::Result<Vec<TranscriptFile>> {
    let projects = config_dir.join("projects");
    if !projects.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for project in std::fs::read_dir(&projects)? {
        let project = project?.path();
        if !project.is_dir() {
            continue;
        }
        out.extend(jsonl_in(&project)?.into_iter().map(|p| TranscriptFile {
            path: p,
            agent_id: None,
        }));
        subagent_files(&project, &mut out)?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn collect(path: &Path, offset: u64) -> (Vec<String>, ReadOutcome) {
        let mut v = Vec::new();
        let o = read_new_lines(path, offset, |l| v.push(l.to_string())).unwrap();
        (v, o)
    }

    #[test]
    fn reads_complete_lines_and_advances_offset() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.jsonl");
        std::fs::write(&p, "a\nbb\n").unwrap();
        let (lines, o) = collect(&p, 0);
        assert_eq!(lines, ["a", "bb"]);
        assert_eq!((o.new_offset, o.reset, o.lines), (5, false, 2));
        let (lines, o) = collect(&p, 5);
        assert!(lines.is_empty());
        assert_eq!((o.new_offset, o.reset), (5, false));
    }

    #[test]
    fn reading_from_middle_offset_returns_rest() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.jsonl");
        std::fs::write(&p, "a\nbb\n").unwrap();
        let (lines, o) = collect(&p, 2);
        assert_eq!(lines, ["bb"]);
        assert_eq!(o.new_offset, 5);
    }

    #[test]
    fn incomplete_last_line_is_left_for_next_read() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.jsonl");
        std::fs::write(&p, "a\n{\"half").unwrap();
        let (lines, o) = collect(&p, 0);
        assert_eq!(lines, ["a"]);
        assert_eq!(o.new_offset, 2);
        std::fs::OpenOptions::new()
            .append(true)
            .open(&p)
            .unwrap()
            .write_all(b"\":1}\n")
            .unwrap();
        let (lines, _) = collect(&p, o.new_offset);
        assert_eq!(lines, ["{\"half\":1}"]);
    }

    #[test]
    fn shrunk_file_is_read_from_start() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.jsonl");
        std::fs::write(&p, "x\n").unwrap();
        let (lines, o) = collect(&p, 100);
        assert_eq!(lines, ["x"]);
        assert!(o.reset);
        assert_eq!(o.new_offset, 2);
    }

    #[test]
    fn crlf_and_blank_lines_are_normalized() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.jsonl");
        std::fs::write(&p, "a\r\n\n b\n").unwrap();
        let (lines, o) = collect(&p, 0);
        assert_eq!(lines, ["a", " b"]);
        assert_eq!((o.lines, o.new_offset), (2, 7));
    }

    #[test]
    fn missing_file_is_error() {
        assert!(read_new_lines(Path::new("/nonexistent/x.jsonl"), 0, |_| {}).is_err());
    }

    #[test]
    fn signature_reports_size_and_mtime() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.jsonl");
        std::fs::write(&p, "abc").unwrap();
        let (size, mtime) = file_signature(&p).unwrap();
        assert_eq!(size, 3);
        assert!(mtime > 1_700_000_000_000);
    }

    #[test]
    fn transcript_files_finds_main_and_subagent_logs() {
        let d = tempfile::tempdir().unwrap();
        let proj = d.path().join("projects/-w");
        std::fs::create_dir_all(proj.join("s1/subagents")).unwrap();
        std::fs::write(proj.join("s1.jsonl"), "").unwrap();
        std::fs::write(proj.join("s1/subagents/agent-abc.jsonl"), "").unwrap();
        std::fs::write(proj.join("s1/subagents/agent-abc.meta.json"), "{}").unwrap();
        std::fs::write(proj.join("notes.txt"), "").unwrap();
        std::fs::write(d.path().join("projects/stray.jsonl"), "").unwrap();
        std::fs::create_dir_all(proj.join("s2")).unwrap();
        let mut files = transcript_files(d.path()).unwrap();
        files.sort_by(|a, b| a.path.cmp(&b.path));
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].agent_id.as_deref(), Some("abc"));
        assert_eq!(files[1].agent_id, None);
        assert!(files[1].path.ends_with("s1.jsonl"));
    }

    #[test]
    fn transcript_files_of_missing_dir_is_empty() {
        let d = tempfile::tempdir().unwrap();
        assert!(transcript_files(d.path()).unwrap().is_empty());
    }
}
