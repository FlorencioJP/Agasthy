//! What AGASTHY knows about AI developer tools, providers and their files.
//!
//! Detection is by evidence on disk (executables, application bundles, editor
//! extensions, configuration directories). A tool is never reported because
//! its name appears in a document.

#[derive(Debug, Clone, Copy)]
pub struct AiToolSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub vendor: &'static str,
    /// `cli`, `desktop_app`, `ide`, `ide_extension`, `local_inference`.
    pub category: &'static str,
    pub binaries: &'static [&'static str],
    /// Application bundle file names (macOS) …
    pub app_names: &'static [&'static str],
    /// … or bundle identifiers, which survive renames such as "Cursor 2.app".
    pub bundle_ids: &'static [&'static str],
    /// VS Code-style extension ids (`publisher.name`).
    pub extensions: &'static [&'static str],
    /// Configuration directories or files relative to the home directory.
    pub config_paths: &'static [&'static str],
    /// Provider the tool is built on, when it is tied to one.
    pub provider: Option<&'static str>,
    /// Local HTTP port for inference engines.
    pub local_port: Option<u16>,
}

const fn tool(
    id: &'static str,
    name: &'static str,
    vendor: &'static str,
    category: &'static str,
) -> AiToolSpec {
    AiToolSpec {
        id,
        name,
        vendor,
        category,
        binaries: &[],
        app_names: &[],
        bundle_ids: &[],
        extensions: &[],
        config_paths: &[],
        provider: None,
        local_port: None,
    }
}

