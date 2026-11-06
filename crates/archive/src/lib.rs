//! Exact tenant-owned segments and immutable object I/O; no retention authority.
mod profile;
mod wire;
use bytes::Bytes;
use fleetiq_domain::TenantId;
use fleetiq_history::StoredPropertyChange;
use parquet::{
    basic::{Compression, Encoding},
    data_type::{ByteArray, ByteArrayType},
    file::{
        metadata::KeyValue,
        properties::{EnabledStatistics, WriterProperties, WriterVersion},
        reader::FileReader,
        serialized_reader::{ReadOptionsBuilder, SerializedFileReader},
        writer::SerializedFileWriter,
    },
    schema::{
        parser::parse_message_type,
        types::{SchemaDescriptor, TypePtr},
    },
};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::sync::Arc;
/// Maximum interpretations per independently bounded segment.
pub const MAX_ROWS: usize = 256;
/// Maximum UTF-8 bytes per exact field; oversized evidence is never truncated.
pub const MAX_FIELD_BYTES: usize = 8192;
/// Maximum aggregate logical field bytes.
pub const MAX_LOGICAL_BYTES: usize = 1_048_576;
/// Maximum accepted or emitted Parquet file bytes.
pub const MAX_FILE_BYTES: usize = 2_097_152;
const MAX_FOOTER: usize = 32_768;
const FORMAT: &str = "property-history-v1";
const HEX: &[u8; 16] = b"0123456789abcdef";
/// Invalid, unsupported or oversized segment; diagnostics contain no evidence.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct ArchiveError;
impl std::fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid or unsupported archive segment")
    }
}
impl std::error::Error for ArchiveError {}
impl From<parquet::errors::ParquetError> for ArchiveError {
    fn from(_: parquet::errors::ParquetError) -> Self {
        Self
    }
}
fn schema() -> Result<TypePtr, ArchiveError> {
    let mut fields = String::new();
    for name in wire::NAMES {
        write!(fields, "required binary {name} (UTF8); ").map_err(|_| ArchiveError)?;
    }
    Ok(Arc::new(parse_message_type(&format!(
        "message fleetiq_history_v1 {{ {fields} }}"
    ))?))
}
fn validate(
    tenant: &TenantId,
    rows: &[StoredPropertyChange],
) -> Result<Vec<wire::Fields>, ArchiveError> {
    if rows.is_empty() || rows.len() > MAX_ROWS {
        return Err(ArchiveError);
    }
    let mut fields = Vec::with_capacity(rows.len());
    let mut total = 0;
    for (index, row) in rows.iter().enumerate() {
        if row.tenant_id() != tenant || rows[..index].iter().any(|r| r.id() == row.id()) {
            return Err(ArchiveError);
        }
        let f = wire::fields(row)?;
        for value in &f {
            if value.len() > MAX_FIELD_BYTES {
                return Err(ArchiveError);
            }
            total += value.len();
        }
        if total > MAX_LOGICAL_BYTES {
            return Err(ArchiveError);
        }
        fields.push(f);
    }
    Ok(fields)
}
fn digest(fields: &[wire::Fields]) -> String {
    let mut hash = Sha256::new();
    hash.update(FORMAT.as_bytes());
    for row in fields {
        for field in row {
            hash.update(
                u64::try_from(field.len())
                    .expect("bounded field length")
                    .to_be_bytes(),
            );
            hash.update(field.as_bytes());
        }
    }
    let mut result = String::with_capacity(64);
    for byte in hash.finalize() {
        result.push(char::from(HEX[usize::from(byte >> 4)]));
        result.push(char::from(HEX[usize::from(byte & 15)]));
    }
    result
}
/// Encodes exact evidence into one uncompressed, flat Parquet row group.
/// # Errors
/// Rejects invalid scope, duplicate identities, size limits or writer failures.
pub fn encode(tenant: &TenantId, rows: &[StoredPropertyChange]) -> Result<Vec<u8>, ArchiveError> {
    let fields = validate(tenant, rows)?;
    let props = WriterProperties::builder()
        .set_writer_version(WriterVersion::PARQUET_1_0)
        .set_dictionary_enabled(false)
        .set_encoding(Encoding::PLAIN)
        .set_compression(Compression::UNCOMPRESSED)
        .set_statistics_enabled(EnabledStatistics::None)
        .set_offset_index_disabled(true)
        .set_created_by("fleetiq-archive-v1".into())
        .set_write_batch_size(MAX_ROWS)
        .set_data_page_size_limit(MAX_FILE_BYTES)
        .set_key_value_metadata(Some(vec![
            KeyValue::new("fleetiq.archive.format".into(), Some(FORMAT.into())),
            KeyValue::new("fleetiq.archive.sha256".into(), Some(digest(&fields))),
        ]))
        .build();
    let mut writer = SerializedFileWriter::new(Vec::new(), schema()?, Arc::new(props))?;
    let mut group = writer.next_row_group()?;
    for i in 0..wire::NAMES.len() {
        let values = fields
            .iter()
            .map(|f| ByteArray::from(f[i].as_str()))
            .collect::<Vec<_>>();
        let mut column = group.next_column()?.ok_or(ArchiveError)?;
        column
            .typed::<ByteArrayType>()
            .write_batch(&values, None, None)?;
        column.close()?;
    }
    group.close()?;
    let bytes = writer.into_inner()?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err(ArchiveError);
    }
    Ok(bytes)
}
/// Decodes only the version-one canonical profile and revalidates domain evidence.
/// # Errors
/// Rejects malformed metadata/pages, unsupported profiles, scope mismatch or limits.
pub fn decode(tenant: &TenantId, bytes: &[u8]) -> Result<Vec<StoredPropertyChange>, ArchiveError> {
    if bytes.len() < 12
        || bytes.len() > MAX_FILE_BYTES
        || bytes.get(..4) != Some(b"PAR1")
        || bytes.get(bytes.len() - 4..) != Some(b"PAR1")
    {
        return Err(ArchiveError);
    }
    let footer = u32::from_le_bytes(
        bytes[bytes.len() - 8..bytes.len() - 4]
            .try_into()
            .map_err(|_| ArchiveError)?,
    );
    let footer = usize::try_from(footer).map_err(|_| ArchiveError)?;
    if footer > MAX_FOOTER || footer > bytes.len() - 12 {
        return Err(ArchiveError);
    }
    let footer_start = bytes.len() - 8 - footer;
    // Supplying the fixed flat schema avoids reconstructing attacker-controlled schema trees.
    let options = ReadOptionsBuilder::new()
        .with_parquet_schema(Arc::new(SchemaDescriptor::new(schema()?)))
        .build();
    let reader = SerializedFileReader::new_with_options(Bytes::copy_from_slice(bytes), options)?;
    let metadata = reader.metadata();
    let file = metadata.file_metadata();
    if metadata.num_row_groups() != 1
        || file.key_value_metadata().is_none_or(|m| {
            m.len() != 2
                || m[0].key != "fleetiq.archive.format"
                || m[0].value.as_deref() != Some(FORMAT)
                || m[1].key != "fleetiq.archive.sha256"
        })
    {
        return Err(ArchiveError);
    }
    let rows = usize::try_from(file.num_rows()).map_err(|_| ArchiveError)?;
    let group = metadata.row_group(0);
    if rows == 0
        || rows > MAX_ROWS
        || group.num_rows() != file.num_rows()
        || group.num_columns() != wire::NAMES.len()
    {
        return Err(ArchiveError);
    }
    let mut columns = Vec::new();
    let mut offset = 4;
    let mut logical = 0;
    for column in group.columns() {
        if column.compression() != Compression::UNCOMPRESSED
            || column.dictionary_page_offset().is_some()
            || column.file_path().is_some()
            || column.num_values() != file.num_rows()
        {
            return Err(ArchiveError);
        }
        let start = usize::try_from(column.data_page_offset()).map_err(|_| ArchiveError)?;
        let size = usize::try_from(column.compressed_size()).map_err(|_| ArchiveError)?;
        let end = start.checked_add(size).ok_or(ArchiveError)?;
        if start != offset
            || end > footer_start
            || column.uncompressed_size() != column.compressed_size()
        {
            return Err(ArchiveError);
        }
        let values = profile::column(&bytes[start..end], rows)?;
        logical += values.iter().map(String::len).sum::<usize>();
        if logical > MAX_LOGICAL_BYTES {
            return Err(ArchiveError);
        }
        columns.push(values);
        offset = end;
    }
    if offset != footer_start {
        return Err(ArchiveError);
    }
    let mut result = Vec::with_capacity(rows);
    for (row, _) in columns[0].iter().enumerate() {
        let fields = std::array::from_fn(|i| columns[i][row].clone());
        result.push(wire::reconstruct(&fields)?);
    }
    let checked = validate(tenant, &result)?;
    if file
        .key_value_metadata()
        .and_then(|m| m[1].value.as_deref())
        != Some(digest(&checked).as_str())
    {
        return Err(ArchiveError);
    }
    Ok(result)
}

/// Immutable operator-configured archive object storage.
pub mod objects;
mod publication;
mod restoration;
