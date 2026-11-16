//! Docker disk usage and the two prune operations AGASTHY offers.
//! Volumes are reported but never removed.

use std::time::Duration;

use agasthy_core::Availability;
use agasthy_platform::{CommandError, CommandRunner, CommandSpec};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DockerUsageRow {
    /// `Images`, `Containers`, `Local Volumes`, `Build Cache`.
    pub kind: String,
    pub total: u32,
    pub active: u32,
    pub size: String,
    pub reclaimable: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DockerUsage {
    pub rows: Vec<DockerUsageRow>,
    pub dangling_images: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DockerPrune {
    /// `docker builder prune --force`: the build cache only.
    BuildCache,
    /// `docker image prune --force`: dangling (untagged) images only.
    DanglingImages,
}

impl DockerPrune {
    pub fn args(&self) -> &'static [&'static str] {
        match self {
            DockerPrune::BuildCache => &["builder", "prune", "--force"],
            DockerPrune::DanglingImages => &["image", "prune", "--force"],
        }
    }
}

fn not_running(e: &str) -> bool {
    let e = e.to_ascii_lowercase();
    e.contains("cannot connect to the docker daemon")
        || e.contains("is the docker daemon running")
        || e.contains("error during connect")
}

pub fn usage(runner: &dyn CommandRunner) -> Availability<DockerUsage> {
    let spec = CommandSpec::new("docker", &["system", "df", "--format", "{{json .}}"])
        .timeout(Duration::from_secs(20));
    let out = match runner.run(&spec) {
        Ok(o) => o,
        Err(CommandError::NotFound { .. }) => {
            return Availability::unavailable("Docker is not installed");
        }
        Err(e) => return Availability::unavailable(e.to_string()),
    };
    if !out.success() {
        return Availability::unavailable(if not_running(&out.stderr) {
            "Docker is installed but its daemon is not running".to_string()
        } else {
            format!(
                "docker system df failed: {}",
                out.stderr.lines().next().unwrap_or("")
            )
        });
    }
    let rows = parse_df(&out.stdout);
    let dangling = runner
        .run(
            &CommandSpec::new(
                "docker",
                &["images", "--filter", "dangling=true", "--quiet"],
            )
            .timeout(Duration::from_secs(20)),
        )
        .ok()
        .filter(|o| o.success())
        .map(|o| o.stdout.lines().filter(|l| !l.trim().is_empty()).count() as u32)
        .unwrap_or(0);
    Availability::available(DockerUsage {
        rows,
        dangling_images: dangling,
    })
}

pub fn parse_df(stdout: &str) -> Vec<DockerUsageRow> {
    stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l.trim()).ok())
        .map(|v| {
            let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
            let n = |k: &str| {
                v.get(k)
                    .and_then(|x| {
                        x.as_str()
                            .and_then(|s| s.parse().ok())
                            .or_else(|| x.as_u64().map(|u| u as u32))
                    })
                    .unwrap_or(0)
            };
            DockerUsageRow {
                kind: s("Type"),
                total: n("TotalCount"),
                active: n("Active"),
                size: s("Size"),
                reclaimable: s("Reclaimable"),
            }
        })
        .collect()
}

/// Run one of the allowed prune commands. Returns docker's summary line.
pub fn prune(runner: &dyn CommandRunner, what: DockerPrune) -> Result<String, String> {
    let out = runner
        .run(&CommandSpec::new("docker", what.args()).timeout(Duration::from_secs(600)))
        .map_err(|e| e.to_string())?;
    if !out.success() {
        return Err(out
            .stderr
            .lines()
            .next()
            .unwrap_or("docker failed")
            .to_string());
    }
    Ok(out
        .stdout
        .lines()
        .rev()
        .find(|l| l.to_ascii_lowercase().contains("reclaimed"))
        .unwrap_or("done")
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agasthy_platform::{CommandOutput, MockCommandRunner};

    #[test]
    fn parses_df_and_reports_daemon_state() {
        let df = r#"{"Active":"2","Reclaimable":"1.2GB (60%)","Size":"2GB","TotalCount":"5","Type":"Images"}
{"Active":"0","Reclaimable":"500MB","Size":"500MB","TotalCount":"12","Type":"Build Cache"}"#;
        let rows = parse_df(df);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].kind, "Build Cache");
        assert_eq!(rows[0].total, 5);

        let m = MockCommandRunner::new();
        m.on(
            "docker",
            &["system", "df", "--format", "{{json .}}"],
            Ok(CommandOutput::failed(
                1,
                "Cannot connect to the Docker daemon at unix:///x. Is the docker daemon running?",
            )),
        );
        match usage(&m) {
            Availability::Unavailable { reason } => assert!(reason.contains("not running")),
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            usage(&MockCommandRunner::new()),
            Availability::Unavailable { .. }
        ));
    }

    #[test]
    fn prune_commands_never_touch_volumes() {
        for p in [DockerPrune::BuildCache, DockerPrune::DanglingImages] {
            let a = p.args().join(" ");
            assert!(
                !a.contains("volume")
                    && !a.contains("system prune")
                    && !a.contains("--all")
                    && !a.contains("-a ")
            );
        }
    }
}