pub const TOOLS: &[AiToolSpec] = &[
    AiToolSpec {
        binaries: &["claude"],
        extensions: &["anthropic.claude-code"],
        config_paths: &[".claude"],
        provider: Some("anthropic"),
        ..tool("claude-code", "Claude Code", "Anthropic", "cli")
    },
    AiToolSpec {
        app_names: &["Claude.app"],
        bundle_ids: &["com.anthropic.claudefordesktop"],
        config_paths: &["Library/Application Support/Claude"],
        provider: Some("anthropic"),
        ..tool(
            "claude-desktop",
            "Claude (desktop app)",
            "Anthropic",
            "desktop_app",
        )
    },
    AiToolSpec {
        binaries: &["codex"],
        extensions: &["openai.chatgpt"],
        config_paths: &[".codex"],
        provider: Some("openai"),
        ..tool("codex-cli", "OpenAI Codex CLI", "OpenAI", "cli")
    },
    AiToolSpec {
        app_names: &["Codex.app"],
        bundle_ids: &["com.openai.codex"],
        provider: Some("openai"),
        ..tool("codex-app", "Codex (desktop app)", "OpenAI", "desktop_app")
    },
    AiToolSpec {
        app_names: &["ChatGPT.app"],
        bundle_ids: &["com.openai.chat"],
        provider: Some("openai"),
        ..tool(
            "chatgpt-desktop",
            "ChatGPT (desktop app)",
            "OpenAI",
            "desktop_app",
        )
    },
    AiToolSpec {
        binaries: &["cursor", "cursor-agent"],
        app_names: &["Cursor.app"],
        bundle_ids: &["com.todesktop.230313mzl4w4u92"],
        config_paths: &[".cursor"],
        ..tool("cursor", "Cursor", "Anysphere", "ide")
    },
    AiToolSpec {
        extensions: &["github.copilot", "github.copilot-chat"],
        config_paths: &[".config/github-copilot"],
        provider: Some("github-copilot"),
        ..tool(
            "github-copilot",
            "GitHub Copilot",
            "GitHub",
            "ide_extension",
        )
    },
    AiToolSpec {
        app_names: &["Copilot.app"],
        bundle_ids: &["com.microsoft.copilot-mac"],
        ..tool(
            "microsoft-copilot",
            "Microsoft Copilot (desktop app)",
            "Microsoft",
            "desktop_app",
        )
    },
    AiToolSpec {
        binaries: &["gemini"],
        extensions: &["google.geminicodeassist"],
        config_paths: &[".gemini"],
        provider: Some("google"),
        ..tool("gemini-cli", "Gemini CLI", "Google", "cli")
    },
    AiToolSpec {
        app_names: &["Gemini.app"],
        provider: Some("google"),
        ..tool(
            "gemini-desktop",
            "Gemini (desktop app)",
            "Google",
            "desktop_app",
        )
    },
    AiToolSpec {
        binaries: &["windsurf"],
        app_names: &["Windsurf.app"],
        bundle_ids: &["com.exafunction.windsurf"],
        config_paths: &[".codeium/windsurf", ".windsurf"],
        ..tool("windsurf", "Windsurf", "Windsurf", "ide")
    },
    AiToolSpec {
        extensions: &["saoudrizwan.claude-dev"],
        ..tool("cline", "Cline", "Cline", "ide_extension")
    },
    AiToolSpec {
        extensions: &["rooveterinaryinc.roo-cline"],
        ..tool("roo-code", "Roo Code", "Roo Code", "ide_extension")
    },
    AiToolSpec {
        extensions: &["continue.continue"],
        config_paths: &[".continue"],
        ..tool("continue", "Continue", "Continue", "ide_extension")
    },
    AiToolSpec {
        binaries: &["aider"],
        config_paths: &[".aider.conf.yml"],
        ..tool("aider", "Aider", "Aider", "cli")
    },
    AiToolSpec {
        binaries: &["openclaw"],
        config_paths: &[".openclaw"],
        ..tool("openclaw", "OpenClaw", "OpenClaw", "cli")
    },
    AiToolSpec {
        binaries: &["ollama"],
        app_names: &["Ollama.app"],
        bundle_ids: &["com.electron.ollama"],
        config_paths: &[".ollama"],
        provider: Some("ollama"),
        local_port: Some(11434),
        ..tool("ollama", "Ollama", "Ollama", "local_inference")
    },
    AiToolSpec {
        binaries: &["lms"],
        app_names: &["LM Studio.app"],
        bundle_ids: &["ai.elementlabs.lmstudio"],
        config_paths: &[".lmstudio", ".cache/lm-studio"],
        provider: Some("lmstudio"),
        local_port: Some(1234),
        ..tool("lm-studio", "LM Studio", "Element Labs", "local_inference")
    },
    AiToolSpec {
        app_names: &["Jan.app"],
        config_paths: &["Library/Application Support/Jan"],
        local_port: Some(1337),
        ..tool("jan", "Jan", "Jan", "local_inference")
    },
    AiToolSpec {
        binaries: &["vllm"],
        provider: Some("vllm"),
        local_port: Some(8000),
        ..tool("vllm", "vLLM", "vLLM", "local_inference")
    },
    AiToolSpec {
        binaries: &["local-ai"],
        provider: Some("localai"),
        local_port: Some(8080),
        ..tool("localai", "LocalAI", "LocalAI", "local_inference")
    },
];

pub fn tool_spec(id: &str) -> Option<&'static AiToolSpec> {
    TOOLS.iter().find(|t| t.id == id)
}

#[derive(Debug, Clone, Copy)]
pub struct ProviderSpec {
    pub id: &'static str,
    pub name: &'static str,
    /// `cloud` or `local`.
    pub kind: &'static str,
    /// Environment variable names that hold credentials (only presence is checked).
    pub credential_env: &'static [&'static str],
    /// Environment variable names that point at an endpoint.
    pub endpoint_env: &'static [&'static str],
    /// Files whose existence (never contents) indicates a stored credential.
    pub credential_files: &'static [&'static str],
}

