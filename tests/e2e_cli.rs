//! `cps`バイナリのE2Eテスト。利用者が打つコマンドそのものを検証するため、実バイナリを起動する。
#![forbid(unsafe_code)]
use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn help_lists_all_subcommands() {
    Command::cargo_bin("cps")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("daemon"))
        .stdout(contains("run"))
        .stdout(contains("profile"));
}

mod common;
use common::cps;
#[cfg(unix)]
use common::fake_claude;

#[cfg(unix)]
#[test]
fn profile_switch_then_run_passes_config_dir_to_claude() {
    let data = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    fake_claude(bin.path());

    cps(data.path(), home.path(), None)
        .args(["profile", "add", "sub"])
        .assert()
        .success();
    cps(data.path(), home.path(), None)
        .args(["profile", "use", "sub"])
        .assert()
        .success();
    let expected = format!(
        "CONFIG={} ARGS=-p hello",
        home.path().join(".claude-sub").display()
    );
    cps(data.path(), home.path(), Some(bin.path()))
        .args(["run", "-p", "hello"])
        .assert()
        .success()
        .stdout(contains(expected));

    cps(data.path(), home.path(), None)
        .args(["profile", "use", "default"])
        .assert()
        .success();
    cps(data.path(), home.path(), Some(bin.path()))
        .args(["run"])
        .assert()
        .success()
        .stdout(contains("CONFIG=none"));
}

#[test]
fn run_without_claude_on_path_exits_127() {
    let data = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let empty = tempfile::tempdir().unwrap();
    Command::cargo_bin("cps")
        .unwrap()
        .env("CPS_DATA_DIR", data.path())
        .env("HOME", home.path())
        .env("PATH", empty.path())
        .arg("run")
        .assert()
        .code(127)
        .stderr(contains("claude"));
}

#[test]
fn unknown_profile_use_fails_with_message() {
    let data = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    cps(data.path(), home.path(), None)
        .args(["profile", "use", "none"])
        .assert()
        .failure()
        .stderr(contains("none"));
}

#[test]
fn unusable_data_dir_fails_with_message() {
    let home = tempfile::tempdir().unwrap();
    let file = home.path().join("not-a-dir");
    std::fs::write(&file, "").unwrap();
    cps(&file, home.path(), None)
        .args(["profile", "list"])
        .assert()
        .code(1)
        .stderr(contains("DBを開けません"));
}

#[cfg(target_os = "macos")]
#[test]
fn default_data_dir_is_under_application_support() {
    let home = tempfile::tempdir().unwrap();
    Command::cargo_bin("cps")
        .unwrap()
        .env("HOME", home.path())
        .env_remove("CPS_DATA_DIR")
        .args(["profile", "list"])
        .assert()
        .success()
        .stdout(contains("* default"));
    assert!(
        home.path()
            .join("Library/Application Support/work.okamyuji.cps/cps.db")
            .exists()
    );
}
