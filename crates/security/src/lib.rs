//! Security and lifecycle findings.
//!
//! * **Outdated software** comes from Homebrew's local data (`brew outdated`),
//!   with auto-update disabled.
//! * **Unknown provenance**: active tools on PATH whose installation source
//!   could not be determined.
//! * **Vulnerabilities** come from OSV.dev, **only when the user has turned
//!   network checks on**. Exactly what is sent is package name, ecosystem and
//!   version: no paths, project names or machine identifiers.
//!
//! A clean result never means "secure": it means no *known* advisory matched
//! the packages AGASTHY could identify.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

use agasthy_core::{
    Confidence, EntityInput, EntityKind, EvidenceInput, IngestBatch, IssueKind, PruneScope,
    RelationKind, RelationshipInput, ScanIssue, ScanKind,
};
use agasthy_platform::{CommandRunner, CommandSpec};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const OSV_QUERY_URL: &str = "https://api.osv.dev/v1/querybatch";
pub const OSV_VULN_URL: &str = "https://api.osv.dev/v1/vulns/";
const MAX_DETAILS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
    Info,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FindingAttrs {
    /// `vulnerability`, `outdated` or `provenance`.
    pub category: String,
    pub finding_id: String,
    pub aliases: Vec<String>,
    pub summary: String,
    pub severity: Severity,
    /// `osv.dev`, `homebrew` or `agasthy`.
    pub source: String,
    pub packages: Vec<String>,
    pub fixed_versions: Vec<String>,
    pub remediation: String,
    pub references: Vec<String>,
    pub published: Option<String>,
}

/// A package version AGASTHY can ask OSV about.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct OsvQuery {
    /// Entity this version belongs to (package or project dependency).
    pub entity_id: String,
    /// OSV ecosystem: `npm`, `PyPI`, `crates.io`, `Go`.
    pub ecosystem: String,
    pub name: String,
    pub version: String,
}

/// An installed tool whose origin is unknown: (entity id, name, path).
pub type UnknownOrigin = (String, String, String);

pub struct SecurityInput<'a> {
    pub osv_enabled: bool,
    pub queries: Vec<OsvQuery>,
    pub unknown_origin: Vec<UnknownOrigin>,
    /// Folder for the temporary OSV request body.
    pub work_dir: &'a Path,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SecurityReport {
    pub osv_enabled: bool,
    /// Number of package versions sent to OSV.dev (0 when disabled).
    pub sent_to_osv: u32,
    pub eligible_for_osv: u32,
    pub vulnerabilities: u32,
    pub outdated: u32,
    pub unknown_origin: u32,
    pub not_covered: Vec<String>,
}

/// Map an AGASTHY ecosystem/manager to OSV's name. `None`: OSV has no data for it.
pub fn osv_ecosystem(eco: &str) -> Option<&'static str> {
    Some(match eco {
        "npm" | "pnpm" => "npm",
        "pip" | "pypi" | "uv" => "PyPI",
        "cargo" => "crates.io",
        "go" => "Go",
        _ => return None,
    })
}

fn severity_from(v: &serde_json::Value) -> Severity {
    let word = v
        .get("database_specific")
        .and_then(|d| d.get("severity"))
        .and_then(|s| s.as_str())
        .or_else(|| {
            v.get("affected").and_then(|a| a.as_array()).and_then(|a| {
                a.iter()
                    .find_map(|x| x.get("ecosystem_specific")?.get("severity")?.as_str())
            })
        })
        .unwrap_or("")
        .to_ascii_uppercase();
    match word.as_str() {
        "CRITICAL" => Severity::Critical,
        "HIGH" => Severity::High,
        "MODERATE" | "MEDIUM" => Severity::Medium,
        "LOW" => Severity::Low,
        _ => Severity::Unknown,
    }
}