pub const PROVIDERS: &[ProviderSpec] = &[
    ProviderSpec {
        id: "openai",
        name: "OpenAI",
        kind: "cloud",
        credential_env: &["OPENAI_API_KEY"],
        endpoint_env: &["OPENAI_BASE_URL", "OPENAI_API_BASE"],
        credential_files: &[".codex/auth.json"],
    },
    ProviderSpec {
        id: "anthropic",
        name: "Anthropic",
        kind: "cloud",
        credential_env: &["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"],
        endpoint_env: &["ANTHROPIC_BASE_URL"],
        credential_files: &[".claude/.credentials.json"],
    },
    ProviderSpec {
        id: "google",
        name: "Google Gemini",
        kind: "cloud",
        credential_env: &[
            "GEMINI_API_KEY",
            "GOOGLE_API_KEY",
            "GOOGLE_APPLICATION_CREDENTIALS",
        ],
        endpoint_env: &["GOOGLE_GENAI_USE_VERTEXAI", "GOOGLE_CLOUD_PROJECT"],
        credential_files: &[".gemini/oauth_creds.json"],
    },
    ProviderSpec {
        id: "openrouter",
        name: "OpenRouter",
        kind: "cloud",
        credential_env: &["OPENROUTER_API_KEY"],
        endpoint_env: &[],
        credential_files: &[],
    },
    ProviderSpec {
        id: "azure-openai",
        name: "Azure OpenAI",
        kind: "cloud",
        credential_env: &["AZURE_OPENAI_API_KEY"],
        endpoint_env: &["AZURE_OPENAI_ENDPOINT"],
        credential_files: &[],
    },
    ProviderSpec {
        id: "huggingface",
        name: "Hugging Face",
        kind: "cloud",
        credential_env: &["HF_TOKEN", "HUGGING_FACE_HUB_TOKEN"],
        endpoint_env: &["HF_ENDPOINT"],
        credential_files: &[".cache/huggingface/token"],
    },
    ProviderSpec {
        id: "mistral",
        name: "Mistral",
        kind: "cloud",
        credential_env: &["MISTRAL_API_KEY"],
        endpoint_env: &[],
        credential_files: &[],
    },
    ProviderSpec {
        id: "groq",
        name: "Groq",
        kind: "cloud",
        credential_env: &["GROQ_API_KEY"],
        endpoint_env: &[],
        credential_files: &[],
    },
    ProviderSpec {
        id: "deepseek",
        name: "DeepSeek",
        kind: "cloud",
        credential_env: &["DEEPSEEK_API_KEY"],
        endpoint_env: &[],
        credential_files: &[],
    },
    ProviderSpec {
        id: "xai",
        name: "xAI",
        kind: "cloud",
        credential_env: &["XAI_API_KEY"],
        endpoint_env: &[],
        credential_files: &[],
    },
    ProviderSpec {
        id: "github-copilot",
        name: "GitHub Copilot",
        kind: "cloud",
        credential_env: &[],
        endpoint_env: &[],
        credential_files: &[
            ".config/github-copilot/apps.json",
            ".config/github-copilot/hosts.json",
        ],
    },
    ProviderSpec {
        id: "ollama",
        name: "Ollama (local)",
        kind: "local",
        credential_env: &[],
        endpoint_env: &["OLLAMA_HOST"],
        credential_files: &[],
    },
    ProviderSpec {
        id: "lmstudio",
        name: "LM Studio (local)",
        kind: "local",
        credential_env: &[],
        endpoint_env: &[],
        credential_files: &[],
    },
    ProviderSpec {
        id: "vllm",
        name: "vLLM (local)",
        kind: "local",
        credential_env: &[],
        endpoint_env: &[],
        credential_files: &[],
    },
    ProviderSpec {
        id: "localai",
        name: "LocalAI (local)",
        kind: "local",
        credential_env: &[],
        endpoint_env: &[],
        credential_files: &[],
    },
];

pub fn provider_spec(id: &str) -> Option<&'static ProviderSpec> {
    PROVIDERS.iter().find(|p| p.id == id)
}

/// Map a provider name used inside a tool's configuration to a provider id.
pub fn provider_from_name(name: &str) -> Option<&'static str> {
    let n = name.to_ascii_lowercase();
    Some(match n.as_str() {
        x if x.contains("openrouter") => "openrouter",
        x if x.contains("azure") => "azure-openai",
        x if x.contains("openai") => "openai",
        x if x.contains("anthropic") || x.contains("claude") => "anthropic",
        x if x.contains("gemini") || x.contains("google") || x.contains("vertex") => "google",
        x if x.contains("ollama") => "ollama",
        x if x.contains("lmstudio") || x.contains("lm-studio") || x.contains("lm_studio") => {
            "lmstudio"
        }
        x if x.contains("huggingface") || x == "hf" => "huggingface",
        x if x.contains("mistral") => "mistral",
        x if x.contains("groq") => "groq",
        x if x.contains("deepseek") => "deepseek",
        x if x.contains("xai") || x.contains("grok") => "xai",
        _ => return None,
    })
}

