//! `agasthy mcp`: a read-only Model Context Protocol server over stdio.
//!
//! Newline-delimited JSON-RPC 2.0 on stdin/stdout, as MCP's stdio transport
//! specifies. Every tool only reads (the app's database is opened read-only,
//! processes and logs are read, nothing is started, stopped or written), and
//! each tool says so with `readOnlyHint`. Diagnostics go to stderr only, so
//! stdout carries nothing but protocol messages.

use std::io::{BufRead, Write};
use std::path::PathBuf;

use serde_json::{Value, json};

use crate::queries;

/// Protocol versions this server speaks, newest first.
const SUPPORTED: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "AGASTHY answers questions about this developer machine from real data: \
repositories and their uncommitted work, runtimes, packages and tools, what breaks if something is \
removed, live coding-agent sessions and what they left running, processes and ports, and token usage. \
Call `status` first to see how fresh the scanned data is. Every tool is read-only; to change anything \
(stop processes, uninstall, clean up), point the user to the AGASTHY app, which reviews each change \
with a dry run. A value with \"status\": \"unavailable\" means unknown, never zero. Token costs are \
estimates from API list prices.";

fn read_only() -> Value {
    json!({
        "readOnlyHint": true,
        "destructiveHint": false,
        "idempotentHint": true,
        "openWorldHint": false,
    })
}

fn no_args() -> Value {
    json!({ "type": "object", "properties": {}, "additionalProperties": false })
}

fn tools() -> Value {
    let id_arg = |what: &str| {
        json!({
            "type": "object",
            "properties": { "id": { "type": "string", "description": format!("Entity id from `find`, e.g. `package:homebrew:openssl@3`. {what}") } },
            "required": ["id"],
            "additionalProperties": false,
        })
    };
    let tool = |name: &str, title: &str, description: &str, schema: Value| {
        json!({
            "name": name,
            "title": title,
            "description": description,
            "inputSchema": schema,
            "annotations": read_only(),
        })
    };
    json!([
        tool(
            "status",
            "Data freshness",
            "When each kind of data (repositories, software, packages, AI tools) was last scanned, as `age_hours`, and how many items of each kind exist. Call this first.",
            no_args()
        ),
        tool(
            "find",
            "Find items",
            "Search everything AGASTHY found: repositories, applications, runtimes, packages, package managers, tools, AI tools and providers, MCP servers, AI instruction files, security findings. Returns ids for `entity` and `impact`. Use `full` for all attributes, such as a repository's uncommitted work (`attributes.working_tree.value`) and branch (`attributes.head.value.branch`).",
            json!({
                "type": "object",
                "properties": {
                    "text": { "type": "string", "description": "Words to search for in names, paths and attributes. Omit to list by kind." },
                    "kind": { "type": "string", "description": "Only this kind.", "enum": ["repository", "remote_project", "application", "binary", "runtime", "tool", "package", "package_manager", "ai_tool", "ai_provider", "mcp_server", "ai_config_file", "finding"] },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 200, "default": 25 },
                    "full": { "type": "boolean", "default": false, "description": "Include every attribute." },
                },
                "additionalProperties": false,
            })
        ),
        tool(
            "entity",
            "Item details",
            "One item with all its attributes, the evidence each fact came from, and its relationships (what it depends on, what depends on it).",
            id_arg("")
        ),
        tool(
            "impact",
            "What breaks if removed",
            "Before uninstalling a package, runtime or tool: which projects would break or fall back to another installed copy, a `risk` level (low, medium, high), a plan and how to recover. Read-only; it removes nothing.",
            id_arg("The item you are thinking of removing.")
        ),
        tool(
            "shared_dependencies",
            "Shared dependencies",
            "Dependencies used by the most projects on this machine.",
            no_args()
        ),
        tool(
            "agents",
            "Coding agents",
            "Live coding-agent sessions (Claude Code, Codex, Cursor, Gemini CLI, Hermes, OpenClaw and others): project, memory, CPU, MCP servers and dev servers each started, and ports. `leftovers` are processes still running after the agent that started them exited.",
            no_args()
        ),
        tool(
            "processes",
            "Processes and ports",
            "Running processes with the ports they listen on, memory, command line and the project each belongs to. Use it to answer \"what is on port 3000?\". Lists the largest by memory first.",
            json!({
                "type": "object",
                "properties": { "limit": { "type": "integer", "minimum": 1, "maximum": 1000, "default": 150 } },
                "additionalProperties": false,
            })
        ),
        tool(
            "token_usage",
            "Token usage",
            "Tokens used by coding agents, from their own local logs (counters only, never message text), with estimated cost from API list prices. Totals per agent; `full` adds per-session and per-day rows with project folder (`cwd`) and model.",
            json!({
                "type": "object",
                "properties": {
                    "days": { "type": "integer", "minimum": 1, "maximum": 90, "default": 7 },
                    "full": { "type": "boolean", "default": false },
                },
                "additionalProperties": false,
            })
        ),
        tool(
            "telemetry",
            "Memory, CPU and disk",
            "Memory, swap, CPU, load and disk usage right now, with the top processes by memory and CPU.",
            no_args()
        ),
    ])
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn arg_bool(args: &Value, key: &str) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn arg_u64(args: &Value, key: &str, default: u64, max: u64) -> Result<u64, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v
            .as_u64()
            .filter(|n| (1..=max).contains(n))
            .ok_or_else(|| format!("`{key}` must be a whole number from 1 to {max}")),
    }
}