/// Parse one OSV vulnerability record.
pub fn parse_vuln(v: &serde_json::Value, package: &str) -> Option<FindingAttrs> {
    let id = v.get("id")?.as_str()?.to_string();
    let mut fixed = BTreeSet::new();
    for a in v
        .get("affected")
        .and_then(|a| a.as_array())
        .into_iter()
        .flatten()
    {
        if a.get("package")
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
            != Some(package)
        {
            continue;
        }
        for r in a
            .get("ranges")
            .and_then(|r| r.as_array())
            .into_iter()
            .flatten()
        {
            for e in r
                .get("events")
                .and_then(|e| e.as_array())
                .into_iter()
                .flatten()
            {
                if let Some(f) = e.get("fixed").and_then(|f| f.as_str()) {
                    fixed.insert(f.to_string());
                }
            }
        }
    }
    let fixed: Vec<String> = fixed.into_iter().collect();
    let summary = v
        .get("summary")
        .and_then(|s| s.as_str())
        .or_else(|| v.get("details").and_then(|d| d.as_str()))
        .unwrap_or("No summary provided")
        .chars()
        .take(300)
        .collect();
    Some(FindingAttrs {
        category: "vulnerability".into(),
        aliases: v
            .get("aliases")
            .and_then(|a| a.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        summary,
        severity: severity_from(v),
        source: "osv.dev".into(),
        packages: vec![package.to_string()],
        remediation: if fixed.is_empty() {
            "No fixed version is listed; check the advisory for mitigations.".into()
        } else {
            format!("Upgrade {package} to {} or later.", fixed.join(" / "))
        },
        fixed_versions: fixed,
        references: v
            .get("references")
            .and_then(|r| r.as_array())
            .map(|r| {
                r.iter()
                    .filter_map(|x| x.get("url")?.as_str().map(str::to_string))
                    .take(5)
                    .collect()
            })
            .unwrap_or_default(),
        published: v
            .get("published")
            .and_then(|p| p.as_str())
            .map(str::to_string),
        finding_id: id,
    })
}

fn curl(runner: &dyn CommandRunner, args: &[&str]) -> Result<String, String> {
    let out = runner
        .run(
            &CommandSpec::new("curl", args)
                .timeout(Duration::from_secs(90))
                .max_output(32 * 1024 * 1024),
        )
        .map_err(|e| e.to_string())?;
    if !out.success() {
        return Err(format!(
            "request failed (curl exit {:?}): {}",
            out.exit_code,
            out.stderr.lines().next().unwrap_or("")
        ));
    }
    Ok(out.stdout)
}

/// The exact arguments used for the batch query (exported for transparency and tests).
pub fn query_args(body: &Path) -> Vec<String> {
    vec![
        "--silent".into(),
        "--show-error".into(),
        "--fail".into(),
        "--max-time".into(),
        "60".into(),
        "--proto".into(),
        "=https".into(),
        "--header".into(),
        "Content-Type: application/json".into(),
        "--data-binary".into(),
        format!("@{}", body.display()),
        OSV_QUERY_URL.into(),
    ]
}

pub fn detail_args(id: &str) -> Vec<String> {
    vec![
        "--silent".into(),
        "--show-error".into(),
        "--fail".into(),
        "--max-time".into(),
        "30".into(),
        "--proto".into(),
        "=https".into(),
        format!("{OSV_VULN_URL}{id}"),
    ]
}

fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() < 80
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.:".contains(c))
}

