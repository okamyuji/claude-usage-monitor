//! ビルドごとの版を決め、macOSではInfo.plistとしてバイナリに埋め込む。
//!
//! 版はCIがmainのcommitごとに付ける`vX.Y.Z`タグから`git describe`で得る。
//! タグもgitも無いとき（crateのtarballなど）はCargo.tomlの版を使う。
#[path = "build/version.rs"]
mod version;

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build/version.rs");
    rerun_on_git_changes();
    let ver = git(&["describe", "--tags", "--match", "v[0-9]*"])
        .and_then(|d| version::semver_from_describe(&d))
        .unwrap_or_else(|| std::env::var("CARGO_PKG_VERSION").unwrap());
    println!("cargo:rustc-env=CUMON_VERSION={ver}");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        // .appに包まない実行ファイルでも、__TEXT,__info_plistに置いたInfo.plistを
        // macOSはmain bundleの情報として読み、標準のAboutパネルが版を出す。
        let plist = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("Info.plist");
        std::fs::write(&plist, info_plist(&ver)).unwrap();
        println!(
            "cargo:rustc-link-arg-bins=-Wl,-sectcreate,__TEXT,__info_plist,{}",
            plist.display()
        );
    }
}

/// CFBundleIdentifierは入れない。通知や権限の判定がバンドルIDに紐づき、挙動が変わるため。
fn info_plist(ver: &str) -> String {
    format!(
        concat!(
            r#"<?xml version="1.0" encoding="UTF-8"?>"#,
            r#"<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">"#,
            r#"<plist version="1.0"><dict>"#,
            "<key>CFBundleName</key><string>cumon</string>",
            "<key>CFBundleShortVersionString</key><string>{ver}</string>",
            "</dict></plist>"
        ),
        ver = ver
    )
}

/// commitやタグが変わったときだけ版を作り直す。存在しないパスを渡すとcargoが毎回作り直すため、あるものだけ渡す。
fn rerun_on_git_changes() {
    let (Some(git_dir), Some(common)) = (
        git(&["rev-parse", "--git-dir"]),
        git(&["rev-parse", "--git-common-dir"]),
    ) else {
        return;
    };
    let mut paths = vec![
        Path::new(&git_dir).join("HEAD"),
        Path::new(&common).join("packed-refs"),
        Path::new(&common).join("refs/tags"),
    ];
    if let Some(head_ref) = git(&["symbolic-ref", "-q", "HEAD"]) {
        paths.push(Path::new(&common).join(head_ref));
    }
    for p in paths.iter().filter(|p| p.exists()) {
        println!("cargo:rerun-if-changed={}", p.display());
    }
}

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}
