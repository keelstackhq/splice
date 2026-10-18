mod common;

use splice::lock::ChannelLock;

#[test]
fn channel_rig_fixture_loads() {
    let repo = tempfile::tempdir().unwrap();
    let dir = repo.path().join(".splice/channels");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/lock/channel-rig.toml"
        ),
        dir.join("rig.toml"),
    )
    .unwrap();
    let lock = ChannelLock::read(repo.path(), "rig").unwrap().unwrap();
    assert_eq!(lock.meta.name, "rig");
    assert_eq!(lock.lock.files.len(), 1);
    assert_eq!(lock.sections.len(), 1);
    assert!(!lock.sections["AGENTS.md"].created);
}