fn brew_outdated(runner: &dyn CommandRunner, batch: &mut IngestBatch) -> u32 {
    let spec = CommandSpec::new("brew", &["outdated", "--json=v2"])
        .timeout(Duration::from_secs(120))
        .env("HOMEBREW_NO_AUTO_UPDATE", "1")
        .env("HOMEBREW_NO_ANALYTICS", "1")
        .env("HOMEBREW_NO_ENV_HINTS", "1");
    let out = match runner.run(&spec) {
        Ok(o) if o.success() => o,
        Ok(o) => {
            batch.issues.push(ScanIssue::new(
                IssueKind::CommandFailed,
                "brew outdated",
                o.stderr.lines().next().unwrap_or("failed").to_string(),
            ));
            return 0;
        }
        Err(agasthy_platform::CommandError::NotFound { .. }) => return 0,
        Err(e) => {
            batch.issues.push(ScanIssue::new(
                IssueKind::CommandFailed,
                "brew outdated",
                e.to_string(),
            ));
            return 0;
        }
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&out.stdout) else {
        batch.issues.push(ScanIssue::new(
            IssueKind::ParseError,
            "brew outdated",
            "unexpected output",
        ));
        return 0;
    };
    let mut n = 0;
    for (list, cask) in [("formulae", false), ("casks", true)] {
        for f in v.get(list).and_then(|x| x.as_array()).into_iter().flatten() {
            let Some(name) = f.get("name").and_then(|x| x.as_str()) else {
                continue;
            };
            let installed = f
                .get("installed_versions")
                .and_then(|x| x.as_array())
                .and_then(|a| a.last())
                .and_then(|x| x.as_str())
                .unwrap_or("?");
            let current = f
                .get("current_version")
                .and_then(|x| x.as_str())
                .unwrap_or("?");
            let pinned = f.get("pinned").and_then(|x| x.as_bool()).unwrap_or(false);
            let pkg_id = if cask {
                format!("package:homebrew-cask:{name}")
            } else {
                format!("package:homebrew:{name}")
            };
            let id = format!("finding:outdated:{pkg_id}");
            batch.entities.push(EntityInput {
                id: id.clone(),
                kind: EntityKind::Finding,
                name: format!("{name} {installed} → {current}"),
                path: None,
                version: Some(current.to_string()),
                source: "homebrew".into(),
                attributes: serde_json::to_value(FindingAttrs {
                    category: "outdated".into(),
                    finding_id: format!("outdated:{name}"),
                    aliases: vec![],
                    summary: format!(
                        "{name} {installed} is installed; {current} is available{}.",
                        if pinned { " (pinned)" } else { "" }
                    ),
                    severity: Severity::Info,
                    source: "homebrew".into(),
                    packages: vec![name.to_string()],
                    fixed_versions: vec![current.to_string()],
                    remediation: if pinned {
                        format!("Pinned: unpin with `brew unpin {name}` if you want to upgrade.")
                    } else {
                        format!(
                            "`brew upgrade {}{name}` (check the impact first).",
                            if cask { "--cask " } else { "" }
                        )
                    },
                    references: vec![],
                    published: None,
                })
                .unwrap_or_default(),
                evidence: vec![EvidenceInput::new(
                    "brew_outdated",
                    format!("{installed} installed, {current} available"),
                    spec.display(),
                    Confidence::Verified,
                )],
            });
            batch.relationships.push(RelationshipInput {
                from_id: pkg_id,
                to_id: id,
                kind: RelationKind::HasFinding,
                confidence: Confidence::Verified,
                attributes: serde_json::json!({ "installed_version": installed }),
                evidence: vec![],
            });
            n += 1;
        }
    }
    n
}

