//! Namespace and cursor scope are application invariants for non-HTTP callers too.
use super::*;
use fleetiq_domain::DefinitionVersion;
fn position(kind: DefinitionKind, id: &str) -> DefinitionPosition {
    DefinitionPosition::new(
        DefinitionIdentity::new(kind, id.into()).unwrap(),
        DefinitionVersion::new(1).unwrap(),
    )
}
#[test]
fn catalogue_rejects_cross_namespace_and_cross_identity_cursors() {
    let kind = DefinitionKind::Property;
    let identity = DefinitionIdentity::new(kind, "temperature".into()).unwrap();
    for (filter, cursor) in [
        (
            None,
            Some(position(DefinitionKind::AssetType, "temperature")),
        ),
        (
            Some(DefinitionIdentity::new(DefinitionKind::AssetType, "temperature".into()).unwrap()),
            None,
        ),
        (Some(identity.clone()), Some(position(kind, "other"))),
    ] {
        assert!(DefinitionCatalogueQuery::new(kind, 50, filter, cursor).is_err());
    }
    for limit in [0, 101, 255] {
        assert!(DefinitionCatalogueQuery::new(kind, limit, None, None).is_err());
    }
    for limit in [1, 100] {
        assert!(
            DefinitionCatalogueQuery::new(
                kind,
                limit,
                Some(identity.clone()),
                Some(position(kind, "temperature"))
            )
            .is_ok()
        );
    }
}
