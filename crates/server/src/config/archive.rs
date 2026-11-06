//! Complete-or-absent archive configuration with no ambient AWS discovery.
use super::ServerConfigError;
use fleetiq_archive::objects::S3ArchiveConfig;
pub(super) fn read(
    values: [Option<String>; 5],
) -> Result<Option<S3ArchiveConfig>, ServerConfigError> {
    match values {
        [None, None, None, None, None] => Ok(None),
        [
            Some(endpoint),
            Some(bucket),
            Some(region),
            Some(access),
            Some(secret),
        ] => S3ArchiveConfig::new(&endpoint, &bucket, &region, access, secret)
            .map(Some)
            .map_err(|_| ServerConfigError::InvalidArchive),
        _ => Err(ServerConfigError::InvalidArchive),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn archive_values_are_explicit_complete_and_redacted() {
        assert!(read([None, None, None, None, None]).unwrap().is_none());
        assert!(read([Some("https://store.example".into()), None, None, None, None]).is_err());
        let config = read([
            Some("https://store.example".into()),
            Some("archive-test".into()),
            Some("us-east-1".into()),
            Some("fixture-access".into()),
            Some("fixture-secret".into()),
        ])
        .unwrap()
        .unwrap();
        assert!(!format!("{config:?}").contains("fixture-secret"));
    }
}