pub fn scan(
    input: &SecurityInput<'_>,
    runner: &dyn CommandRunner,
) -> (IngestBatch, SecurityReport) {
    let mut batch = IngestBatch::new(ScanKind::Security);
    let outdated = brew_outdated(runner, &mut batch);

    for (id, name, path) in &input.unknown_origin {
        let fid = format!("finding:provenance:{id}");
        batch.entities.push(EntityInput {
            id: fid.clone(),
            kind: EntityKind::Finding,
            name: format!("Origin unknown: {name}"),
            path: Some(path.clone()),
            version: None,
            source: "agasthy".into(),
            attributes: serde_json::to_value(FindingAttrs {
                category: "provenance".into(),
                finding_id: format!("provenance:{name}"),
                aliases: vec![],
                summary: format!("{name} at {path} is the active copy on PATH, but AGASTHY could not tell how it was installed."),
                severity: Severity::Info,
                source: "agasthy".into(),
                packages: vec![name.clone()],
                fixed_versions: vec![],
                remediation: "If you do not recognise it, check where it came from before relying on it.".into(),
                references: vec![],
                published: None,
            })
            .unwrap_or_default(),
            evidence: vec![EvidenceInput::new("no_install_hint", "resolved path matches no known installer", path.clone(), Confidence::Probable)],
        });
        batch.relationships.push(RelationshipInput {
            from_id: id.clone(),
            to_id: fid,
            kind: RelationKind::HasFinding,
            confidence: Confidence::Probable,
            attributes: serde_json::Value::Null,
            evidence: vec![],
        });
    }

    let mut report = SecurityReport {
        osv_enabled: input.osv_enabled,
        sent_to_osv: 0,
        eligible_for_osv: input.queries.len() as u32,
        vulnerabilities: 0,
        outdated,
        unknown_origin: input.unknown_origin.len() as u32,
        not_covered: vec!["Homebrew formulae and casks (OSV has no Homebrew data); outdated status is reported instead.".into()],
    };

    if input.osv_enabled && !input.queries.is_empty() {
        match osv(input, runner, &mut batch) {
            Ok((sent, vulns)) => {
                report.sent_to_osv = sent;
                report.vulnerabilities = vulns;
            }
            Err(e) => batch
                .issues
                .push(ScanIssue::new(IssueKind::CommandFailed, "osv.dev", e)),
        }
    }

    batch.prune.push(PruneScope {
        kind: EntityKind::Finding,
        source: Some("homebrew".into()),
    });
    batch.prune.push(PruneScope {
        kind: EntityKind::Finding,
        source: Some("agasthy".into()),
    });
    if input.osv_enabled {
        batch.prune.push(PruneScope {
            kind: EntityKind::Finding,
            source: Some("osv.dev".into()),
        });
    }
    (batch, report)
}

