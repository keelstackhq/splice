//! Shared helpers for the CLI integration tests.

use assert_cmd::Command;

/// The `splice` binary under test.
pub fn splice_cmd() -> Command {
    assert_cmd::cargo::cargo_bin_cmd!("splice")
}
