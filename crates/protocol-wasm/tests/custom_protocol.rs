//! Conformance of an actual source-built Rust guest, not a hand-written ABI fixture.
use fleetiq_protocol_host::{
    FrameProcessingGate, FrameProcessingLimits, SessionInputBuffer, SessionState, StagedReply,
};
use fleetiq_protocol_sdk::FramingInput;
use fleetiq_protocol_wasm::{
    ComponentAdmissionPolicy, IdentityInvocationError, InvocationBudget, ProtocolSessionTurn,
    ProtocolSessionTurnBudgets, WasmSandbox,
};
use fleetiq_telemetry::{ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion};
use std::{
    fs,
    num::{NonZeroU64, NonZeroUsize},
    path::PathBuf,
    process::Command,
    time::Duration,
};
use wit_component::ComponentEncoder;
fn nz(n: usize) -> NonZeroUsize {
    NonZeroUsize::new(n).unwrap()
}
fn budget() -> InvocationBudget {
    InvocationBudget::new(
        NonZeroU64::new(10_000_000).unwrap(),
        Duration::from_secs(5),
        nz(4 << 20),
        nz(1000),
    )
    .unwrap()
}
fn package() -> ProtocolPackageRef {
    ProtocolPackageRef::new(
        ProtocolPackageId::new("example.custom").unwrap(),
        ProtocolPackageVersion::new("1").unwrap(),
    )
}
fn build() -> Vec<u8> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target = root.join("target/protocol-guest");
    let output = Command::new(env!("CARGO"))
        .current_dir(&root)
        .args([
            "build",
            "--locked",
            "--release",
            "--target",
            "wasm32-unknown-unknown",
            "-p",
            "fleetiq-custom-protocol-example",
            "--target-dir",
        ])
        .arg(&target)
        .output()
        .expect("guest build must run; missing prerequisites are failures");
    assert!(
        output.status.success(),
        "guest compilation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let module = fs::read(
        target.join("wasm32-unknown-unknown/release/fleetiq_custom_protocol_example.wasm"),
    )
    .unwrap();
    ComponentEncoder::default()
        .module(&module)
        .unwrap()
        .validate(true)
        .encode()
        .unwrap()
}
fn turn(
    s: &WasmSandbox,
    c: &fleetiq_protocol_wasm::AdmittedProtocolComponent,
    state: &mut SessionState,
    input: &mut SessionInputBuffer,
) -> ProtocolSessionTurn {
    s.run_component_session_turn(
        c,
        ProtocolSessionTurnBudgets::new(budget(), budget()),
        &FrameProcessingGate::new(FrameProcessingLimits::new(nz(8), nz(8), nz(64), nz(256))),
        state,
        input,
    )
    .unwrap()
}
#[test]
fn source_built_guest_conforms_to_framing_state_and_reply_gates() {
    let bytes = build();
    let sandbox = WasmSandbox::new(Duration::from_millis(10), nz(512 << 10)).unwrap();
    let policy = ComponentAdmissionPolicy::new(nz(bytes.len()));
    let component = sandbox
        .admit_identity_protocol_component(package(), &bytes, policy)
        .unwrap();
    let handshake = b"\x04\x01ABC";
    let mut input = SessionInputBuffer::new(nz(128));
    let mut state = SessionState::new(nz(32));
    input.push(&handshake[..2]).unwrap();
    assert!(matches!(
        turn(&sandbox, component.protocol(), &mut state, &mut input),
        ProtocolSessionTurn::NeedMoreData { .. }
    ));
    assert!(state.as_bytes().is_empty());
    assert_eq!(input.as_bytes(), &handshake[..2]);
    // Coalesced handshake + telemetry still consumes exactly one prefix per turn.
    input.push(&handshake[2..]).unwrap();
    input.push(b"\x03\x02\x00\x2a").unwrap();
    let ProtocolSessionTurn::Processed(hello) =
        turn(&sandbox, component.protocol(), &mut state, &mut input)
    else {
        panic!("complete handshake")
    };
    assert_eq!(state.as_bytes(), b"ABC");
    assert!(hello.output().decoded_outputs().is_empty());
    assert!(matches!(
        &hello.output().staged_replies()[0],
        StagedReply::Ready(_)
    ));
    assert_eq!(input.as_bytes(), b"\x03\x02\x00\x2a");
    let ProtocolSessionTurn::Processed(sample) =
        turn(&sandbox, component.protocol(), &mut state, &mut input)
    else {
        panic!("complete telemetry")
    };
    assert_eq!(sample.output().decoded_outputs().len(), 1);
    let decoded = &sample.output().decoded_outputs()[0];
    assert_eq!(decoded.endpoint_id().as_str(), "main");
    assert_eq!(decoded.signal_id().as_str(), "temperature");
    assert_eq!(
        decoded.value(),
        &fleetiq_protocol_sdk::DecodedValue::Integer(42)
    );
    assert!(matches!(
        &sample.output().staged_replies()[0],
        StagedReply::AwaitingDurableAcceptance(_)
    ));
    assert!(input.as_bytes().is_empty());
    assert_eq!(state.as_bytes(), b"ABC");
    // Fresh instances produce identical semantic outputs from the same explicit state.
    let mut replay = SessionInputBuffer::new(nz(128));
    replay.push(b"\x03\x02\x00\x2a").unwrap();
    let ProtocolSessionTurn::Processed(repeated) =
        turn(&sandbox, component.protocol(), &mut state, &mut replay)
    else {
        panic!("replayed telemetry")
    };
    assert_eq!(sample.output(), repeated.output());
    // Invalid processing preserves the host's previous session state.
    replay.push(b"\x03\xff\x00\x00").unwrap();
    assert!(
        sandbox
            .run_component_session_turn(
                component.protocol(),
                ProtocolSessionTurnBudgets::new(budget(), budget()),
                &FrameProcessingGate::new(FrameProcessingLimits::new(
                    nz(8),
                    nz(8),
                    nz(64),
                    nz(256)
                )),
                &mut state,
                &mut replay
            )
            .is_err()
    );
    assert_eq!(state.as_bytes(), b"ABC");
}