/// Run one tool. `Err` is a tool error the model should see, not a protocol error.
fn call(db: &Option<PathBuf>, name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "status" => queries::status(db),
        "find" => queries::find(
            db,
            arg_str(args, "text").map(str::to_string),
            arg_str(args, "kind"),
            arg_u64(args, "limit", 25, 200)? as u32,
            arg_bool(args, "full"),
        ),
        "entity" | "impact" => {
            let id = arg_str(args, "id").ok_or("`id` is required (get one from `find`)")?;
            if name == "entity" {
                queries::entity(db, id)
            } else {
                queries::impact(db, id)
            }
        }
        "shared_dependencies" => queries::shared(db),
        "agents" => queries::agents(db),
        "processes" => queries::processes(db, Some(arg_u64(args, "limit", 150, 1000)? as usize)),
        "token_usage" => {
            queries::usage(arg_u64(args, "days", 7, 90)? as u32, arg_bool(args, "full"))
        }
        "telemetry" => queries::telemetry(),
        other => Err(format!("unknown tool `{other}`")),
    }
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// Handle one message. `None` for notifications, which get no reply.
pub fn handle(db: &Option<PathBuf>, msg: &Value) -> Option<Value> {
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(Value::as_str);
    let (Some(id), Some(method)) = (id, method) else {
        // Notifications (no id) and stray responses need no reply.
        return match (msg.get("id"), method) {
            (Some(id), None) if msg.get("result").is_none() && msg.get("error").is_none() => {
                Some(error(id.clone(), -32600, "invalid request: no method"))
            }
            _ => None,
        };
    };
    let params = msg.get("params").cloned().unwrap_or(json!({}));
    Some(match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str);
            let version = asked
                .filter(|v| SUPPORTED.contains(v))
                .unwrap_or(SUPPORTED[0]);
            result(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": {
                        "name": "agasthy",
                        "title": "AGASTHY",
                        "version": env!("CARGO_PKG_VERSION"),
                        "websiteUrl": "https://github.com/HMAKT99/Agasthy",
                    },
                    "instructions": INSTRUCTIONS,
                }),
            )
        }
        "ping" => result(id, json!({})),
        "tools/list" => result(id, json!({ "tools": tools() })),
        "tools/call" => {
            let Some(name) = params.get("name").and_then(Value::as_str) else {
                return Some(error(id, -32602, "invalid params: `name` is required"));
            };
            let known = tools()
                .as_array()
                .is_some_and(|t| t.iter().any(|t| t["name"] == name));
            if !known {
                return Some(error(id, -32602, &format!("unknown tool `{name}`")));
            }
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            match call(db, name, &args) {
                Ok(v) => result(
                    id,
                    json!({
                        "content": [{ "type": "text", "text": serde_json::to_string(&v).unwrap_or_default() }],
                        "isError": false,
                    }),
                ),
                Err(e) => result(
                    id,
                    json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
                ),
            }
        }
        other => error(id, -32601, &format!("method not found: {other}")),
    })
}

/// Serve until stdin closes.
pub fn serve(db: Option<PathBuf>) -> Result<(), String> {
    eprintln!(
        "agasthy {} MCP server (read-only) on stdio",
        env!("CARGO_PKG_VERSION")
    );
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) if msg.is_object() => handle(&db, &msg),
            Ok(_) => Some(error(Value::Null, -32600, "invalid request")),
            Err(_) => Some(error(Value::Null, -32700, "parse error")),
        };
        if let Some(r) = reply {
            serde_json::to_writer(&mut stdout, &r).map_err(|e| e.to_string())?;
            stdout.write_all(b"\n").map_err(|e| e.to_string())?;
            stdout.flush().map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
