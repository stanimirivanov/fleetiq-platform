//! Conformance of the real source-built Teltonika component through host gates.
use fleetiq_domain::{AssetId, TenantId, UnixTimeMillis};
use fleetiq_protocol_host::{
    FrameProcessingGate, FrameProcessingLimits, SessionInputBuffer, SessionState, StagedReply,
};
use fleetiq_protocol_sdk::FramingInput;
use fleetiq_protocol_wasm::{
    AdmittedIdentityProtocolComponent, ComponentAdmissionPolicy, InvocationBudget,
    ProcessedProtocolSessionTurn, ProtocolSessionTurn, ProtocolSessionTurnBudgets,
    ProtocolSessionTurnError, WasmSandbox,
};
use fleetiq_telemetry::{
    AcceptedMessage, AcceptedMessageId, AcceptedPayload, ObservationQuality, ProtocolPackageId,
    ProtocolPackageRef, ProtocolPackageVersion,
};
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
        NonZeroU64::new(50_000_000).unwrap(),
        Duration::from_secs(5),
        nz(16 << 20),
        nz(1000),
    )
    .unwrap()
}
fn build() -> Vec<u8> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target = root.join("target/protocol-guest");
    for operation in ["clippy", "build"] {
        let mut command = Command::new(env!("CARGO"));
        command
            .current_dir(&root)
            .args([
                operation,
                "--locked",
                "--release",
                "--target",
                "wasm32-unknown-unknown",
                "-p",
                "fleetiq-teltonika",
                "--target-dir",
            ])
            .arg(&target);
        if operation == "clippy" {
            command.args(["--", "-D", "warnings"]);
        }
        let output = command.output().expect("pinned guest toolchain must run");
        assert!(
            output.status.success(),
            "guest {operation} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let module =
        fs::read(target.join("wasm32-unknown-unknown/release/fleetiq_teltonika.wasm")).unwrap();
    ComponentEncoder::default()
        .module(&module)
        .unwrap()
        .validate(true)
        .encode()
        .unwrap()
}
fn fixture(text: &str) -> Vec<u8> {
    text.trim()
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
        .collect()
}
fn turn(
    s: &WasmSandbox,
    c: &AdmittedIdentityProtocolComponent,
    state: &mut SessionState,
    input: &mut SessionInputBuffer,
    output_limit: usize,
) -> Result<ProtocolSessionTurn, ProtocolSessionTurnError> {
    s.run_component_session_turn(
        c.protocol(),
        ProtocolSessionTurnBudgets::new(budget(), budget()),
        &FrameProcessingGate::new(FrameProcessingLimits::new(
            nz(output_limit),
            nz(1),
            nz(4),
            nz(4),
        )),
        state,
        input,
    )
}
fn check_identity(s: &WasmSandbox, c: &AdmittedIdentityProtocolComponent) {
    let hello = b"\x00\x0f123456789012345";
    let (claim, _) = s
        .identify_component_device(c, budget(), FramingInput::new(hello, &[]), nz(17))
        .unwrap();
    assert_eq!(claim.kind(), "imei");
    assert_eq!(claim.value(), "123456789012345");
    assert!(!format!("{claim:?}").contains("123456789012345"));
    for (bytes, state) in [(&hello[..16], &[][..]), (&hello[..], &[1][..])] {
        assert!(
            s.identify_component_device(c, budget(), FramingInput::new(bytes, state), nz(32))
                .is_err()
        );
    }
}
fn handshake(
    s: &WasmSandbox,
    c: &AdmittedIdentityProtocolComponent,
) -> (SessionState, SessionInputBuffer, Vec<u8>) {
    let hello = b"\x00\x0f123456789012345";
    let mut state = SessionState::new(nz(1));
    let mut input = SessionInputBuffer::new(nz(2560));
    input.push(&hello[..8]).unwrap();
    assert!(matches!(
        turn(s, c, &mut state, &mut input, 1200).unwrap(),
        ProtocolSessionTurn::NeedMoreData { .. }
    ));
    assert_eq!(state.as_bytes().len(), 0);
    let avl = fixture(include_str!(
        "../../../packages/teltonika/tests/fixtures/codec8.hex"
    ));
    input.push(&hello[8..]).unwrap();
    input.push(&avl).unwrap();
    let ProtocolSessionTurn::Processed(hello) = turn(s, c, &mut state, &mut input, 1200).unwrap()
    else {
        panic!("handshake")
    };
    assert_eq!(state.as_bytes(), [1]);
    assert_eq!(hello.output().decoded_outputs().len(), 0);
    let StagedReply::Ready(ready) = &hello.output().staged_replies()[0] else {
        panic!("immediate handshake ACK")
    };
    assert_eq!(ready.as_bytes(), [1]);
    assert_eq!(input.as_bytes(), avl);
    (state, input, avl)
}
fn check_outputs(
    decoded: &ProcessedProtocolSessionTurn,
    package: &ProtocolPackageRef,
    frame: &[u8],
) -> AcceptedMessage {
    assert_eq!(decoded.package(), package);
    assert_eq!(decoded.frame().as_bytes(), frame);
    let values = decoded.output().decoded_outputs();
    assert!(values.len() >= 26);
    assert_eq!(
        values[0].device_recorded_at().unwrap().get(),
        1_700_000_000_123
    );
    assert_eq!(values[3].signal_id().as_str(), "gps.longitude_e7");
    assert_eq!(
        values[3].value(),
        &fleetiq_protocol_sdk::DecodedValue::Integer(-123_456_789)
    );
    let unknown = values
        .iter()
        .find(|v| v.signal_id().as_str() == "io.253.raw")
        .unwrap();
    assert_eq!(
        unknown.value(),
        &fleetiq_protocol_sdk::DecodedValue::Text("ffffffffffffffff".into())
    );
    let uncertain = values
        .iter()
        .rev()
        .find(|v| v.signal_id().as_str() == "gps.longitude_e7")
        .unwrap();
    assert_eq!(uncertain.quality(), ObservationQuality::Uncertain);
    assert_eq!(values[3].quality(), ObservationQuality::Good);
    // An in-memory witness exercises the existing release gate, not a DB commit.
    let accepted = AcceptedMessage::new(
        AcceptedMessageId::new("teltonika-message").unwrap(),
        TenantId::new("tenant-a").unwrap(),
        AssetId::new("device-1").unwrap(),
        package.clone(),
        UnixTimeMillis::new(1_700_000_001_500),
        AcceptedPayload::new(frame),
    );
    let StagedReply::AwaitingDurableAcceptance(deferred) =
        decoded.output().staged_replies()[0].clone()
    else {
        panic!("deferred AVL ACK")
    };
    let ready = deferred.release(&accepted);
    assert_eq!(ready.as_bytes(), [0, 0, 0, 2]);
    assert_eq!(ready.durable_acceptance(), Some(accepted.id()));
    accepted
}
fn check_packet(
    s: &WasmSandbox,
    c: &AdmittedIdentityProtocolComponent,
    package: &ProtocolPackageRef,
    state: &mut SessionState,
    input: &mut SessionInputBuffer,
    frame: Vec<u8>,
) {
    if input.as_bytes().is_empty() {
        input.push(&frame).unwrap();
    }
    let ProtocolSessionTurn::Processed(decoded) = turn(s, c, state, input, 1200).unwrap() else {
        panic!("AVL")
    };
    assert_eq!(input.as_bytes().len(), 0);
    assert_eq!(state.as_bytes(), [1]);
    let accepted = check_outputs(&decoded, package, &frame);
    // Fresh instances with explicit state must replay the same outputs.
    input.push(&frame).unwrap();
    let ProtocolSessionTurn::Processed(repeated) = turn(s, c, state, input, 1200).unwrap() else {
        panic!("replay")
    };
    assert_eq!(decoded.output(), repeated.output());
    let mut bad = frame;
    let last = bad.len() - 1;
    bad[last] ^= 1;
    input.push(&bad).unwrap();
    assert!(turn(s, c, state, input, 1200).is_err());
    assert_eq!(state.as_bytes(), [1]);
    *input = SessionInputBuffer::new(nz(2560));
    input.push(accepted.payload().as_bytes()).unwrap();
    assert!(turn(s, c, state, input, 1).is_err());
    assert_eq!(state.as_bytes(), [1]);
    *input = SessionInputBuffer::new(nz(2560));
}
#[test]
fn built_teltonika_identifies_frames_replays_and_preserves_ack_and_error_gates() {
    let bytes = build();
    let package = ProtocolPackageRef::new(
        ProtocolPackageId::new("teltonika.tcp").unwrap(),
        ProtocolPackageVersion::new("1").unwrap(),
    );
    let sandbox = WasmSandbox::new(Duration::from_millis(10), nz(512 << 10)).unwrap();
    let component = sandbox
        .admit_identity_protocol_component(
            package.clone(),
            &bytes,
            ComponentAdmissionPolicy::new(nz(bytes.len())),
        )
        .unwrap();
    check_identity(&sandbox, &component);
    let (mut state, mut input, avl) = handshake(&sandbox, &component);
    for frame in [
        avl,
        fixture(include_str!(
            "../../../packages/teltonika/tests/fixtures/codec8-extended.hex"
        )),
    ] {
        check_packet(
            &sandbox, &component, &package, &mut state, &mut input, frame,
        );
    }
}
