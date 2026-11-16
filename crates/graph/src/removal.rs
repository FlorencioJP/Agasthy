//! "What happens if I remove X?" for any entity, built only from recorded,
//! evidence-backed relationships. Nothing is removed: this is a simulation.

use std::collections::{BTreeSet, HashMap, VecDeque};

use agasthy_core::{Confidence, CoreResult, Entity, EntityKind, EntityRef, RelationKind, Store};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{ImpactOutcome, ImpactReport, runtime_impact};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ImpactEntry {
    pub entity: EntityRef,
    pub explanation: String,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ImpactSection {
    pub title: String,
    pub entries: Vec<ImpactEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RemovalImpact {
    pub target: EntityRef,
    pub risk: RiskLevel,
    pub summary: String,
    pub sections: Vec<ImpactSection>,
    /// Per-runtime project analysis, for runtimes removed directly or with a package.
    pub runtime_reports: Vec<ImpactReport>,
    /// What this analysis cannot see.
    pub unknowns: Vec<String>,
    /// Suggested steps. AGASTHY does not run these.
    pub plan: Vec<String>,
    pub recovery: Vec<String>,
}

fn eref(e: &Entity) -> EntityRef {
    EntityRef {
        id: e.id.clone(),
        kind: e.kind,
        name: e.name.clone(),
        path: e.path.clone(),
    }
}

fn entities(store: &Store, ids: &[String]) -> CoreResult<HashMap<String, Entity>> {
    Ok(store
        .entities_by_ids(ids)?
        .into_iter()
        .map(|e| (e.id.clone(), e))
        .collect())
}

/// Incoming edges of `kind` to `id`, with their source entities.
fn incoming(
    store: &Store,
    id: &str,
    kind: RelationKind,
) -> CoreResult<Vec<(agasthy_core::Edge, Entity)>> {
    let edges: Vec<_> = store
        .edges_touching(id)?
        .into_iter()
        .filter(|e| e.kind == kind && e.to_id == id)
        .collect();
    let ids: Vec<String> = edges.iter().map(|e| e.from_id.clone()).collect();
    let map = entities(store, &ids)?;
    Ok(edges
        .into_iter()
        .filter_map(|e| map.get(&e.from_id).cloned().map(|ent| (e, ent)))
        .collect())
}

fn outgoing(
    store: &Store,
    id: &str,
    kind: RelationKind,
) -> CoreResult<Vec<(agasthy_core::Edge, Entity)>> {
    let edges: Vec<_> = store
        .edges_touching(id)?
        .into_iter()
        .filter(|e| e.kind == kind && e.from_id == id)
        .collect();
    let ids: Vec<String> = edges.iter().map(|e| e.to_id.clone()).collect();
    let map = entities(store, &ids)?;
    Ok(edges
        .into_iter()
        .filter_map(|e| map.get(&e.to_id).cloned().map(|ent| (e, ent)))
        .collect())
}

fn worst(reports: &[ImpactReport]) -> RiskLevel {
    if reports.iter().any(|r| r.breaks > 0) {
        RiskLevel::High
    } else if reports.iter().any(|r| r.unknown > 0) {
        RiskLevel::Unknown
    } else if reports.iter().any(|r| r.falls_back > 0) {
        RiskLevel::Medium
    } else {
        RiskLevel::Low
    }
}

pub fn removal_impact(store: &Store, id: &str) -> CoreResult<Option<RemovalImpact>> {
    let Some(target) = store.get_entity(id)? else {
        return Ok(None);
    };
    let mut sections = Vec::new();
    let mut runtime_reports = Vec::new();
    let mut unknowns = Vec::new();
    let mut plan = Vec::new();
    let mut recovery = Vec::new();
    let mut risk = RiskLevel::Low;

    match target.kind {
        EntityKind::Runtime => {
            if let Some(r) = runtime_impact(store, id)? {
                risk = worst(std::slice::from_ref(&r));
                runtime_reports.push(r);
            }
            let installer = outgoing(store, id, RelationKind::InstalledBy)?;
            if let Some((_, pkg)) = installer.first() {
                plan.push(format!("This copy was installed by the package `{}`; remove it through that package manager, not by deleting the file.", pkg.name));
                recovery.push(format!("Reinstall the package `{}`.", pkg.name));
            }
            unknowns.push("Scripts, editors and services that call this runtime by absolute path are not detected.".into());
        }
        EntityKind::Package => {
            let manager = target
                .attributes
                .get("manager")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            // Packages that depend on it, transitively.
            let mut seen = BTreeSet::new();
            let mut queue = VecDeque::from([(target.id.clone(), 0u32)]);
            let mut dependents = Vec::new();
            while let Some((cur, depth)) = queue.pop_front() {
                for (_, ent) in incoming(store, &cur, RelationKind::DependsOn)? {
                    if ent.kind != EntityKind::Package || !seen.insert(ent.id.clone()) {
                        continue;
                    }
                    let explicit = ent
                        .attributes
                        .get("installed_on_request")
                        .and_then(|v| v.as_bool());
                    dependents.push(ImpactEntry {
                        explanation: format!(
                            "{}depends on it{}",
                            if depth == 0 { "" } else { "indirectly " },
                            match explicit {
                                Some(true) => " (you installed it explicitly)",
                                Some(false) => " (installed as a dependency)",
                                None => "",
                            }
                        ),
                        entity: eref(&ent),
                        confidence: Confidence::Verified,
                    });
                    queue.push_back((ent.id.clone(), depth + 1));
                }
            }
            // Runtimes provided by dependent packages stop working too, and so do
            // the projects that need those runtimes.
            let mut broken_runtimes = Vec::new();
            for d in &dependents {
                for (_, rt) in incoming(store, &d.entity.id, RelationKind::InstalledBy)? {
                    if rt.kind != EntityKind::Runtime {
                        continue;
                    }
                    broken_runtimes.push(ImpactEntry {
                        explanation: format!(
                            "provided by {}, which depends on {}; it may fail to start",
                            d.entity.name, target.name
                        ),
                        entity: eref(&rt),
                        confidence: Confidence::Strong,
                    });
                    if let Some(r) = runtime_impact(store, &rt.id)? {
                        runtime_reports.push(r);
                    }
                }
            }
            if !dependents.is_empty() {
                risk = RiskLevel::High;
                sections.push(ImpactSection {
                    title: format!("Packages that need {}", target.name),
                    entries: dependents,
                });
            }
            if !broken_runtimes.is_empty() {
                sections.push(ImpactSection {
                    title: "Runtimes that depend on it".into(),
                    entries: broken_runtimes,
                });
            }
            // Executables and runtimes that come from this package.
            let provided = incoming(store, id, RelationKind::InstalledBy)?;
            let mut removed = Vec::new();
            for (_, ent) in &provided {
                removed.push(ImpactEntry {
                    entity: eref(ent),
                    explanation:
                        "this executable resolves into the package and would be removed with it"
                            .into(),
                    confidence: Confidence::Verified,
                });
                if ent.kind == EntityKind::Runtime
                    && let Some(r) = runtime_impact(store, &ent.id)?
                {
                    runtime_reports.push(r);
                }
            }
            for (_, app) in outgoing(store, id, RelationKind::Provides)? {
                removed.push(ImpactEntry {
                    entity: eref(&app),
                    explanation: "application installed by this cask".into(),
                    confidence: Confidence::Strong,
                });
            }
            if !removed.is_empty() {
                sections.push(ImpactSection {
                    title: "Removed with it".into(),
                    entries: removed,
                });
            }
            risk = risk.max(worst(&runtime_reports));
            if manager == "homebrew" {
                let cask = target.id.starts_with("package:homebrew-cask:");
                plan.push(format!(
                    "Double-check with `brew uses --installed {}`.",
                    target.name
                ));
                plan.push(format!(
                    "Remove with `brew uninstall {}{}` once nothing above needs it.",
                    if cask { "--cask " } else { "" },
                    target.name
                ));
                recovery.push(format!(
                    "`brew install {}{}`{}",
                    if cask { "--cask " } else { "" },
                    target.name,
                    target
                        .version
                        .as_deref()
                        .map(|v| format!(" (you have {v})"))
                        .unwrap_or_default()
                ));
            } else {
                plan.push(format!("Remove it with {manager}'s own uninstall command."));
                recovery.push(format!(
                    "Reinstall it with {manager}{}.",
                    target
                        .version
                        .as_deref()
                        .map(|v| format!(" (version {v})"))
                        .unwrap_or_default()
                ));
            }
            unknowns.push("Projects that use this package directly from PATH without declaring it are not detected.".into());
        }
        EntityKind::Dependency => {
            let users = incoming(store, id, RelationKind::DependsOn)?;
            let entries: Vec<ImpactEntry> = users
                .iter()
                .map(|(edge, ent)| {
                    let decls = edge
                        .attributes
                        .get("declarations")
                        .and_then(|d| d.as_array())
                        .cloned()
                        .unwrap_or_default();
                    let desc: Vec<String> = decls
                        .iter()
                        .map(|d| {
                            format!(
                                "{} {}{}",
                                d.get("manifest").and_then(|v| v.as_str()).unwrap_or("?"),
                                d.get("constraint")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("(any)"),
                                d.get("resolved")
                                    .and_then(|v| v.as_str())
                                    .map(|r| format!(" → {r}"))
                                    .unwrap_or_default()
                            )
                        })
                        .collect();
                    ImpactEntry {
                        entity: eref(ent),
                        explanation: desc.join("; "),
                        confidence: Confidence::Verified,
                    }
                })
                .collect();
            if !entries.is_empty() {
                risk = if entries.len() > 1 {
                    RiskLevel::High
                } else {
                    RiskLevel::Medium
                };
                sections.push(ImpactSection {
                    title: "Projects that declare it".into(),
                    entries,
                });
            }
            plan.push("Removing a declared dependency means editing each project's manifest; imports of it will fail until code is changed.".into());
            plan.push("To test a version change safely, use the Update Lab.".into());
            recovery.push("Revert the manifest and lockfile change with Git.".into());
            unknowns
                .push("Transitive use (other dependencies that need it) is not analysed.".into());
        }
        _ => {
            // Generic: anything that points at it.
            let mut entries = Vec::new();
            for e in store
                .edges_touching(id)?
                .into_iter()
                .filter(|e| e.to_id == id)
            {
                if let Some(src) = store.get_entity(&e.from_id)? {
                    entries.push(ImpactEntry {
                        explanation: format!(
                            "{} ({})",
                            e.kind.as_str().replace('_', " "),
                            e.confidence.as_str()
                        ),
                        entity: eref(&src),
                        confidence: e.confidence,
                    });
                }
            }
            if !entries.is_empty() {
                risk = RiskLevel::Medium;
                sections.push(ImpactSection {
                    title: "Things that refer to it".into(),
                    entries,
                });
            }
            if let Some((_, pkg)) = outgoing(store, id, RelationKind::InstalledBy)?.first() {
                plan.push(format!(
                    "Installed by `{}`: remove it through that package manager.",
                    pkg.name
                ));
            }
            unknowns.push("Only recorded relationships are considered; uses that left no evidence on disk are not visible.".into());
        }
    }

    let breaks: u32 = runtime_reports.iter().map(|r| r.breaks).sum();
    let summary = match risk {
        RiskLevel::High if breaks > 0 => {
            format!("Removing {} would break {breaks} project(s).", target.name)
        }
        RiskLevel::High => format!("Other installed software depends on {}.", target.name),
        RiskLevel::Medium => format!(
            "{} is referenced; review the items below first.",
            target.name
        ),
        RiskLevel::Unknown => format!(
            "Some effects of removing {} cannot be determined; see unknowns.",
            target.name
        ),
        RiskLevel::Low => format!("No recorded dependents of {} were found.", target.name),
    };
    if runtime_reports
        .iter()
        .any(|r| r.items.iter().any(|i| i.outcome == ImpactOutcome::Unknown))
    {
        unknowns.push("Some projects do not declare a runtime version, so compatibility with the fallback is unknown.".into());
    }
    Ok(Some(RemovalImpact {
        target: eref(&target),
        risk,
        summary,
        sections,
        runtime_reports,
        unknowns,
        plan,
        recovery,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agasthy_core::{EntityInput, EvidenceInput, IngestBatch, RelationshipInput, ScanKind};
    use serde_json::json;

    fn ent(
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
            path: None,
            version: version.map(Into::into),
            source: "test".into(),
            attributes: attrs,
            evidence: vec![],
        }
    }
    fn rel(
        from: &str,
        to: &str,
        kind: RelationKind,
        attrs: serde_json::Value,
    ) -> RelationshipInput {
        RelationshipInput {
            from_id: from.into(),
            to_id: to.into(),
            kind,
            confidence: Confidence::Verified,
            attributes: attrs,
            evidence: vec![EvidenceInput::new("t", "t", "t", Confidence::Verified)],
        }
    }

    #[test]
    fn package_removal_follows_dependents_binaries_and_projects() {
        let mut store = Store::open_in_memory().unwrap();
        let mut b = IngestBatch::new(ScanKind::Packages);
        b.entities.push(ent(
            "package:homebrew:python@3.13",
            EntityKind::Package,
            "python@3.13",
            Some("3.13.7"),
            json!({"manager": "homebrew"}),
        ));
        b.entities.push(ent(
            "package:homebrew:pipx",
            EntityKind::Package,
            "pipx",
            Some("1.7"),
            json!({"manager": "homebrew", "installed_on_request": true}),
        ));
        b.entities.push(ent(
            "package:homebrew:tool-on-pipx",
            EntityKind::Package,
            "tool-on-pipx",
            None,
            json!({"manager": "homebrew"}),
        ));
        b.entities.push(ent(
            "runtime:/opt/homebrew/bin/python3",
            EntityKind::Runtime,
            "Python 3",
            Some("3.13.7"),
            json!({"tool": "python3", "active": true, "rank": 0}),
        ));
        b.entities.push(ent(
            "repository:/p/app",
            EntityKind::Repository,
            "app",
            None,
            json!({}),
        ));
        b.relationships.push(rel(
            "package:homebrew:pipx",
            "package:homebrew:python@3.13",
            RelationKind::DependsOn,
            json!(null),
        ));
        b.relationships.push(rel(
            "package:homebrew:tool-on-pipx",
            "package:homebrew:pipx",
            RelationKind::DependsOn,
            json!(null),
        ));
        b.relationships.push(rel(
            "runtime:/opt/homebrew/bin/python3",
            "package:homebrew:python@3.13",
            RelationKind::InstalledBy,
            json!(null),
        ));
        b.entities.push(ent(
            "runtime:/opt/homebrew/bin/pipx-runtime",
            EntityKind::Runtime,
            "pipx runtime",
            Some("1.0"),
            json!({"tool": "python", "active": false, "rank": 1}),
        ));
        b.relationships.push(rel(
            "runtime:/opt/homebrew/bin/pipx-runtime",
            "package:homebrew:pipx",
            RelationKind::InstalledBy,
            json!(null),
        ));
        b.relationships.push(rel("repository:/p/app", "runtime:/opt/homebrew/bin/python3", RelationKind::RequiresRuntime, json!({"runtime": "python", "declarations": [{"manifest": "pyproject.toml", "constraint": ">=3.12", "scope": "requires-python", "resolved": null}]})));
        let id = store.begin_scan(ScanKind::Packages).unwrap();
        store.ingest(id, &b).unwrap();

        let r = removal_impact(&store, "package:homebrew:python@3.13")
            .unwrap()
            .unwrap();
        assert_eq!(r.risk, RiskLevel::High);
        let dependents = &r
            .sections
            .iter()
            .find(|s| s.title.starts_with("Packages that need"))
            .unwrap()
            .entries;
        assert_eq!(dependents.len(), 2, "direct and transitive");
        assert!(
            dependents
                .iter()
                .any(|d| d.explanation.starts_with("indirectly"))
        );
        assert!(
            r.sections
                .iter()
                .any(|s| s.title == "Runtimes that depend on it"
                    && s.entries[0].entity.name == "pipx runtime")
        );
        assert_eq!(
            r.runtime_reports.len(),
            2,
            "the package's own runtime and the dependent's"
        );
        let own = r
            .runtime_reports
            .iter()
            .find(|x| x.target.id == "runtime:/opt/homebrew/bin/python3")
            .unwrap();
        assert_eq!(own.breaks, 1, "no other python on PATH");
        assert!(
            r.plan
                .iter()
                .any(|p| p.contains("brew uses --installed python@3.13"))
        );
        assert!(r.recovery[0].contains("brew install python@3.13"));
        assert!(r.summary.contains("break 1 project"));
    }

    #[test]
    fn dependency_and_unknown_entities() {
        let mut store = Store::open_in_memory().unwrap();
        let mut b = IngestBatch::new(ScanKind::Repositories);
        b.entities.push(ent(
            "dependency:npm:react",
            EntityKind::Dependency,
            "react",
            None,
            json!({}),
        ));
        b.entities.push(ent(
            "repository:/a",
            EntityKind::Repository,
            "a",
            None,
            json!({}),
        ));
        b.relationships.push(rel("repository:/a", "dependency:npm:react", RelationKind::DependsOn, json!({"declarations": [{"manifest": "package.json", "constraint": "^19.0.0", "scope": "runtime", "resolved": "19.3.1"}]})));
        let id = store.begin_scan(ScanKind::Repositories).unwrap();
        store.ingest(id, &b).unwrap();
        let r = removal_impact(&store, "dependency:npm:react")
            .unwrap()
            .unwrap();
        assert_eq!(r.risk, RiskLevel::Medium);
        assert_eq!(
            r.sections[0].entries[0].explanation,
            "package.json ^19.0.0 → 19.3.1"
        );
        assert!(removal_impact(&store, "nope").unwrap().is_none());
        let lonely = removal_impact(&store, "repository:/a").unwrap().unwrap();
        assert_eq!(lonely.risk, RiskLevel::Low);
    }
}
