//! OSの日本語フォントの読み込み。
//!
//! eguiの既定フォントには日本語の字形がない。フォントを同梱すると配布物とメモリが増えるため、OSのフォントをGUIの起動時に読む。

use std::path::PathBuf;

/// 日本語フォントの候補。先に見つかったものを使う。
pub const CJK_FONT_CANDIDATES: &[&str] = &[
    "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "C:\\Windows\\Fonts\\YuGothM.ttc",
    "C:\\Windows\\Fonts\\meiryo.ttc",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
];

/// 候補のうち最初に存在するファイル。
pub fn first_existing(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates.iter().find(|p| p.is_file()).cloned()
}

/// 日本語フォントを読む。見つからなければ`None`を返し、GUIは既定フォントで起動する。
pub fn load_cjk_font() -> Option<(PathBuf, Vec<u8>)> {
    let cands: Vec<PathBuf> = CJK_FONT_CANDIDATES.iter().map(PathBuf::from).collect();
    let path = first_existing(&cands)?;
    let bytes = std::fs::read(&path).ok()?;
    Some((path, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_existing_font_is_chosen() {
        let d = tempfile::tempdir().unwrap();
        let a = d.path().join("a.ttc");
        let b = d.path().join("b.ttc");
        std::fs::write(&b, b"font").unwrap();
        assert_eq!(first_existing(&[a.clone(), b.clone()]), Some(b.clone()));
        std::fs::write(&a, b"font").unwrap();
        assert_eq!(first_existing(&[a.clone(), b]), Some(a));
    }

    #[test]
    fn no_font_found_returns_none() {
        assert_eq!(first_existing(&[PathBuf::from("/nonexistent/x.ttc")]), None);
        assert_eq!(first_existing(&[]), None);
    }

    #[test]
    fn candidates_cover_three_os() {
        assert!(
            CJK_FONT_CANDIDATES
                .iter()
                .any(|p| p.starts_with("/System/Library/Fonts"))
        );
        assert!(
            CJK_FONT_CANDIDATES
                .iter()
                .any(|p| p.starts_with("C:\\Windows\\Fonts"))
        );
        assert!(
            CJK_FONT_CANDIDATES
                .iter()
                .any(|p| p.starts_with("/usr/share/fonts"))
        );
    }
}
