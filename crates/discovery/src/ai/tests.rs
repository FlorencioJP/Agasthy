//! End-to-end AI discovery against a synthetic home directory and project.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use agasthy_core::{EntityKind, RelationKind};
use agasthy_platform::{
    AppInventory, AppScan, GpuInfo, PackageManagerSupport, Platform, PlatformInfo, ProbePolicy,
    SearchDir, SearchDirOrigin, SupportLevel, SystemPaths,
};

use super::*;

struct TestPlatform {
    bin: PathBuf,
}

impl SystemPaths for TestPlatform {
    fn executable_search_dirs(&self) -> Vec<SearchDir> {
        vec![SearchDir {
            path: self.bin.clone(),
            origin: SearchDirOrigin::ProcessPath,
        }]
    }
    fn executable_file_names(&self, tool: &str) -> Vec<String> {
        vec![tool.into()]
    }
    fn is_protected_scan_root(&self, _: &Path) -> bool {
        false
    }
}
impl AppInventory for TestPlatform {
    fn list_applications(&self) -> AppScan {
        AppScan {
            apps: Availability::available(vec![]),
            issues: vec![],
        }
    }
}
impl ProbePolicy for TestPlatform {
    fn may_probe_version(&self, _: &str, _: &Path) -> Result<(), String> {
        Ok(())
    }
}
impl Platform for TestPlatform {
    fn info(&self) -> PlatformInfo {
        PlatformInfo {
            os: "test".into(),
            os_version: None,
            arch: "test".into(),
            support_level: SupportLevel::Skeleton,
        }
    }
    fn supported_package_managers(&self) -> Vec<PackageManagerSupport> {
        vec![]
    }
    fn gpus(&self, _: &dyn agasthy_platform::CommandRunner) -> Availability<Vec<GpuInfo>> {
        Availability::unsupported("test")
    }
}

