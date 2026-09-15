//! Synthetic processing proposals and deterministic append-response faults.

use std::{convert::Infallible, error::Error, io, num::NonZeroUsize};

use fleetiq_application::{
    AcceptedMessageAppendOutcome, AcceptedMessageLog, DecodedObservationAppendOutcome,
    DecodedObservationBatch, DecodedObservationLog, DurablyPersistedFrame, FrameAcceptanceContext,
    PendingAcceptedMessage, PreparedFrameAcceptance,
};
use fleetiq_domain::{AssetId, TenantId, UnixTimeMillis};
use fleetiq_postgres::{PostgresAcceptedMessageLog, PostgresDecodedObservationLog};
use fleetiq_protocol_host::{
    FramePoll, FrameProcessingGate, FrameProcessingLimits, SessionInputBuffer, SessionState,
};
use fleetiq_protocol_sdk::{
    DecodedMeasurement, DecodedOutput, DecodedValue, DeviceEndpointId, FrameBoundary,
    FrameProcessingOutput, ObservationQuality, ProposedReply, ReplyRelease, SessionStateAction,
    SourceSignalId,
};
use fleetiq_telemetry::{
    AcceptedMessageId, ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion,
};
use sqlx::PgPool;

#[derive(Clone, Copy)]
pub(super) enum Fault {
    BeforeAppend,
    AfterAppend,
}

pub(super) struct FaultLog {
    pool: PgPool,
    fault: Fault,
}

impl FaultLog {
    pub(super) const fn new(pool: PgPool, fault: Fault) -> Self {
        Self { pool, fault }
    }
}

impl AcceptedMessageLog for FaultLog {
    type Error = io::Error;

    async fn append(
        &self,
        candidate: &PendingAcceptedMessage,
    ) -> Result<AcceptedMessageAppendOutcome, Self::Error> {
        if matches!(self.fault, Fault::AfterAppend) {
            // The real adapter returns only after commit. Discard that response
            // rather than weakening or bypassing its durability implementation.
            PostgresAcceptedMessageLog::new(self.pool.clone())
                .append(candidate)
                .await
                .map_err(io::Error::other)?;
        }
        Err(io::Error::new(
            io::ErrorKind::ConnectionReset,
            "injected append response failure",
        ))
    }
}

impl DecodedObservationLog for FaultLog {
    type Error = io::Error;

    async fn append(
        &self,
        batch: &DecodedObservationBatch,
    ) -> Result<DecodedObservationAppendOutcome, Self::Error> {
        if matches!(self.fault, Fault::AfterAppend) {
            PostgresDecodedObservationLog::new(self.pool.clone())
                .append(batch)
                .await
                .map_err(io::Error::other)?;
        }
        Err(io::Error::new(
            io::ErrorKind::ConnectionReset,
            "injected append response failure",
        ))
    }
}