/// What an AI configuration file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    Instructions,
    Rules,
    Agent,
    Skill,
    Command,
    McpConfig,
    Settings,
}

impl FileType {
    pub fn as_str(&self) -> &'static str {
        match self {
            FileType::Instructions => "instructions",
            FileType::Rules => "rules",
            FileType::Agent => "agent",
            FileType::Skill => "skill",
            FileType::Command => "command",
            FileType::McpConfig => "mcp_config",
            FileType::Settings => "settings",
        }
    }

    /// Whether the text is prose instructions worth indexing (vs. configuration
    /// that may hold secrets and is never stored).
    pub fn indexable(&self) -> bool {
        matches!(
            self,
            FileType::Instructions
                | FileType::Rules
                | FileType::Agent
                | FileType::Skill
                | FileType::Command
        )
    }
}

/// Classify a file by its path relative to its root (a repository or the home
/// directory). Returns the type and the tools that read it.
pub fn classify(rel: &str) -> Option<(FileType, &'static [&'static str])> {
    let rel = rel.replace('\\', "/");
    let name = rel.rsplit('/').next().unwrap_or(&rel);
    let in_dir = |d: &str| rel.starts_with(&format!("{d}/")) || rel.contains(&format!("/{d}/"));
    use FileType::*;
    const AGENTS_MD_READERS: &[&str] = &[
        "codex-cli",
        "codex-app",
        "cursor",
        "github-copilot",
        "windsurf",
        "gemini-cli",
        "aider",
        "openclaw",
    ];
    Some(match name {
        "AGENTS.md" | "AGENTS.override.md" => (Instructions, AGENTS_MD_READERS),
        "CLAUDE.md" | "CLAUDE.local.md" => (Instructions, &["claude-code"]),
        "GEMINI.md" => (Instructions, &["gemini-cli"]),
        "CONVENTIONS.md" => (Instructions, &["aider"]),
        ".cursorrules" => (Rules, &["cursor"]),
        ".windsurfrules" | "global_rules.md" if name == ".windsurfrules" || in_dir("windsurf") => {
            (Rules, &["windsurf"])
        }
        ".clinerules" => (Rules, &["cline"]),
        ".roorules" => (Rules, &["roo-code"]),
        "copilot-instructions.md" if in_dir(".github") => (Instructions, &["github-copilot"]),
        ".mcp.json" => (McpConfig, &["claude-code"]),
        "claude_desktop_config.json" => (McpConfig, &["claude-desktop"]),
        ".aider.conf.yml" => (Settings, &["aider"]),
        "SKILL.md" if in_dir(".claude") => (Skill, &["claude-code"]),
        "SKILL.md" if in_dir(".codex") => (Skill, &["codex-cli", "codex-app"]),
        "SKILL.md" if in_dir(".cursor") => (Skill, &["cursor"]),
        "mcp.json" if in_dir(".cursor") => (McpConfig, &["cursor"]),
        "mcp.json" if in_dir(".vscode") || in_dir("Code/User") => (McpConfig, &["github-copilot"]),
        "mcp_config.json" if in_dir("windsurf") => (McpConfig, &["windsurf"]),
        "settings.json" | "settings.local.json" if in_dir(".claude") => {
            (Settings, &["claude-code"])
        }
        "settings.json" if in_dir(".gemini") => (Settings, &["gemini-cli"]),
        "config.toml" if in_dir(".codex") => (Settings, &["codex-cli", "codex-app"]),
        "config.yaml" | "config.json" if in_dir(".continue") => (Settings, &["continue"]),
        _ if name.ends_with(".md") && in_dir(".claude/agents") => (Agent, &["claude-code"]),
        _ if name.ends_with(".md") && in_dir(".claude/commands") => (Command, &["claude-code"]),
        _ if name.ends_with(".md") && in_dir(".codex/prompts") => {
            (Command, &["codex-cli", "codex-app"])
        }
        _ if name.ends_with(".toml") && in_dir(".gemini/commands") => (Command, &["gemini-cli"]),
        _ if (name.ends_with(".mdc") || name.ends_with(".md")) && in_dir(".cursor/rules") => {
            (Rules, &["cursor"])
        }
        _ if name.ends_with(".md") && in_dir(".clinerules") => (Rules, &["cline"]),
        _ if name.ends_with(".md") && in_dir(".roo/rules") => (Rules, &["roo-code"]),
        _ if name.ends_with(".md") && in_dir(".windsurf/rules") => (Rules, &["windsurf"]),
        _ if name.ends_with(".instructions.md") && in_dir(".github/instructions") => {
            (Rules, &["github-copilot"])
        }
        _ if name.ends_with(".prompt.md") && in_dir(".github/prompts") => {
            (Command, &["github-copilot"])
        }
        _ if name.ends_with(".agent.md") || (name.ends_with(".md") && in_dir(".github/agents")) => {
            (Agent, &["github-copilot"])
        }
        _ => return None,
    })
}