fn osv(
    input: &SecurityInput<'_>,
    runner: &dyn CommandRunner,
    batch: &mut IngestBatch,
) -> Result<(u32, u32), String> {
    // Deduplicate (ecosystem, name, version); remember which entities share each.
    let mut uniq: BTreeMap<(String, String, String), Vec<String>> = BTreeMap::new();
    for q in &input.queries {
        uniq.entry((q.ecosystem.clone(), q.name.clone(), q.version.clone()))
            .or_default()
            .push(q.entity_id.clone());
    }
    let keys: Vec<_> = uniq.keys().cloned().collect();
    let mut vuln_for: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let mut sent = 0u32;
    std::fs::create_dir_all(input.work_dir).map_err(|e| e.to_string())?;
    for (chunk_i, chunk) in keys.chunks(500).enumerate() {
        let body = serde_json::json!({
            "queries": chunk.iter().map(|(eco, name, ver)| serde_json::json!({"package": {"name": name, "ecosystem": eco}, "version": ver})).collect::<Vec<_>>()
        });
        let path = input.work_dir.join("osv-query.json");
        std::fs::write(&path, serde_json::to_vec(&body).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let args = query_args(&path);
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let resp = curl(runner, &refs);
        let _ = std::fs::remove_file(&path);
        let resp = resp?;
        sent += chunk.len() as u32;
        let v: serde_json::Value =
            serde_json::from_str(&resp).map_err(|e| format!("unexpected OSV response: {e}"))?;
        for (i, r) in v
            .get("results")
            .and_then(|r| r.as_array())
            .into_iter()
            .flatten()
            .enumerate()
        {
            for vuln in r
                .get("vulns")
                .and_then(|x| x.as_array())
                .into_iter()
                .flatten()
            {
                if let Some(id) = vuln
                    .get("id")
                    .and_then(|x| x.as_str())
                    .filter(|id| safe_id(id))
                {
                    vuln_for
                        .entry(id.to_string())
                        .or_default()
                        .push(chunk_i * 500 + i);
                }
            }
        }
    }
    let ids: Vec<String> = vuln_for.keys().take(MAX_DETAILS).cloned().collect();
    if vuln_for.len() > MAX_DETAILS {
        batch.issues.push(ScanIssue::new(
            IssueKind::Skipped,
            "osv.dev",
            format!(
                "{} advisories matched; details were fetched for the first {MAX_DETAILS}",
                vuln_for.len()
            ),
        ));
    }
    for id in &ids {
        let args = detail_args(id);
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let detail = curl(runner, &refs)
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).map_err(|e| e.to_string()));
        let idxs = &vuln_for[id];
        let first = &keys[idxs[0]];
        let mut attrs = match &detail {
            Ok(v) => parse_vuln(v, &first.1),
            Err(_) => None,
        }
        .unwrap_or_else(|| FindingAttrs {
            category: "vulnerability".into(),
            finding_id: id.clone(),
            aliases: vec![],
            summary: "Details could not be fetched.".into(),
            severity: Severity::Unknown,
            source: "osv.dev".into(),
            packages: vec![],
            fixed_versions: vec![],
            remediation: format!("See https://osv.dev/vulnerability/{id}"),
            references: vec![],
            published: None,
        });
        attrs.packages = idxs
            .iter()
            .map(|&i| format!("{}@{}", keys[i].1, keys[i].2))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        if attrs.references.is_empty() {
            attrs
                .references
                .push(format!("https://osv.dev/vulnerability/{id}"));
        }
        let fid = format!("finding:osv:{id}");
        batch.entities.push(EntityInput {
            id: fid.clone(),
            kind: EntityKind::Finding,
            name: format!(
                "{id}: {}",
                attrs.summary.chars().take(80).collect::<String>()
            ),
            path: None,
            version: None,
            source: "osv.dev".into(),
            evidence: vec![EvidenceInput::new(
                "osv_match",
                format!("OSV lists {} as affected", attrs.packages.join(", ")),
                format!("{OSV_VULN_URL}{id}"),
                Confidence::Strong,
            )],
            attributes: serde_json::to_value(&attrs).unwrap_or_default(),
        });
        for &i in idxs {
            for entity in &uniq[&keys[i]] {
                batch.relationships.push(RelationshipInput {
                    from_id: entity.clone(),
                    to_id: fid.clone(),
                    kind: RelationKind::HasFinding,
                    confidence: Confidence::Strong,
                    attributes: serde_json::json!({ "installed_version": keys[i].2 }),
                    evidence: vec![],
                });
            }
        }
    }
    Ok((sent, ids.len() as u32))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agasthy_platform::{CommandOutput, MockCommandRunner};

    #[test]
    fn parses_osv_records() {
        let v: serde_json::Value = serde_json::from_str(r#"{"id":"GHSA-xxxx","aliases":["CVE-2024-1"],"summary":"Prototype pollution","database_specific":{"severity":"MODERATE"},
          "affected":[{"package":{"name":"lodash","ecosystem":"npm"},"ranges":[{"type":"SEMVER","events":[{"introduced":"0"},{"fixed":"4.17.21"}]}]}],
          "references":[{"url":"https://example.invalid/a"}]}"#).unwrap();
        let f = parse_vuln(&v, "lodash").unwrap();
        assert_eq!(f.severity, Severity::Medium);
        assert_eq!(f.fixed_versions, vec!["4.17.21"]);
        assert!(f.remediation.contains("4.17.21"));
        assert_eq!(f.aliases, vec!["CVE-2024-1"]);
    }

    #[test]
    fn disabled_network_sends_nothing() {
        let d = tempfile::tempdir().unwrap();
        let m = MockCommandRunner::new();
        let input = SecurityInput {
            osv_enabled: false,
            queries: vec![OsvQuery {
                entity_id: "package:npm:lodash".into(),
                ecosystem: "npm".into(),
                name: "lodash".into(),
                version: "4.17.0".into(),
            }],
            unknown_origin: vec![("binary:/x/tool".into(), "tool".into(), "/x/tool".into())],
            work_dir: d.path(),
        };
        let (b, r) = scan(&input, &m);
        assert_eq!(r.sent_to_osv, 0);
        assert_eq!(r.eligible_for_osv, 1);
        assert!(
            !m.calls().iter().any(|c| c.program == "curl"),
            "no network when disabled"
        );
        assert_eq!(r.unknown_origin, 1);
        assert!(
            b.entities
                .iter()
                .any(|e| e.id == "finding:provenance:binary:/x/tool")
        );
    }

    #[test]
    fn enabled_queries_osv_and_links_findings() {
        let d = tempfile::tempdir().unwrap();
        let m = MockCommandRunner::new();
        let qargs = query_args(&d.path().join("osv-query.json"));
        let q: Vec<&str> = qargs.iter().map(String::as_str).collect();
        m.on(
            "curl",
            &q, // Results come back in query order: (crates.io, serde) sorts before (npm, lodash).
            Ok(CommandOutput::ok(
                r#"{"results":[{},{"vulns":[{"id":"GHSA-1"}]}]}"#,
            )),
        );
        let dargs = detail_args("GHSA-1");
        let dq: Vec<&str> = dargs.iter().map(String::as_str).collect();
        m.on("curl", &dq, Ok(CommandOutput::ok(r#"{"id":"GHSA-1","summary":"bad","database_specific":{"severity":"HIGH"},"affected":[{"package":{"name":"lodash"},"ranges":[{"events":[{"fixed":"4.17.21"}]}]}]}"#)));
        m.on("brew", &["outdated", "--json=v2"], Ok(CommandOutput::ok(r#"{"formulae":[{"name":"jq","installed_versions":["1.7.1"],"current_version":"1.8.0","pinned":false}],"casks":[]}"#)));
        let input = SecurityInput {
            osv_enabled: true,
            queries: vec![
                OsvQuery {
                    entity_id: "package:npm:lodash".into(),
                    ecosystem: "npm".into(),
                    name: "lodash".into(),
                    version: "4.17.0".into(),
                },
                OsvQuery {
                    entity_id: "dependency:npm:lodash".into(),
                    ecosystem: "npm".into(),
                    name: "lodash".into(),
                    version: "4.17.0".into(),
                },
                OsvQuery {
                    entity_id: "dependency:cargo:serde".into(),
                    ecosystem: "crates.io".into(),
                    name: "serde".into(),
                    version: "1.0.0".into(),
                },
            ],
            unknown_origin: vec![],
            work_dir: d.path(),
        };
        let (b, r) = scan(&input, &m);
        assert_eq!(r.sent_to_osv, 2, "duplicates are sent once");
        assert_eq!(r.vulnerabilities, 1);
        assert_eq!(r.outdated, 1);
        let f = b
            .entities
            .iter()
            .find(|e| e.id == "finding:osv:GHSA-1")
            .unwrap();
        assert_eq!(f.attributes["severity"], "high");
        let links = b
            .relationships
            .iter()
            .filter(|x| x.to_id == "finding:osv:GHSA-1")
            .count();
        assert_eq!(links, 2, "both entities with lodash@4.17.0 are linked");
        assert!(
            !d.path().join("osv-query.json").exists(),
            "request body is removed"
        );
        // What was sent contains only names, ecosystems and versions.
        let sent = m.calls().into_iter().find(|c| c.program == "curl").unwrap();
        assert!(sent.args.iter().any(|a| a == OSV_QUERY_URL));
    }
}