pub(super) async fn seed_device(pool: &PgPool) -> Result<(), sqlx::Error> {
    // Only admission metadata is seeded directly. Every ingestion fact in these
    // tests must cross the public application transitions and real log adapters.
    sqlx::raw_sql(
        r"
        INSERT INTO metadata.tenants (tenant_id) VALUES ('tenant-a');
        INSERT INTO metadata.asset_type_definitions
            (tenant_id, type_id, version, display_name, property_count)
        VALUES ('tenant-a', 'gateway', 1, 'Gateway', 0);
        INSERT INTO metadata.assets
            (tenant_id, asset_id, type_id, type_version, display_name)
        VALUES ('tenant-a', 'device-a', 'gateway', 1, 'Gateway A');
        INSERT INTO metadata.communication_devices (tenant_id, device_asset_id)
        VALUES ('tenant-a', 'device-a');
    ",
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub(super) fn prepared_frame(output_count: u8) -> Result<PreparedFrameAcceptance, Box<dyn Error>> {
    let limit = NonZeroUsize::new(64).unwrap();
    let mut input = SessionInputBuffer::new(limit);
    input.push(b"fixture-frame")?;
    let FramePoll::Frame(frame) = input.poll_frame(|bytes| {
        Ok::<_, Infallible>(FrameBoundary::Frame {
            length: NonZeroUsize::new(bytes.len()).unwrap(),
        })
    })?
    else {
        unreachable!("fixture supplies one complete frame");
    };
    let outputs = (0..output_count)
        .map(|index| {
            Ok(DecodedOutput::new(
                DeviceEndpointId::new(format!("channel-{index}"))?,
                SourceSignalId::new("reading")?,
                DecodedMeasurement::new(
                    DecodedValue::Integer(i64::from(index) + 100),
                    None,
                    ObservationQuality::Good,
                )?,
                Some(UnixTimeMillis::new(900)),
            ))
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    let proposal = FrameProcessingOutput::new(
        outputs,
        SessionStateAction::Keep,
        vec![
            ProposedReply::new(b"ready".as_slice(), ReplyRelease::AfterProcessing)?,
            ProposedReply::new(b"accepted".as_slice(), ReplyRelease::AfterDurableAcceptance)?,
        ],
    );
    let output = FrameProcessingGate::new(FrameProcessingLimits::new(limit, limit, limit, limit))
        .apply(&mut SessionState::new(limit), proposal)?;
    Ok(PreparedFrameAcceptance::new(
        FrameAcceptanceContext::new(
            AcceptedMessageId::new("message-1")?,
            TenantId::new("tenant-a")?,
            AssetId::new("device-a")?,
            ProtocolPackageRef::new(
                ProtocolPackageId::new("synthetic.ingestion")?,
                ProtocolPackageVersion::new("1")?,
            ),
            UnixTimeMillis::new(1_000),
        ),
        frame,
        output,
    ))
}

pub(super) async fn counts(pool: &PgPool) -> Result<(i64, i64, i64), sqlx::Error> {
    // One snapshot makes the three durability-stage counts directly comparable.
    sqlx::query_as(
        r"SELECT
        (SELECT count(*) FROM ingestion.accepted_messages),
        (SELECT count(*) FROM ingestion.decoded_message_results),
        (SELECT count(*) FROM ingestion.decoded_observations)",
    )
    .fetch_one(pool)
    .await
}

pub(super) fn assert_observations(frame: &DurablyPersistedFrame, count: u8) {
    assert_eq!(frame.message().payload().as_bytes(), b"fixture-frame");
    assert_eq!(frame.observations().len(), usize::from(count));
    for (index, observation) in (0..count).zip(frame.observations()) {
        assert_eq!(
            observation.id(),
            &frame.message().decoded_observation_id(u32::from(index))
        );
        assert_eq!(observation.tenant_id(), frame.message().tenant_id());
        assert_eq!(
            observation.source().device_asset_id(),
            frame.message().device_asset_id()
        );
        assert_eq!(
            observation.source().endpoint_id().as_str(),
            format!("channel-{index}")
        );
        assert_eq!(
            observation.value(),
            &DecodedValue::Integer(i64::from(index) + 100)
        );
        assert_eq!(
            observation.device_recorded_at(),
            Some(UnixTimeMillis::new(900))
        );
        assert_eq!(observation.received_at(), UnixTimeMillis::new(1_000));
    }
}

pub(super) async fn assert_stored_observations(pool: &PgPool) -> Result<(), sqlx::Error> {
    let rows: Vec<(i64, String, String, i64)> = sqlx::query_as(
        r"
        SELECT output_index, endpoint_id, signal_id, value_integer
        FROM ingestion.decoded_observations ORDER BY output_index
    ",
    )
    .fetch_all(pool)
    .await?;
    assert_eq!(
        rows,
        vec![
            (0, "channel-0".to_owned(), "reading".to_owned(), 100),
            (1, "channel-1".to_owned(), "reading".to_owned(), 101),
        ]
    );
    Ok(())
}
