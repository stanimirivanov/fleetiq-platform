use fleetiq_domain::DefinitionVersion;

use crate::error::{PostgresRepositoryError, corrupt};

pub(super) fn definition_version(
    field: &'static str,
    value: i64,
) -> Result<DefinitionVersion, PostgresRepositoryError> {
    let value = u32::try_from(value).map_err(|error| corrupt(field, error))?;
    DefinitionVersion::new(value).map_err(|error| corrupt(field, error))
}
