//! The tools the Operator may use. All read-only, except `propose_action`,
//! which only produces a reviewed plan for the user; it cannot execute.

use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, Serialize, TS)]
#[ts(export)]
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    /// JSON Schema for the arguments.
    #[ts(type = "string")]
    pub parameters: &'static str,
}

pub const TOOLS: &[ToolSpec] = &[
    ToolSpec {
        name: "machine_overview",
        description: "Counts of discovered items, live CPU/memory/disk figures, and when each scan last ran.",
        parameters: r#"{"type":"object","properties":{}}"#,
    },
    ToolSpec {
        name: "top_processes",
        description: "Processes using the most memory or CPU, with PID, project and whether AGASTHY may stop them.",
        parameters: r#"{"type":"object","properties":{"by":{"type":"string","enum":["memory","cpu"]},"limit":{"type":"integer","minimum":1,"maximum":25}},"required":["by"]}"#,
    },
    ToolSpec {
        name: "listening_ports",
        description: "Processes listening on TCP ports (current user's processes). Optionally filter to one port.",
        parameters: r#"{"type":"object","properties":{"port":{"type":"integer"}}}"#,
    },
    ToolSpec {
        name: "search",
        description: "Search discovered items by text. Kinds: repository, application, binary, runtime, package, package_manager, dependency, ai_tool, ai_provider, mcp_server, ai_config_file, local_model, finding.",
        parameters: r#"{"type":"object","properties":{"text":{"type":"string"},"kind":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":50}},"required":["text"]}"#,
    },
    ToolSpec {
        name: "entity_details",
        description: "Facts, evidence and relationships for one item by id (ids come from search results).",
        parameters: r#"{"type":"object","properties":{"id":{"type":"string"}},"required":["id"]}"#,
    },
    ToolSpec {
        name: "repositories",
        description: "Scanned Git repositories with last commit time, uncommitted changes and size. stale_days filters to repositories without commits for that many days.",
        parameters: r#"{"type":"object","properties":{"stale_days":{"type":"integer","minimum":1}}}"#,
    },
    ToolSpec {
        name: "disk_cleanup_candidates",
        description: "Regenerable folders and caches that could be cleaned, with sizes and whether each passed AGASTHY's safety checks. refresh=true measures again (slow).",
        parameters: r#"{"type":"object","properties":{"refresh":{"type":"boolean"}}}"#,
    },
    ToolSpec {
        name: "removal_impact",
        description: "Simulate removing a package, runtime, dependency, tool or app: what depends on it and which projects would break.",
        parameters: r#"{"type":"object","properties":{"id":{"type":"string"}},"required":["id"]}"#,
    },
    ToolSpec {
        name: "ai_ecosystem",
        description: "Installed AI tools, providers (and evidence level), MCP servers, local models and AI instruction files (names and locations only).",
        parameters: r#"{"type":"object","properties":{}}"#,
    },
    ToolSpec {
        name: "security_findings",
        description: "Outdated packages, unknown-origin tools and (if enabled) OSV advisories.",
        parameters: r#"{"type":"object","properties":{}}"#,
    },
    ToolSpec {
        name: "docker_status",
        description: "Docker disk usage (images, containers, volumes, build cache) and whether the daemon is running.",
        parameters: r#"{"type":"object","properties":{}}"#,
    },
    ToolSpec {
        name: "propose_action",
        description: "Propose a change for the user to review. Does NOT execute it. request is one of: {\"type\":\"terminate_process\",\"pid\":N,\"start_time\":N,\"force\":false}, {\"type\":\"quarantine_paths\",\"paths\":[...]} (paths must come from disk_cleanup_candidates with eligible=true), {\"type\":\"docker_prune\",\"target\":\"build_cache\"|\"dangling_images\"}.",
        parameters: r#"{"type":"object","properties":{"request":{"type":"object"},"reason":{"type":"string"}},"required":["request","reason"]}"#,
    },
];

/// Implemented by the application: executes a tool against real data.
pub trait ToolHost: Send + Sync {
    fn call(&self, name: &str, args: &serde_json::Value) -> Result<serde_json::Value, String>;
}

/// A one-line description of a tool result for the visible trace.
pub fn summarize(name: &str, v: &serde_json::Value) -> String {
    let len = |k: &str| {
        v.get(k)
            .and_then(|x| x.as_array())
            .map(|a| a.len())
            .unwrap_or(0)
    };
    match name {
        "top_processes" => format!("{} processes", len("processes")),
        "listening_ports" => format!("{} listening processes", len("processes")),
        "search" => format!("{} matches", len("results")),
        "repositories" => format!("{} repositories", len("repositories")),
        "disk_cleanup_candidates" => format!("{} candidates", len("candidates")),
        "ai_ecosystem" => format!(
            "{} tools, {} MCP servers, {} files",
            len("tools"),
            len("mcp_servers"),
            len("files")
        ),
        "security_findings" => format!("{} findings", len("findings")),
        "propose_action" => v
            .get("title")
            .and_then(|t| t.as_str())
            .unwrap_or("proposal")
            .to_string(),
        "removal_impact" => v
            .get("summary")
            .and_then(|t| t.as_str())
            .unwrap_or("impact")
            .to_string(),
        _ => "done".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schemas_are_valid_json_and_names_unique() {
        let mut names = std::collections::BTreeSet::new();
        for t in TOOLS {
            let v: serde_json::Value =
                serde_json::from_str(t.parameters).unwrap_or_else(|e| panic!("{}: {e}", t.name));
            assert_eq!(v["type"], "object");
            assert!(names.insert(t.name));
        }
        // No tool can run commands or touch files.
        for forbidden in ["shell", "exec", "run_command", "write_file", "delete"] {
            assert!(!names.iter().any(|n| n.contains(forbidden)));
        }
    }
}
