//! 外部のファイルから来るIDの検査。

/// パスやコマンドに埋め込んでよいIDか。IDはJSONLや状態ファイルから来るため、`../`や記号で外の場所やコマンドを指させない。
pub fn is_safe_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_ascii_alphanumerics_hyphen_and_underscore() {
        assert!(is_safe_id("2c939a2e-7340-4363-b743-1867ce6efab2"));
        assert!(is_safe_id("a_b"));
        assert!(!is_safe_id(""));
        assert!(!is_safe_id("../x"));
        assert!(!is_safe_id("a b"));
        assert!(!is_safe_id("a;b"));
        assert!(!is_safe_id("ａ"));
    }
}
