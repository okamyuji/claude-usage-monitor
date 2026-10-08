//! macOSの「About cumon」は、バイナリに埋め込んだInfo.plistから版を読む。
#![cfg(target_os = "macos")]

#[test]
fn binary_embeds_info_plist_with_the_build_version() {
    let bin = std::fs::read(env!("CARGO_BIN_EXE_cumon")).unwrap();
    let ver = env!("CUMON_VERSION");
    let expected = format!("<key>CFBundleShortVersionString</key><string>{ver}</string>");
    assert!(
        bin.windows(expected.len())
            .any(|w| w == expected.as_bytes()),
        "Info.plistに{ver}が見つかりません"
    );
}