fn write(p: &Path, text: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

#[cfg(unix)]
#[test]
fn discovers_ai_ecosystem() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join("home");
    let bin = d.path().join("bin");
    let repo = d.path().join("code/app");
    std::fs::create_dir_all(&bin).unwrap();
    let claude = bin.join("claude");
    write(&claude, "#!/bin/false\n");
    std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();

    // Global configuration
    write(
        &home.join(".claude/CLAUDE.md"),
        "# Global\nAlways use pnpm.\n",
    );
    write(
        &home.join(".claude/settings.json"),
        r#"{"env":{"ANTHROPIC_BASE_URL":"https://proxy.example.invalid"}}"#,
    );
    write(
        &home.join(".claude.json"),
        r#"{"oauthAccount":{"emailAddress":"x"},"mcpServers":{"fs":{"command":"npx","args":["-y","@modelcontextprotocol/server-filesystem"]}},"projects":{"/p":{"mcpServers":{"gh":{"command":"gh-mcp","env":{"GITHUB_TOKEN":"ghp_NEVER_STORE_THIS_0123456789"}}}}}}"#,
    );
    write(
        &home.join(".codex/config.toml"),
        "model_provider = \"openrouter\"\n[model_providers.openrouter]\nbase_url = \"https://openrouter.ai/api/v1\"\n",
    );
    write(
        &home.join(".codex/auth.json"),
        r#"{"OPENAI_API_KEY":"sk-NEVER_READ_THIS"}"#,
    );
    write(
        &home.join(".ollama/models/manifests/registry.ollama.ai/library/qwen3/8b"),
        r#"{"layers":[{"size":42}]}"#,
    );

    // Project
    write(
        &repo.join("CLAUDE.md"),
        "# App\nSee @AGENTS.md\nUse npm for installs.\nOPENAI_API_KEY=sk-proj-abcdefghijklmnopqrstuvwxyz\n",
    );
    write(&repo.join("AGENTS.md"), "# Agents\nIndent with 2 spaces.\n");
    write(
        &repo.join(".cursorrules"),
        "# Agents\nIndent with 2 spaces.\n",
    );
    write(
        &repo.join(".mcp.json"),
        r#"{"mcpServers":{"fs":{"command":"npx","args":["-y","@modelcontextprotocol/server-filesystem"]}}}"#,
    );
    write(&repo.join(".claude/agents/reviewer.md"), "# Reviewer\n");

    let mut opts = AiScanOptions::new(
        home.clone(),
        vec![("repository:/code/app".into(), repo.clone())],
    );
    opts.env_names = BTreeSet::from(["OPENAI_API_KEY".to_string()]);
    opts.check_local_ports = false;
    let b = scan_ai(&TestPlatform { bin }, &opts, &crate::no_progress);

    let all = serde_json::to_string(&b.entities).unwrap()
        + &serde_json::to_string(&b.relationships).unwrap();
    assert!(
        !all.contains("NEVER_STORE") && !all.contains("NEVER_READ"),
        "secrets must not be stored"
    );
    assert!(!all.contains("sk-proj-abc"), "instruction text is redacted");
    assert!(
        !all.contains("emailAddress"),
        "unrelated ~/.claude.json content is not stored"
    );

    let entity = |id: &str| {
        b.entities
            .iter()
            .find(|e| e.id == id)
            .unwrap_or_else(|| panic!("missing {id}"))
    };
    let claude_tool = entity("ai_tool:claude-code");
    assert_eq!(claude_tool.attributes["installed"], true);
    let codex = entity("ai_tool:codex-cli");
    assert_eq!(
        codex.attributes["installed"], false,
        "config only: not claimed as installed"
    );
    assert!(
        b.entities.iter().all(|e| e.id != "ai_tool:cursor"),
        "nothing on disk for Cursor"
    );

    assert_eq!(
        entity("ai_provider:anthropic").attributes["highest"],
        "endpoint_configured"
    );
    assert_eq!(
        entity("ai_provider:openai").attributes["highest"],
        "credential_present"
    );
    assert_eq!(
        entity("ai_provider:openrouter").attributes["highest"],
        "endpoint_configured"
    );

    let mcp: Vec<_> = b
        .entities
        .iter()
        .filter(|e| e.kind == EntityKind::McpServer)
        .collect();
    assert_eq!(
        mcp.len(),
        3,
        "two in ~/.claude.json (one project-scoped) and one in .mcp.json"
    );
    let gh = mcp.iter().find(|e| e.name == "gh").unwrap();
    assert_eq!(gh.attributes["scope"], "/p");
    assert_eq!(
        gh.attributes["env_names"],
        serde_json::json!(["GITHUB_TOKEN"])
    );
    let fps: BTreeSet<_> = mcp
        .iter()
        .map(|e| e.attributes["fingerprint"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        fps.len(),
        2,
        "the filesystem server is the same server in two files"
    );

    let model = entity("local_model:ollama:qwen3:8b");
    assert_eq!(model.attributes["size_bytes"], 42);

    let file = |rel: &str| {
        b.entities
            .iter()
            .find(|e| {
                e.kind == EntityKind::AiConfigFile
                    && e.path.as_deref().is_some_and(|p| p.ends_with(rel))
            })
            .unwrap_or_else(|| panic!("missing file {rel}"))
    };
    let agents = file("app/AGENTS.md");
    assert_eq!(agents.attributes["scope"], "project");
    assert_eq!(
        agents.attributes["orphaned"], true,
        "no AGENTS.md reader installed"
    );
    assert_eq!(file("app/CLAUDE.md").attributes["orphaned"], false);
    assert_eq!(
        agents.attributes["duplicates"].as_array().unwrap().len(),
        1,
        "identical to .cursorrules"
    );
    assert!(
        file("app/CLAUDE.md").attributes["overlaps"]
            .as_array()
            .unwrap()
            .len()
            >= 2
    );
    assert_eq!(
        file(".claude/agents/reviewer.md").attributes["file_type"],
        "agent"
    );

    let has = |k: RelationKind| b.relationships.iter().filter(|r| r.kind == k).count();
    assert_eq!(has(RelationKind::SameContent), 1);
    assert_eq!(
        has(RelationKind::References),
        1,
        "CLAUDE.md imports AGENTS.md"
    );
    assert!(
        has(RelationKind::PotentialConflict) >= 1,
        "global says pnpm, project says npm"
    );
    let conflict = b
        .relationships
        .iter()
        .find(|r| r.kind == RelationKind::PotentialConflict)
        .unwrap();
    assert_eq!(conflict.attributes["topic"], "package_manager");
    assert_eq!(has(RelationKind::Contains), 5);
    assert!(has(RelationKind::UsesProvider) >= 1);
    assert!(has(RelationKind::DeclaresMcpServer) == 3);
}
