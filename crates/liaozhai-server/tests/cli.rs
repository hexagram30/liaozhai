//! CLI integration tests for account management subcommands.

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn server_cmd(dir: &TempDir) -> Command {
    let config_path = dir.path().join("test.toml");
    let db_path = dir.path().join("accounts.db");
    std::fs::write(
        &config_path,
        format!("[auth]\ndb_path = \"{}\"\n", db_path.display()),
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("liaozhai-server").unwrap();
    cmd.arg("--config").arg(&config_path);
    cmd
}

#[test]
fn account_create_via_stdin() {
    let dir = TempDir::new().unwrap();
    server_cmd(&dir)
        .args(["account", "create", "alice"])
        .write_stdin("secret\nsecret\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("Account 'alice' created."));
}

#[test]
fn account_create_password_mismatch() {
    let dir = TempDir::new().unwrap();
    server_cmd(&dir)
        .args(["account", "create", "alice"])
        .write_stdin("secret\ndifferent\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Passwords do not match"));
}

#[test]
fn account_create_duplicate() {
    let dir = TempDir::new().unwrap();
    // First create succeeds
    server_cmd(&dir)
        .args(["account", "create", "alice"])
        .write_stdin("secret\nsecret\n")
        .assert()
        .success();

    // Second create fails
    server_cmd(&dir)
        .args(["account", "create", "alice"])
        .write_stdin("other\nother\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}

#[test]
fn account_list_empty() {
    let dir = TempDir::new().unwrap();
    server_cmd(&dir)
        .args(["account", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("(no accounts)"));
}

#[test]
fn account_list_after_create() {
    let dir = TempDir::new().unwrap();
    server_cmd(&dir)
        .args(["account", "create", "alice"])
        .write_stdin("secret\nsecret\n")
        .assert()
        .success();

    server_cmd(&dir)
        .args(["account", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("alice"))
        .stdout(predicate::str::contains("(never)"));
}
