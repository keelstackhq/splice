#![allow(dead_code)]
use assert_cmd::Command;

pub fn splice_cmd() -> Command {
    let mut c = Command::cargo_bin("splice").unwrap();
    c.env("GIT_TERMINAL_PROMPT", "0");
    c
}
