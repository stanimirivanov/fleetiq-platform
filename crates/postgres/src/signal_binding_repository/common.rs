use bigdecimal::BigDecimal;
use fleetiq_domain::RecordRevision;

use crate::error::{PostgresRepositoryError, corrupt};

pub(super) fn parse_revision(
    field: &'static str,
    value: &BigDecimal,
) -> Result<RecordRevision, PostgresRepositoryError> {
    let value = value
        .to_string()
        .parse::<u64>()
        .map_err(|error| corrupt(field, error))?;
    RecordRevision::new(value).map_err(|error| corrupt(field, error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_the_complete_record_revision_range() {
        let stored = BigDecimal::from(u64::MAX);

        assert_eq!(
            parse_revision("test.revision", &stored)
                .expect("maximum u64 should remain representable")
                .get(),
            u64::MAX
        );
    }
}
