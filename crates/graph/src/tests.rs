use agasthy_core::{
    Confidence, EntityInput, EntityKind, IngestBatch, RelationKind, RelationshipInput, ScanKind,
    Store,
};
use serde_json::json;

use super::*;

fn entity(
    id: &str,
    kind: EntityKind,
    name: &str,
    version: Option<&str>,
    attrs: serde_json::Value,
) -> EntityInput {
    EntityInput {
        id: id.into(),
        kind,
        name: name.into(),
        path: Some(format!("/x/{name}")),
        version: version.map(str::to_string),
        source: "test".into(),
        attributes: attrs,
        evidence: vec![],
    }
}

fn requires(from: &str, to: &str, constraint: Option<&str>) -> RelationshipInput {
    RelationshipInput {
        from_id: from.into(),
        to_id: to.into(),
        kind: RelationKind::RequiresRuntime,
        confidence: Confidence::Verified,
        attributes: json!({"runtime": "node", "declarations": [
            {"manifest": "package.json", "constraint": constraint, "scope": "engines.node", "resolved": null}
        ]}),
        evidence: vec![],
    }
}

fn depends(from: &str, to: &str) -> RelationshipInput {
    RelationshipInput {
        from_id: from.into(),
        to_id: to.into(),
        kind: RelationKind::DependsOn,
        confidence: Confidence::Verified,
        attributes: serde_json::Value::Null,
        evidence: vec![],
    }
}

/// node 22 (active) and node 18 (shadowed); four projects with different needs.
fn fixture_store() -> Store {
    let mut store = Store::open_in_memory().unwrap();
    let mut b = IngestBatch::new(ScanKind::Repositories);
    let n22 = "runtime:/opt/homebrew/bin/node";
    let n18 = "runtime:/usr/local/bin/node";
    b.entities.push(entity(
        n22,
        EntityKind::Runtime,
        "Node.js",
        Some("22.3.0"),
        json!({"tool": "node", "active": true, "rank": 0}),
    ));
    b.entities.push(entity(
        n18,
        EntityKind::Runtime,
        "Node.js",
        Some("18.20.0"),
        json!({"tool": "node", "active": false, "rank": 1}),
    ));
    for (id, name) in [
        ("repository:/x/modern", "modern"),
        ("repository:/x/legacy", "legacy"),
        ("repository:/x/loose", "loose"),
        ("repository:/x/lts", "lts"),
        ("repository:/x/future", "future"),
    ] {
        b.entities
            .push(entity(id, EntityKind::Repository, name, None, json!({})));
    }
    b.entities.push(entity(
        "dependency:npm:react",
        EntityKind::Dependency,
        "react",
        None,
        json!({"ecosystem": "npm", "package": "react"}),
    ));
    b.entities.push(entity(
        "package_manager:homebrew",
        EntityKind::PackageManager,
        "Homebrew",
        None,
        json!({}),
    ));
    for to in [n22, n18] {
        b.relationships
            .push(requires("repository:/x/modern", to, Some(">=20")));
        b.relationships
            .push(requires("repository:/x/legacy", to, Some(">=16")));
        b.relationships
            .push(requires("repository:/x/loose", to, None));
        b.relationships
            .push(requires("repository:/x/lts", to, Some("lts/*")));
        b.relationships
            .push(requires("repository:/x/future", to, Some(">=24")));
    }
    b.relationships
        .push(depends("repository:/x/modern", "dependency:npm:react"));
    b.relationships
        .push(depends("repository:/x/legacy", "dependency:npm:react"));
    let id = store.begin_scan(ScanKind::Repositories).unwrap();
    store.ingest(id, &b).unwrap();
    store
}

