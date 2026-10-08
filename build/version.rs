//! `git describe`の出力から版の文字列を作る。build.rsとライブラリのテストの両方から読み込む。

/// `v0.3.2`は`0.3.2`、`v0.3.2-3-gabc1234`は`0.3.2+3.gabc1234`にする。
/// CIのタグ付けと同じく`vX.Y.Z`の形のタグだけを認め、それ以外は`None`を返す。
pub fn semver_from_describe(describe: &str) -> Option<String> {
    let (tag, meta) = match describe.rsplitn(3, '-').collect::<Vec<_>>()[..] {
        [hash, n, tag] if is_describe_suffix(n, hash) => (tag, Some(format!("{n}.{hash}"))),
        _ => (describe, None),
    };
    let ver = tag.strip_prefix('v')?;
    let parts: Vec<&str> = ver.split('.').collect();
    if parts.len() != 3 || !parts.iter().all(|p| is_semver_number(p)) {
        return None;
    }
    Some(match meta {
        Some(m) => format!("{ver}+{m}"),
        None => ver.to_string(),
    })
}

fn is_describe_suffix(n: &str, hash: &str) -> bool {
    !n.is_empty()
        && n.bytes().all(|b| b.is_ascii_digit())
        && hash
            .strip_prefix('g')
            .is_some_and(|h| !h.is_empty() && h.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// semverの数値部は、先頭に0を付けない。
fn is_semver_number(p: &str) -> bool {
    !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) && (p == "0" || !p.starts_with('0'))
}

#[cfg(test)]
mod tests {
    use super::semver_from_describe;

    #[test]
    fn tagged_commit_is_the_bare_version() {
        assert_eq!(semver_from_describe("v0.3.2").as_deref(), Some("0.3.2"));
        assert_eq!(semver_from_describe("v10.0.20").as_deref(), Some("10.0.20"));
    }

    #[test]
    fn commits_after_a_tag_carry_build_metadata() {
        assert_eq!(
            semver_from_describe("v0.3.2-3-gabc1234").as_deref(),
            Some("0.3.2+3.gabc1234")
        );
    }

    #[test]
    fn tags_outside_strict_semver_are_rejected() {
        for bad in [
            "",
            "0.3.2",
            "v0.3",
            "v0.3.2.1",
            "v01.3.2",
            "v0.3.x",
            "v0..2",
            "v1.2.3-rc1",
            "v1.2.3-rc1-3-gabc1234",
            "v0.3.2-3-gxyz",
            "v0.3.2-x-gabc1234",
            "v0.3.2-3-g",
        ] {
            assert_eq!(semver_from_describe(bad), None, "{bad}");
        }
    }
}
