//! Executable input validation for the trusted retention turn.
use std::process::Command;

#[test]
fn reclaim_once_rejects_invalid_inputs_before_connecting() {
    for (key, value) in [
        ("FLEETIQ_RECLAIM_TENANT_ID", None),
        ("FLEETIQ_RECLAIM_TENANT_ID", Some("")),
        ("FLEETIQ_RECLAIM_MANIFEST_ID", Some("0")),
        (
            "FLEETIQ_RECLAIM_POLICY_REVISION",
            Some("9223372036854775808"),
        ),
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fleetiq-server"));
        command
            .arg("reclaim-once")
            .env_clear()
            .env("FLEETIQ_RECLAIM_TENANT_ID", "tenant-a")
            .env("FLEETIQ_RECLAIM_MANIFEST_ID", "1")
            .env("FLEETIQ_RECLAIM_POLICY_REVISION", "1");
        match value {
            Some(value) => command.env(key, value),
            None => command.env_remove(key),
        };
        let result = command.output().unwrap();
        assert!(!result.status.success());
        assert_eq!(result.stdout.len(), 0);
        assert_eq!(
            String::from_utf8(result.stderr).unwrap().trim(),
            "invalid reclaim target"
        );
    }
}

#[test]
fn reclaim_once_requires_storage_before_connecting() {
    let result = Command::new(env!("CARGO_BIN_EXE_fleetiq-server"))
        .arg("reclaim-once")
        .env_clear()
        .env("FLEETIQ_RECLAIM_TENANT_ID", "tenant-a")
        .env("FLEETIQ_RECLAIM_MANIFEST_ID", "1")
        .env("FLEETIQ_RECLAIM_POLICY_REVISION", "1")
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert_eq!(result.stdout.len(), 0);
    assert_eq!(
        String::from_utf8(result.stderr).unwrap().trim(),
        "reclaim storage configuration required"
    );
}
