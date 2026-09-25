//! `cps`バイナリのE2Eテスト。利用者が打つコマンドそのものを検証するため、実バイナリを起動する。
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