#[test]
fn removing_the_active_runtime() {
    let store = fixture_store();
    let r = runtime_impact(&store, "runtime:/opt/homebrew/bin/node")
        .unwrap()
        .unwrap();
    let outcome = |name: &str| {
        r.items
            .iter()
            .find(|i| i.project.name == name)
            .unwrap()
            .outcome
    };
    assert_eq!(
        outcome("modern"),
        ImpactOutcome::Breaks,
        "18 does not satisfy >=20"
    );
    assert_eq!(
        outcome("legacy"),
        ImpactOutcome::FallsBack,
        "18 satisfies >=16"
    );
    assert_eq!(
        outcome("loose"),
        ImpactOutcome::Unknown,
        "no declared version"
    );
    assert_eq!(
        outcome("lts"),
        ImpactOutcome::Unknown,
        "lts/* is not evaluable"
    );
    assert_eq!(outcome("future"), ImpactOutcome::AlreadyIncompatible);
    assert_eq!(r.breaks, 1);
    assert_eq!(r.falls_back, 1);
    assert_eq!(r.unknown, 2);
    assert_eq!(r.items[0].outcome, ImpactOutcome::Breaks, "worst first");
    assert_eq!(r.other_copies.len(), 1);
    assert!(!r.limitations.is_empty());
}

#[test]
fn removing_a_shadowed_runtime_changes_nothing() {
    let store = fixture_store();
    let r = runtime_impact(&store, "runtime:/usr/local/bin/node")
        .unwrap()
        .unwrap();
    assert!(
        r.items
            .iter()
            .all(|i| i.outcome == ImpactOutcome::Unaffected)
    );
    assert_eq!(r.breaks, 0);
}

#[test]
fn non_runtime_has_no_report() {
    let store = fixture_store();
    assert!(
        runtime_impact(&store, "repository:/x/modern")
            .unwrap()
            .is_none()
    );
    assert!(
        runtime_impact(&store, "runtime:/missing")
            .unwrap()
            .is_none()
    );
}

#[test]
fn neighborhood_and_overview() {
    let store = fixture_store();
    let v = neighborhood(&store, "dependency:npm:react", 1, &[], 100).unwrap();
    let ids: Vec<&str> = v.nodes.iter().map(|n| n.id.as_str()).collect();
    assert!(ids.contains(&"repository:/x/modern") && ids.contains(&"repository:/x/legacy"));
    assert_eq!(v.edges.len(), 2);
    assert!(!v.truncated);

    // Depth 2 from a repo reaches the other repo through the shared dependency.
    let v2 = neighborhood(&store, "repository:/x/modern", 2, &[], 100).unwrap();
    assert!(v2.nodes.iter().any(|n| n.id == "repository:/x/legacy"));

    // Kind filter keeps only runtimes (plus the center).
    let v3 = neighborhood(
        &store,
        "repository:/x/modern",
        1,
        &[EntityKind::Runtime],
        100,
    )
    .unwrap();
    assert!(
        v3.nodes
            .iter()
            .all(|n| n.kind == EntityKind::Runtime || n.id == "repository:/x/modern")
    );

    // Node limit is reported.
    let v4 = neighborhood(&store, "runtime:/opt/homebrew/bin/node", 1, &[], 3).unwrap();
    assert!(v4.truncated);
    assert_eq!(v4.nodes.len(), 3);

    let o = overview(&store, OverviewKind::Projects, 500).unwrap();
    assert!(
        o.nodes.iter().any(|n| n.id == "dependency:npm:react"),
        "shared by 2 projects"
    );
    assert!(o.nodes.iter().any(|n| n.kind == EntityKind::Runtime));
    assert!(!o.nodes.iter().any(|n| n.kind == EntityKind::PackageManager));

    let shared = shared_dependencies(&store, 10).unwrap();
    assert_eq!(shared[0].dependency.name, "react");
    assert_eq!(shared[0].projects, 2);
}

#[test]
fn ids_map_to_kinds() {
    assert_eq!(
        kind_of_id("package_manager:homebrew"),
        Some(EntityKind::PackageManager)
    );
    assert_eq!(
        kind_of_id("remote:github.com/a/b"),
        Some(EntityKind::RemoteProject)
    );
    assert_eq!(kind_of_id("nonsense"), None);
}