#[test]
fn built_guest_identity_and_admission_remain_explicit_and_bounded() {
    let bytes = build();
    let sandbox = WasmSandbox::new(Duration::from_millis(10), nz(512 << 10)).unwrap();
    let policy = ComponentAdmissionPolicy::new(nz(bytes.len()));
    let component = sandbox
        .admit_identity_protocol_component(package(), &bytes, policy)
        .unwrap();
    // The additive world also remains invocable by the original runtime admission.
    let legacy = sandbox
        .admit_protocol_component(package(), &bytes, policy)
        .unwrap();
    assert_eq!(
        legacy.artifact_digest(),
        component.protocol().artifact_digest()
    );
    assert!(
        sandbox
            .admit_protocol_component(
                package(),
                &bytes,
                ComponentAdmissionPolicy::new(nz(bytes.len() - 1))
            )
            .is_err()
    );
    assert!(
        sandbox
            .admit_identity_protocol_component(package(), b"not a component", policy)
            .is_err()
    );
    let handshake = b"\x04\x01ABC";
    let (claim, report) = sandbox
        .identify_component_device(
            &component,
            budget(),
            FramingInput::new(handshake, b""),
            nz(64),
        )
        .unwrap();
    assert_eq!(claim.kind(), "serial");
    assert_eq!(claim.value(), "ABC");
    assert!(report.fuel_consumed() > 0);
    assert!(!format!("{claim:?}").contains("ABC"));
    assert!(matches!(
        sandbox.identify_component_device(
            &component,
            budget(),
            FramingInput::new(handshake, b""),
            nz(4)
        ),
        Err(IdentityInvocationError::InputLimitExceeded)
    ));
    assert!(matches!(
        sandbox.identify_component_device(
            &component,
            budget(),
            FramingInput::new(b"\x04\x01A!C", b""),
            nz(64)
        ),
        Err(IdentityInvocationError::PackageRejected { code: 1 })
    ));
}

/// Uses the packaged worker entry point so CI exercises actual Linux limits and
/// import-free typed admission, rather than just in-process sandbox helpers.
#[cfg(target_os = "linux")]
#[test]
fn isolated_worker_admits_source_built_guest_and_rejects_invalid_content() {
    use std::{io::Write, process::Stdio};
    let bytes = build();
    for (world, input, expected) in [
        ("identity-v1", bytes.as_slice(), 0),
        ("runtime-v1", bytes.as_slice(), 0),
        ("runtime-v1", b"invalid-component".as_slice(), 2),
    ] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_fleetiq-protocol-admission"))
            .arg(world)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("real admission worker");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input)
            .expect("bounded worker input");
        assert_eq!(child.wait().unwrap().code(), Some(expected));
    }
}
#[cfg(not(target_os = "linux"))]
#[test]
fn admission_worker_refuses_unsupported_isolation() {
    let status = Command::new(env!("CARGO_BIN_EXE_fleetiq-protocol-admission"))
        .arg("runtime-v1")
        .env_clear()
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(78));
}
