//! Reading MCP server declarations from tool configuration files.
//!
//! Only the server list is read. Environment variable *values* are never read
//! (only their names), and arguments are redacted.

use crate::ai::redact::redact_args;

#[derive(Debug, Clone, PartialEq)]
pub struct ServerDecl {
    pub name: String,
    pub transport: String,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub url: Option<String>,
    pub env_names: Vec<String>,
    pub disabled: bool,
    /// `global` or a project path for project-scoped entries in user config.
    pub scope: String,
}

impl ServerDecl {
    pub fn fingerprint(&self) -> String {
        match (&self.command, &self.url) {
            (Some(c), _) => format!("cmd:{} {}", base(c), self.args.join(" ")),
            (None, Some(u)) => format!("url:{u}"),
            _ => format!("name:{}", self.name),
        }
    }
}

fn base(cmd: &str) -> &str {
    cmd.rsplit(['/', '\\']).next().unwrap_or(cmd)
}

fn clean_url(u: &str) -> String {
    let (safe, _) = agasthy_provenance::redact_url(u);
    safe.split('?').next().unwrap_or(&safe).to_string()
}

fn from_json_entry(name: &str, v: &serde_json::Value, scope: &str) -> Option<ServerDecl> {
    let obj = v.as_object()?;
    let command = obj
        .get("command")
        .and_then(|c| c.as_str())
        .map(str::to_string);
    let args: Vec<String> = obj
        .get("args")
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let url = obj
        .get("url")
        .or_else(|| obj.get("serverUrl"))
        .or_else(|| obj.get("httpUrl"))
        .and_then(|u| u.as_str())
        .map(clean_url);
    let declared = obj
        .get("type")
        .or_else(|| obj.get("transport"))
        .and_then(|t| t.as_str());
    let transport = match (declared, &command, &url) {
        (Some(t), _, _) => match t {
            "stdio" => "stdio",
            "sse" => "sse",
            "http" | "streamable-http" | "streamableHttp" => "http",
            _ => "unknown",
        },
        (None, Some(_), _) => "stdio",
        (None, None, Some(_)) => "http",
        _ => "unknown",
    }
    .to_string();
    let mut env_names: Vec<String> = obj
        .get("env")
        .and_then(|e| e.as_object())
        .map(|e| e.keys().cloned().collect())
        .unwrap_or_default();
    env_names.sort();
    Some(ServerDecl {
        name: name.to_string(),
        transport,
        command,
        args: redact_args(&args),
        url,
        env_names,
        disabled: obj
            .get("disabled")
            .and_then(|d| d.as_bool())
            .unwrap_or(false)
            || obj.get("enabled").and_then(|d| d.as_bool()) == Some(false),
        scope: scope.to_string(),
    })
}

fn servers_in(obj: &serde_json::Value, key: &str, scope: &str, out: &mut Vec<ServerDecl>) {
    if let Some(map) = obj.get(key).and_then(|m| m.as_object()) {
        for (name, v) in map {
            if let Some(d) = from_json_entry(name, v, scope) {
                out.push(d);
            }
        }
    }
}

/// Parse the MCP servers declared in a configuration file's text.
pub fn parse(file_name: &str, text: &str) -> Result<Vec<ServerDecl>, String> {
    let mut out = Vec::new();
    if file_name.ends_with(".toml") {
        let t: toml::Table = text
            .parse()
            .map_err(|e: toml::de::Error| e.message().to_string())?;
        if let Some(servers) = t.get("mcp_servers").and_then(|s| s.as_table()) {
            for (name, v) in servers {
                let json = serde_json::to_value(v).map_err(|e| e.to_string())?;
                if let Some(d) = from_json_entry(name, &json, "global") {
                    out.push(d);
                }
            }
        }
        return Ok(out);
    }
    if file_name.ends_with(".yaml") || file_name.ends_with(".yml") {
        return Ok(out);
    }
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    servers_in(&v, "mcpServers", "global", &mut out);
    // VS Code's mcp.json
    servers_in(&v, "servers", "global", &mut out);
    // Claude Code's ~/.claude.json keeps per-project servers.
    if file_name == ".claude.json"
        && let Some(projects) = v.get("projects").and_then(|p| p.as_object())
    {
        for (path, p) in projects {
            servers_in(p, "mcpServers", path, &mut out);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_configs() {
        let text = r#"{"mcpServers":{
            "fs":{"command":"npx","args":["-y","@modelcontextprotocol/server-filesystem","/tmp"]},
            "gh":{"command":"/usr/local/bin/github-mcp","env":{"GITHUB_TOKEN":"ghp_should_never_be_read_123456"}},
            "remote":{"type":"http","url":"https://u:p@mcp.example.invalid/x?token=abc"},
            "off":{"command":"x","disabled":true}
        }}"#;
        let s = parse("claude_desktop_config.json", text).unwrap();
        assert_eq!(s.len(), 4);
        let gh = s.iter().find(|d| d.name == "gh").unwrap();
        assert_eq!(gh.env_names, vec!["GITHUB_TOKEN"]);
        assert!(!format!("{s:?}").contains("ghp_should_never"));
        let remote = s.iter().find(|d| d.name == "remote").unwrap();
        assert_eq!(remote.url.as_deref(), Some("https://mcp.example.invalid/x"));
        assert_eq!(remote.transport, "http");
        assert!(s.iter().find(|d| d.name == "off").unwrap().disabled);
        let fs = s.iter().find(|d| d.name == "fs").unwrap();
        assert_eq!(
            fs.fingerprint(),
            "cmd:npx -y @modelcontextprotocol/server-filesystem /tmp"
        );
    }

    #[test]
    fn parses_claude_json_projects_and_vscode_and_codex() {
        let claude = r#"{"oauthAccount":{"x":"y"},"mcpServers":{"a":{"command":"a"}},"projects":{"/p/one":{"mcpServers":{"b":{"command":"b"}},"history":[]}}}"#;
        let s = parse(".claude.json", claude).unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(s.iter().find(|d| d.name == "b").unwrap().scope, "/p/one");
        let vscode =
            r#"{"servers":{"pw":{"type":"stdio","command":"npx","args":["@playwright/mcp"]}}}"#;
        assert_eq!(parse("mcp.json", vscode).unwrap()[0].name, "pw");
        let codex = "model = \"o4\"\n[mcp_servers.docs]\ncommand = \"uvx\"\nargs = [\"docs-mcp\", \"--api-key\", \"abcdef0123456789\"]\n[mcp_servers.docs.env]\nAPI_KEY = \"secret\"\n";
        let s = parse("config.toml", codex).unwrap();
        assert_eq!(s[0].args, vec!["docs-mcp", "--api-key", "[redacted]"]);
        assert_eq!(s[0].env_names, vec!["API_KEY"]);
        assert!(parse("x.json", "not json").is_err());
    }
}
