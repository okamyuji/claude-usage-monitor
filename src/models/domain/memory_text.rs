//! メモリの表示と、終了の要求まわりの文言。OSには触らない。
use crate::models::domain::memory::ExitError;
use crate::models::domain::session_id::is_safe_id;
use std::path::Path;

/// Desktopを終了させるAppleScript。アプリ名は同名の別アプリに解決されることがあるため、bundle idで宛先を決める。
pub const QUIT_DESKTOP: &str = "quit app id \"com.anthropic.claudefordesktop\"";

/// バイト数を`412MB`や`1.2GB`の形にする。
pub fn fmt_bytes(b: u64) -> String {
    const MIB: u64 = 1 << 20;
    const GIB: u64 = 1 << 30;
    if b >= GIB {
        format!("{:.1}GB", b as f64 / GIB as f64)
    } else {
        format!("{}MB", b / MIB)
    }
}

/// `osascript`の終了コードと標準エラー出力から結果を作る。シグナルで終わったときは`code`が`None`になる。
pub fn quit_result(code: Option<i32>, stderr: &str) -> Result<(), ExitError> {
    if code == Some(0) {
        return Ok(());
    }
    let why = match (stderr.trim(), code) {
        ("", Some(c)) => format!("osascriptが終了コード{c}で終わりました"),
        ("", None) => "osascriptがシグナルで終わりました".to_string(),
        (s, _) => s.to_string(),
    };
    Err(ExitError::Failed(why))
}

/// 端末のセッションを再開するコマンド。POSIXのshell用なのでWindowsでは出さない。
/// IDはファイルから来るため、コマンドに埋め込めない値や、オプションと読まれる`-`始まりの値では出さない。
pub fn resume_command(
    session_id: &str,
    config_dir: Option<&Path>,
    windows: bool,
) -> Option<String> {
    if windows || !is_safe_id(session_id) || session_id.starts_with('-') {
        return None;
    }
    let base = format!("claude --resume {session_id}");
    Some(match config_dir {
        None => base,
        Some(d) => {
            let q = d.to_string_lossy().replace('\'', "'\\''");
            format!("CLAUDE_CONFIG_DIR='{q}' {base}")
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIB: u64 = 1 << 20;

    #[test]
    fn shows_mb_below_one_gib_and_gb_from_one_gib() {
        assert_eq!(fmt_bytes(0), "0MB");
        assert_eq!(fmt_bytes(412 * MIB + 1), "412MB");
        assert_eq!(fmt_bytes(1023 * MIB), "1023MB");
        assert_eq!(fmt_bytes(1024 * MIB), "1.0GB");
        assert_eq!(fmt_bytes(1229 * MIB), "1.2GB");
    }

    #[test]
    fn reads_osascript_result() {
        assert_eq!(quit_result(Some(0), ""), Ok(()));
        assert_eq!(quit_result(Some(0), "warn"), Ok(()));
        assert_eq!(
            quit_result(Some(1), " 拒否されました\n"),
            Err(ExitError::Failed("拒否されました".into()))
        );
        assert_eq!(
            quit_result(Some(1), ""),
            Err(ExitError::Failed(
                "osascriptが終了コード1で終わりました".into()
            ))
        );
        assert_eq!(
            quit_result(None, ""),
            Err(ExitError::Failed(
                "osascriptがシグナルで終わりました".into()
            ))
        );
    }

    #[test]
    fn builds_resume_command() {
        let id = "2c939a2e-7340";
        assert_eq!(
            resume_command(id, None, false).as_deref(),
            Some("claude --resume 2c939a2e-7340")
        );
        assert_eq!(
            resume_command(id, Some(Path::new("/h/My Claude/it's")), false).as_deref(),
            Some("CLAUDE_CONFIG_DIR='/h/My Claude/it'\\''s' claude --resume 2c939a2e-7340")
        );
        assert_eq!(resume_command("a;rm", None, false), None);
        assert_eq!(resume_command("-rf", None, false), None);
        assert_eq!(resume_command(id, None, true), None);
    }
}
