mod common;

use common::splice_cmd;

#[test]
fn bad_flag_with_json_is_one_json_object_exit_2() {
    let out = splice_cmd()
        .args(["new", "--json", "--bogus"])
        .assert()
        .code(2)
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["ok"], false);
    assert_eq!(v["command"], "new");
}