/// Hidden directories a project-level walk may enter to find AI files.
pub const AI_DIRS: &[&str] = &[
    ".claude",
    ".cursor",
    ".github",
    ".gemini",
    ".codex",
    ".roo",
    ".clinerules",
    ".windsurf",
    ".vscode",
    ".continue",
];

/// Files and directories under the home directory that hold global AI configuration.
pub const GLOBAL_LOCATIONS: &[&str] = &[
    ".claude/CLAUDE.md",
    ".claude/settings.json",
    ".claude/agents",
    ".claude/commands",
    ".claude/skills",
    ".codex/AGENTS.md",
    ".codex/AGENTS.override.md",
    ".codex/config.toml",
    ".codex/prompts",
    ".codex/skills",
    ".gemini/GEMINI.md",
    ".gemini/settings.json",
    ".gemini/commands",
    ".cursor/mcp.json",
    ".cursor/rules",
    ".codeium/windsurf/mcp_config.json",
    ".codeium/windsurf/memories/global_rules.md",
    ".continue/config.yaml",
    ".continue/config.json",
    ".aider.conf.yml",
    "Library/Application Support/Claude/claude_desktop_config.json",
    "Library/Application Support/Code/User/mcp.json",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification() {
        assert_eq!(classify("CLAUDE.md").unwrap().0, FileType::Instructions);
        let agents_readers = classify("packages/api/AGENTS.md").unwrap().1;
        assert!(agents_readers.contains(&"codex-app") && agents_readers.contains(&"cursor"));
        assert_eq!(
            classify(".cursor/rules/style.mdc").unwrap(),
            (FileType::Rules, &["cursor"][..])
        );
        assert_eq!(
            classify(".claude/agents/reviewer.md").unwrap().0,
            FileType::Agent
        );
        assert_eq!(
            classify(".claude/skills/pdf/SKILL.md").unwrap().0,
            FileType::Skill
        );
        assert_eq!(
            classify(".github/copilot-instructions.md").unwrap().1,
            &["github-copilot"]
        );
        assert_eq!(
            classify(".github/instructions/ts.instructions.md")
                .unwrap()
                .0,
            FileType::Rules
        );
        assert_eq!(classify(".mcp.json").unwrap().0, FileType::McpConfig);
        assert_eq!(classify(".vscode/mcp.json").unwrap().1, &["github-copilot"]);
        assert_eq!(
            classify(".codex/config.toml").unwrap().0,
            FileType::Settings
        );
        assert!(classify("README.md").is_none());
        assert!(classify("docs/notes.md").is_none());
        assert!(
            classify("copilot-instructions.md").is_none(),
            "only under .github"
        );
        assert!(!FileType::McpConfig.indexable());
        assert!(FileType::Agent.indexable());
    }

    #[test]
    fn provider_names() {
        assert_eq!(provider_from_name("OpenAI"), Some("openai"));
        assert_eq!(provider_from_name("azure"), Some("azure-openai"));
        assert_eq!(provider_from_name("openrouter"), Some("openrouter"));
        assert_eq!(provider_from_name("ollama"), Some("ollama"));
        assert_eq!(provider_from_name("my-proxy"), None);
        for t in TOOLS {
            if let Some(p) = t.provider {
                assert!(
                    provider_spec(p).is_some(),
                    "{} names unknown provider {p}",
                    t.id
                );
            }
        }
    }
}
