//! Built-in Operator: answers common questions with no model at all.
//!
//! A small intent router picks tools, calls them through the same `ToolHost`
//! the model uses, and writes the answer from the returned data. Nothing is
//! sent anywhere. Unrecognised questions get an overview plus suggestions.

use serde_json::{Value, json};

use crate::tools::ToolHost;
use crate::{OperatorAnswer, OperatorMode, Proposal, ToolStep, call_tool};

pub const SUGGESTIONS: &[&str] = &[
    "What are my coding agents doing?",
    "Clean up after my agents",
    "What is using my RAM?",
    "What is using my CPU?",
    "Free 10 GB safely",
    "Which repositories have I not touched in 90 days?",
    "Which AI coding assistants are installed?",
    "Which MCP servers are configured?",
    "Which AGENTS.md or CLAUDE.md files are duplicated?",
    "Is Docker using a lot of space?",
    "What is listening on port 3000?",
    "Which projects depend on openssl@3?",
    "Show security findings",
];

struct Ctx<'a> {
    host: &'a dyn ToolHost,
    steps: Vec<ToolStep>,
    proposals: Vec<Proposal>,
}

impl Ctx<'_> {
    fn call(&mut self, name: &str, args: Value) -> Value {
        call_tool(self.host, name, args, &mut self.steps, &mut self.proposals)
    }
}

fn arr<'a>(v: &'a Value, k: &str) -> &'a [Value] {
    v.get(k)
        .and_then(|x| x.as_array())
        .map(Vec::as_slice)
        .unwrap_or(&[])
}
fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("")
}
fn n(v: &Value, k: &str) -> Option<f64> {
    v.get(k).and_then(|x| x.as_f64())
}

/// Binary units, matching the rest of the app (and Activity Monitor for memory).
pub fn human_bytes(b: f64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{v:.0} {}", U[i])
    } else {
        format!("{v:.1} {}", U[i])
    }
}

fn error_of(v: &Value) -> Option<&str> {
    v.get("error").and_then(|e| e.as_str())
}

/// Parse "free 10 gb", "10GB", "500 mb" into bytes.
fn parse_amount(q: &str) -> Option<f64> {
    let q = q.to_lowercase();
    let words: Vec<&str> = q
        .split(|c: char| c.is_whitespace())
        .filter(|w| !w.is_empty())
        .collect();
    for (i, w) in words.iter().enumerate() {
        let digits: String = w
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        if digits.is_empty() {
            continue;
        }
        let num: f64 = digits.parse().ok()?;
        let unit_inline = w[digits.len()..].to_string();
        let unit = if unit_inline.is_empty() {
            words.get(i + 1).map(|u| u.to_string()).unwrap_or_default()
        } else {
            unit_inline
        };
        let mult = match unit.trim_matches(|c: char| !c.is_ascii_alphabetic()) {
            "tb" => 1024f64.powi(4),
            "gb" | "g" => 1024f64.powi(3),
            "mb" | "m" => 1024f64.powi(2),
            _ => continue,
        };
        return Some(num * mult);
    }
    None
}

fn parse_number_after(q: &str, keys: &[&str]) -> Option<u64> {
    let words: Vec<&str> = q.split_whitespace().collect();
    for (i, w) in words.iter().enumerate() {
        if keys
            .iter()
            .any(|k| w.trim_matches(|c: char| !c.is_ascii_alphanumeric()) == *k)
            && let Some(next) = words.get(i + 1)
            && let Ok(v) = next.trim_matches(|c: char| !c.is_ascii_digit()).parse()
        {
            return Some(v);
        }
    }
    None
}

fn first_number(q: &str) -> Option<u64> {
    q.split(|c: char| !c.is_ascii_digit())
        .find(|t| !t.is_empty())
        .and_then(|t| t.parse().ok())
}

/// The token after "on", "of" or "depend(s) on" — e.g. "which projects depend on openssl@3".
fn subject_after(q: &str, keys: &[&str]) -> Option<String> {
    let words: Vec<&str> = q.split_whitespace().collect();
    for (i, w) in words.iter().enumerate() {
        if keys.contains(w)
            && let Some(next) = words.get(i + 1)
        {
            let t = next.trim_matches(|c: char| matches!(c, '?' | '.' | ',' | '"' | '\'' | '`'));
            if !t.is_empty() && !["the", "a", "my", "it"].contains(&t) {
                return Some(t.to_string());
            }
        }
    }
    None
}

pub fn answer(question: &str, host: &dyn ToolHost) -> OperatorAnswer {
    let q = question.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| q.contains(w));
    let mut cx = Ctx {
        host,
        steps: vec![],
        proposals: vec![],
    };

    let agent_intent = has(&[
        "agent",
        "session",
        "left running",
        "leftover",
        "orphan",
        "claude code",
        "codex",
    ]) && !has(&["agents.md", "claude.md", "instruction", "installed", "mcp"]);
    let text = if agent_intent {
        agents(
            &mut cx,
            has(&[
                "clean",
                "stop",
                "kill",
                "leftover",
                "left running",
                "orphan",
            ]),
        )
    } else if has(&["free ", "clean", "disk space", "storage", "reclaim"]) && !has(&["docker"]) {
        free_space(&mut cx, parse_amount(&q))
    } else if has(&["docker"]) {
        docker(&mut cx)
    } else if has(&["port", "listening", "not running", "localhost"]) {
        ports(
            &mut cx,
            parse_number_after(&q, &["port", "on"])
                .or_else(|| first_number(&q))
                .filter(|p| *p > 0 && *p < 65536),
        )
    } else if has(&["ram", "memory", "swap"]) {
        processes(&mut cx, "memory")
    } else if has(&["cpu", "slow", "hot", "fan", "battery"]) {
        processes(&mut cx, "cpu")
    } else if has(&[
        "agents.md",
        "claude.md",
        "instruction",
        "rules file",
        "duplicat",
    ]) {
        instruction_files(&mut cx)
    } else if has(&["mcp"]) {
        mcp(&mut cx)
    } else if has(&["optimi", "local ai", "local model", "ollama", "gpu"]) {
        local_ai(&mut cx)
    } else if has(&["ai ", "assistant", "copilot", "agent", "llm", "provider"])
        || q.ends_with(" ai")
        || q == "ai"
    {
        ai_tools(&mut cx)
    } else if has(&[
        "stale",
        "not touched",
        "haven't touched",
        "unused repo",
        "old repo",
        "inactive",
    ]) || (has(&["repo"]) && has(&["days", "month", "year"]))
    {
        let days = first_number(&q).unwrap_or(90);
        let days = if has(&["month"]) && days < 24 {
            days * 30
        } else if has(&["year"]) && days < 10 {
            days * 365
        } else {
            days
        };
        stale_repos(&mut cx, days)
    } else if has(&[
        "depend on",
        "depends on",
        "uses ",
        "safe to upgrade",
        "safe to remove",
        "remove ",
        "uninstall",
        "upgrade ",
    ]) {
        let subject = subject_after(&q, &["on", "upgrade", "remove", "uninstall", "uses"]);
        match subject {
            Some(sub) => impact(&mut cx, &sub, has(&["upgrade"])),
            None => "Name the package, runtime or tool, for example \"Which projects depend on openssl@3?\"".into(),
        }
    } else if has(&["security", "vulnerab", "cve", "outdated", "advisor"]) {
        security(&mut cx)
    } else if has(&["repo"]) {
        stale_repos(&mut cx, 0)
    } else {
        overview(&mut cx)
    };

    OperatorAnswer {
        answer: text,
        steps: cx.steps,
        proposals: cx.proposals,
        mode: OperatorMode::Builtin,
        model: None,
        data_sent_to: None,
    }
}

fn processes(cx: &mut Ctx, by: &str) -> String {
    let v = cx.call("top_processes", json!({"by": by, "limit": 8}));
    if let Some(e) = error_of(&v) {
        return format!("I could not read processes: {e}");
    }
    let list = arr(&v, "processes");
    if list.is_empty() {
        return "No processes were returned.".into();
    }
    let mut out = String::new();
    if by == "memory" {
        if let Some(sys) = v.get("system")
            && let (Some(used), Some(total)) = (n(sys, "memory_used"), n(sys, "memory_total"))
        {
            out.push_str(&format!(
                "Memory: {} of {} in use",
                human_bytes(used),
                human_bytes(total)
            ));
            if let Some(sw) = n(sys, "swap_used").filter(|x| *x > 0.0) {
                out.push_str(&format!(", {} swap", human_bytes(sw)));
            }
            out.push_str(".\n\n");
        }
        out.push_str("Largest processes by memory:\n");
    } else {
        out.push_str("Busiest processes by CPU:\n");
    }
    for p in list {
        let metric = if by == "memory" {
            n(p, "memory")
                .map(human_bytes)
                .unwrap_or_else(|| "?".into())
        } else {
            n(p, "cpu")
                .map(|c| format!("{c:.0}%"))
                .unwrap_or_else(|| "CPU unavailable".into())
        };
        let proj = s(p, "project");
        let proj = if proj.is_empty() {
            String::new()
        } else {
            format!(" — {proj}")
        };
        let prot = s(p, "protected");
        let prot = if prot.is_empty() {
            String::new()
        } else {
            format!(" (protected: {prot})")
        };
        out.push_str(&format!(
            "- {} (PID {}): {metric}{proj}{prot}\n",
            s(p, "name"),
            p.get("pid").and_then(|x| x.as_u64()).unwrap_or(0)
        ));
    }
    out.push_str("\nTo stop one, open it in Processes; AGASTHY will show a review before anything is stopped.");
    out
}

fn agents(cx: &mut Ctx, cleanup: bool) -> String {
    let v = cx.call("agent_sessions", json!({}));
    if let Some(e) = error_of(&v) {
        return format!("I could not read agent sessions: {e}");
    }
    let sessions = arr(&v, "sessions");
    let leftovers = arr(&v, "leftovers");
    let mut out = String::new();
    if sessions.is_empty() {
        out.push_str("No coding-agent sessions are running.\n");
    } else {
        let total: f64 = sessions.iter().filter_map(|s| n(s, "memory")).sum();
        out.push_str(&format!(
            "{} agent sessions are running, using {} in total:\n",
            sessions.len(),
            human_bytes(total)
        ));
        for ses in sessions {
            let proj = match s(ses, "project") {
                "" => s(ses, "cwd"),
                p => p,
            };
            let mut extras = Vec::new();
            for (k, label) in [
                ("mcp_servers", "MCP"),
                ("dev_servers", "dev server"),
                ("children", "child processes"),
            ] {
                if let Some(c) = ses.get(k).and_then(|x| x.as_u64()).filter(|c| *c > 0) {
                    extras.push(format!("{c} {label}"));
                }
            }
            out.push_str(&format!(
                "- {} (PID {}){} — {}{}\n",
                s(ses, "agent"),
                ses.get("pid").and_then(|x| x.as_u64()).unwrap_or(0),
                if proj.is_empty() {
                    String::new()
                } else {
                    format!(" in {proj}")
                },
                human_bytes(n(ses, "memory").unwrap_or(0.0)),
                if extras.is_empty() {
                    String::new()
                } else {
                    format!(", {}", extras.join(", "))
                }
            ));
        }
    }
    let stoppable: Vec<&Value> = leftovers
        .iter()
        .filter(|l| s(l, "protected").is_empty())
        .collect();
    if leftovers.is_empty() {
        out.push_str("\nNothing was left running by agents that have exited.");
    } else {
        out.push_str(&format!(
            "\n{} processes are still running after their agent exited:\n",
            leftovers.len()
        ));
        for l in leftovers.iter().take(15) {
            let from = s(l, "agent");
            out.push_str(&format!(
                "- {} (PID {}), {}{}\n",
                s(l, "name"),
                l.get("pid").and_then(|x| x.as_u64()).unwrap_or(0),
                human_bytes(n(l, "memory").unwrap_or(0.0)),
                if from.is_empty() {
                    " — parent exited, origin unknown".to_string()
                } else {
                    format!(" — started by {from}")
                }
            ));
        }
        if cleanup && !stoppable.is_empty() {
            let targets: Vec<Value> = stoppable
                .iter()
                .map(|l| json!({"pid": l["pid"], "start_time": l["start_time"]}))
                .collect();
            let _ = cx.call(
                "propose_action",
                json!({"request": {"type": "terminate_processes", "targets": targets}, "reason": format!("Stop {} processes left running by exited agents", stoppable.len())}),
            );
        } else if !stoppable.is_empty() {
            out.push_str(
                "\nAsk \"Clean up after my agents\" for a reviewed proposal to stop them.",
            );
        }
    }
    out
}

fn ports(cx: &mut Ctx, port: Option<u64>) -> String {
    let args = match port {
        Some(p) => json!({"port": p}),
        None => json!({}),
    };
    let v = cx.call("listening_ports", args);
    if let Some(e) = error_of(&v) {
        return format!("I could not read listening ports: {e}");
    }
    let list = arr(&v, "processes");
    if list.is_empty() {
        return match port {
            Some(p) => format!(
                "Nothing owned by you is listening on port {p}. If your dev server should be there, it is not running (or it runs as another user, which AGASTHY cannot see)."
            ),
            None => "No processes owned by you are listening on TCP ports.".into(),
        };
    }
    let mut out = match port {
        Some(p) => format!("Listening on port {p}:\n"),
        None => "Processes listening on TCP ports:\n".into(),
    };
    for p in list {
        let ports: Vec<String> = arr(p, "ports")
            .iter()
            .filter_map(|x| x.as_u64())
            .map(|x| x.to_string())
            .collect();
        let proj = s(p, "project");
        out.push_str(&format!(
            "- {} (PID {}) on {}{}\n",
            s(p, "name"),
            p.get("pid").and_then(|x| x.as_u64()).unwrap_or(0),
            ports.join(", "),
            if proj.is_empty() {
                String::new()
            } else {
                format!(" — {proj}")
            }
        ));
    }
    out
}

fn free_space(cx: &mut Ctx, target: Option<f64>) -> String {
    let v = cx.call("disk_cleanup_candidates", json!({}));
    if let Some(e) = error_of(&v) {
        return format!("I could not analyse disk usage: {e}");
    }
    let mut eligible: Vec<&Value> = arr(&v, "candidates")
        .iter()
        .filter(|c| c.get("eligible").and_then(|x| x.as_bool()) == Some(true))
        // Not worth a cleanup step.
        .filter(|c| n(c, "size").unwrap_or(0.0) >= 1024.0 * 1024.0)
        .collect();
    eligible.sort_by(|a, b| {
        n(b, "size")
            .unwrap_or(0.0)
            .total_cmp(&n(a, "size").unwrap_or(0.0))
    });
    let total: f64 = eligible.iter().filter_map(|c| n(c, "size")).sum();
    let mut out = String::new();
    if let Some(free) = n(&v, "free_bytes") {
        out.push_str(&format!("Free disk space now: {}.\n", human_bytes(free)));
    }
    if eligible.is_empty() {
        out.push_str("I found no regenerable folders that passed the safety checks. Open Storage to see everything that was measured and why items were excluded.");
        return out;
    }
    let goal = target.unwrap_or(total);
    let mut picked = Vec::new();
    let mut sum = 0.0;
    for c in &eligible {
        if sum >= goal {
            break;
        }
        sum += n(c, "size").unwrap_or(0.0);
        picked.push(*c);
    }
    out.push_str(&format!(
        "{} of regenerable folders passed the safety checks. ",
        human_bytes(total)
    ));
    if let Some(t) = target {
        if total < t {
            out.push_str(&format!("That is less than the {} you asked for; everything eligible is included below.\n\n", human_bytes(t)));
        } else {
            out.push_str(&format!(
                "These {} items reach {}:\n\n",
                picked.len(),
                human_bytes(sum)
            ));
        }
    } else {
        out.push_str("Largest items:\n\n");
    }
    for c in picked.iter().take(15) {
        out.push_str(&format!(
            "- {} — {} ({})\n",
            s(c, "path"),
            human_bytes(n(c, "size").unwrap_or(0.0)),
            s(c, "category")
        ));
    }
    if picked.len() > 15 {
        out.push_str(&format!("- …and {} more\n", picked.len() - 15));
    }
    out.push_str("\nThey can be rebuilt by their tools. The proposal moves them to AGASTHY's quarantine, so you can restore them until you purge it.");
    let paths: Vec<&str> = picked.iter().map(|c| s(c, "path")).collect();
    let _ = cx.call(
        "propose_action",
        json!({"request": {"type": "quarantine_paths", "paths": paths}, "reason": format!("Free about {} of regenerable folders", human_bytes(sum))}),
    );
    out
}

fn docker(cx: &mut Ctx) -> String {
    let v = cx.call("docker_status", json!({}));
    if let Some(e) = error_of(&v) {
        return format!("I could not read Docker usage: {e}");
    }
    if v.get("available").and_then(|x| x.as_bool()) == Some(false) {
        return format!("Docker usage is unavailable: {}.", s(&v, "reason"));
    }
    let mut out = String::from("Docker disk usage:\n");
    let mut reclaim_cache = 0.0;
    let mut dangling = 0.0;
    for row in arr(&v, "usage") {
        let size = n(row, "size").unwrap_or(0.0);
        let rec = n(row, "reclaimable").unwrap_or(0.0);
        out.push_str(&format!(
            "- {}: {} ({} reclaimable)\n",
            s(row, "kind"),
            human_bytes(size),
            human_bytes(rec)
        ));
        match s(row, "kind") {
            "build_cache" | "Build Cache" => reclaim_cache = rec,
            "images" | "Images" => dangling = n(row, "dangling").unwrap_or(0.0),
            _ => {}
        }
    }
    out.push_str("\nVolumes are never touched by AGASTHY: they can hold databases and other data that cannot be rebuilt.");
    if reclaim_cache > 0.0 {
        let _ = cx.call("propose_action", json!({"request": {"type": "docker_prune", "target": "build_cache"}, "reason": format!("Reclaim about {} of Docker build cache", human_bytes(reclaim_cache))}));
    }
    if dangling > 0.0 {
        let _ = cx.call("propose_action", json!({"request": {"type": "docker_prune", "target": "dangling_images"}, "reason": "Remove untagged (dangling) images"}));
    }
    out
}

fn stale_repos(cx: &mut Ctx, days: u64) -> String {
    let args = if days > 0 {
        json!({"stale_days": days})
    } else {
        json!({})
    };
    let v = cx.call("repositories", args);
    if let Some(e) = error_of(&v) {
        return format!("I could not read repositories: {e}");
    }
    let list = arr(&v, "repositories");
    if list.is_empty() {
        return if days > 0 {
            format!(
                "No scanned repository is older than {days} days. Only folders you added in Settings are scanned."
            )
        } else {
            "No repositories have been scanned. Add folders in Settings and run a scan.".into()
        };
    }
    let mut out = if days > 0 {
        format!(
            "{} repositories without a commit in {days}+ days:\n",
            list.len()
        )
    } else {
        format!("{} scanned repositories:\n", list.len())
    };
    for r in list.iter().take(25) {
        let dirty = if r.get("dirty").and_then(|x| x.as_bool()) == Some(true) {
            " — has uncommitted changes"
        } else {
            ""
        };
        let size = n(r, "size")
            .map(|b| format!(", {}", human_bytes(b)))
            .unwrap_or_default();
        out.push_str(&format!(
            "- {} ({}{size}){dirty}\n",
            s(r, "path"),
            s(r, "last_commit")
        ));
    }
    if list.len() > 25 {
        out.push_str(&format!("- …and {} more\n", list.len() - 25));
    }
    out.push_str("\nAGASTHY never deletes repositories. Their build folders (node_modules, target) can be cleaned from Storage.");
    out
}

fn ai_tools(cx: &mut Ctx) -> String {
    let v = cx.call("ai_ecosystem", json!({}));
    if let Some(e) = error_of(&v) {
        return format!("I could not read the AI ecosystem: {e}");
    }
    let tools = arr(&v, "tools");
    if tools.is_empty() {
        return "No AI tools were found. Run an AI scan from the AI page.".into();
    }
    let mut out = format!("{} AI tools found:\n", tools.len());
    for t in tools {
        let how = s(t, "installed_as").replace('_', " ");
        out.push_str(&format!(
            "- {}{}\n",
            s(t, "name"),
            if how.is_empty() || s(t, "name").to_lowercase().contains(&how) {
                String::new()
            } else {
                format!(" ({how})")
            }
        ));
    }
    let providers = arr(&v, "providers");
    if !providers.is_empty() {
        out.push_str("\nProviders found:\n");
        for p in providers {
            out.push_str(&format!("- {} — {}\n", s(p, "name"), s(p, "evidence")));
        }
    }
    out
}

fn mcp(cx: &mut Ctx) -> String {
    let v = cx.call("ai_ecosystem", json!({}));
    if let Some(e) = error_of(&v) {
        return format!("I could not read MCP configuration: {e}");
    }
    let list = arr(&v, "mcp_servers");
    if list.is_empty() {
        return "No MCP servers were found in the configuration files AGASTHY knows about.".into();
    }
    let mut out = format!("{} MCP server entries:\n", list.len());
    for m in list {
        let missing = if m.get("command_found").and_then(|x| x.as_bool()) == Some(false) {
            " — command not found"
        } else {
            ""
        };
        out.push_str(&format!(
            "- {} ({}, {}) in {}{missing}\n",
            s(m, "name"),
            s(m, "transport"),
            s(m, "scope"),
            s(m, "config")
        ));
    }
    out.push_str(
        "\nArguments and environment values are not shown; secrets are redacted at scan time.",
    );
    out
}

fn instruction_files(cx: &mut Ctx) -> String {
    let v = cx.call("ai_ecosystem", json!({}));
    if let Some(e) = error_of(&v) {
        return format!("I could not read AI instruction files: {e}");
    }
    let files = arr(&v, "files");
    if files.is_empty() {
        return "No AI instruction files were found in scanned locations.".into();
    }
    let mut out = format!("{} AI instruction files found.\n", files.len());
    let dup: Vec<&Value> = files
        .iter()
        .filter(|f| f.get("duplicate_of").is_some_and(|d| !d.is_null()))
        .collect();
    let orphan: Vec<&Value> = files
        .iter()
        .filter(|f| f.get("orphaned").and_then(|x| x.as_bool()) == Some(true))
        .collect();
    let conflicts: Vec<&Value> = files
        .iter()
        .filter(|f| f.get("conflicts").and_then(|x| x.as_u64()).unwrap_or(0) > 0)
        .collect();
    if dup.is_empty() {
        out.push_str("\nNo identical copies.\n");
    } else {
        out.push_str("\nIdentical content:\n");
        for f in &dup {
            out.push_str(&format!("- {} = {}\n", s(f, "path"), s(f, "duplicate_of")));
        }
    }
    if !orphan.is_empty() {
        out.push_str("\nNot read by any installed tool:\n");
        for f in &orphan {
            out.push_str(&format!("- {}\n", s(f, "path")));
        }
    }
    if !conflicts.is_empty() {
        out.push_str("\nPossible conflicting directives:\n");
        for f in &conflicts {
            out.push_str(&format!("- {}\n", s(f, "path")));
        }
    }
    out
}

fn local_ai(cx: &mut Ctx) -> String {
    let ov = cx.call("machine_overview", json!({}));
    let ai = cx.call("ai_ecosystem", json!({}));
    let mut out = String::new();
    if let Some(sys) = ov.get("system") {
        if let Some(t) = n(sys, "memory_total") {
            out.push_str(&format!("Memory: {} total", human_bytes(t)));
            if let Some(a) = n(sys, "memory_available") {
                out.push_str(&format!(", {} available now", human_bytes(a)));
            }
            out.push_str(".\n");
        }
        let gpus = arr(sys, "gpus");
        for g in gpus {
            out.push_str(&format!("GPU: {}\n", s(g, "name")));
        }
    }
    let models = arr(&ai, "local_models");
    if models.is_empty() {
        out.push_str("No local models were found.\n");
    } else {
        out.push_str(&format!("\n{} local models:\n", models.len()));
        for m in models {
            out.push_str(&format!(
                "- {} ({}){}\n",
                s(m, "name"),
                s(m, "runtime"),
                n(m, "size")
                    .map(|b| format!(", {}", human_bytes(b)))
                    .unwrap_or_default()
            ));
        }
    }
    out.push_str("\nOn Apple silicon, memory is shared with the GPU: a model needs roughly its file size in free memory plus room for context. Closing the largest processes (ask \"What is using my RAM?\") gives a local model the most headroom.");
    out
}

fn impact(cx: &mut Ctx, subject: &str, upgrade: bool) -> String {
    let found = cx.call("search", json!({"text": subject, "limit": 10}));
    let results = arr(&found, "results");
    let exact = results
        .iter()
        .find(|r| s(r, "name").eq_ignore_ascii_case(subject))
        .or_else(|| results.first());
    let Some(hit) = exact else {
        return format!(
            "I could not find \"{subject}\" in the discovered software, packages or dependencies."
        );
    };
    let id = s(hit, "id").to_string();
    let v = cx.call("removal_impact", json!({"id": id}));
    if let Some(e) = error_of(&v) {
        return format!(
            "I found {} but could not simulate its impact: {e}",
            s(hit, "name")
        );
    }
    let mut out = format!("{} — {}\n", s(hit, "name"), s(&v, "summary"));
    if let Some(risk) = v.get("risk").and_then(|r| r.as_str()) {
        out.push_str(&format!("Risk if removed: {risk}\n"));
    }
    for sec in arr(&v, "sections") {
        let items = arr(sec, "items");
        if items.is_empty() {
            continue;
        }
        out.push_str(&format!("\n{} ({}):\n", s(sec, "title"), items.len()));
        for it in items.iter().take(12) {
            out.push_str(&format!(
                "- {}\n",
                it.as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| s(it, "name").to_string())
            ));
        }
    }
    let unknowns = arr(&v, "unknowns");
    if !unknowns.is_empty() {
        out.push_str("\nWhat AGASTHY cannot see:\n");
        for u in unknowns {
            out.push_str(&format!("- {}\n", u.as_str().unwrap_or("")));
        }
    }
    if upgrade {
        out.push_str("\nTo test an upgrade without touching your checkout, open a dependent project and use Update Lab: it works in a separate Git worktree.");
    }
    out
}

fn security(cx: &mut Ctx) -> String {
    let v = cx.call("security_findings", json!({}));
    if let Some(e) = error_of(&v) {
        return format!("I could not read security findings: {e}");
    }
    let list = arr(&v, "findings");
    if list.is_empty() {
        return "No findings. Run a security scan from the Security page (advisory lookups are off unless you enable them).".into();
    }
    let mut out = format!("{} findings:\n", list.len());
    for f in list.iter().take(20) {
        out.push_str(&format!("- [{}] {}\n", s(f, "severity"), s(f, "title")));
    }
    if list.len() > 20 {
        out.push_str(&format!(
            "- …and {} more on the Security page\n",
            list.len() - 20
        ));
    }
    out
}

fn overview(cx: &mut Ctx) -> String {
    let v = cx.call("machine_overview", json!({}));
    if let Some(e) = error_of(&v) {
        return format!("I could not read the machine overview: {e}");
    }
    let mut out = String::from("Here is what AGASTHY knows about this machine:\n");
    if let Some(counts) = v.get("counts").and_then(|c| c.as_object()) {
        for (k, c) in counts {
            if c.as_u64().unwrap_or(0) > 0 {
                out.push_str(&format!("- {}: {}\n", k.replace('_', " "), c));
            }
        }
    }
    out.push_str("\nI answer without a model, so I understand a fixed set of questions. Try:\n");
    for sug in SUGGESTIONS.iter().take(6) {
        out.push_str(&format!("- {sug}\n"));
    }
    out.push_str("\nFor open-ended questions, connect a model in Settings → AI model (local models keep data on this machine).");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

    struct Host;
    impl ToolHost for Host {
        fn call(&self, name: &str, args: &Value) -> Result<Value, String> {
            Ok(match name {
                "top_processes" => json!({
                    "system": {"memory_used": 12.0 * GIB, "memory_total": 16.0 * GIB, "swap_used": GIB},
                    "processes": [{"name": "node", "pid": 42, "memory": 2.1 * GIB, "cpu": 50.0, "project": "~/code/web", "protected": ""}]
                }),
                "agent_sessions" => json!({
                    "sessions": [{"agent": "Claude Code", "pid": 10, "project": "web", "memory": 0.5 * GIB, "mcp_servers": 1, "dev_servers": 0, "children": 2}],
                    "leftovers": [{"name": "vite", "pid": 40, "start_time": 1040, "memory": 0.1 * GIB, "agent": "Claude Code", "protected": ""}]
                }),
                "listening_ports" => {
                    if args.get("port").and_then(|p| p.as_u64()) == Some(3000) {
                        json!({"processes": []})
                    } else {
                        json!({"processes": [{"name": "vite", "pid": 7, "ports": [5173], "project": ""}]})
                    }
                }
                "disk_cleanup_candidates" => json!({"free_bytes": 4e9, "candidates": [
                    {"path": "~/code/a/node_modules", "size": 6e9, "category": "node_modules", "eligible": true},
                    {"path": "~/code/b/target", "size": 5e9, "category": "rust target", "eligible": true},
                    {"path": "~/code/c/node_modules", "size": 9e9, "category": "node_modules", "eligible": false}
                ]}),
                "propose_action" => {
                    json!({"request": args["request"], "title": args["reason"], "can_execute": true, "reason": args["reason"]})
                }
                "docker_status" => json!({"available": true, "usage": [
                    {"kind": "build_cache", "size": 3e9, "reclaimable": 3e9},
                    {"kind": "volumes", "size": 1e9, "reclaimable": 1e9}
                ]}),
                "repositories" => {
                    json!({"repositories": [{"path": "~/old", "last_commit": "2024-01-02", "dirty": true, "size": 1e8}]})
                }
                "search" => {
                    json!({"results": [{"id": "package:homebrew:openssl@3", "name": "openssl@3"}]})
                }
                "removal_impact" => {
                    json!({"summary": "4 projects would break", "risk": "high", "sections": [{"title": "Projects", "items": ["~/code/web"]}], "unknowns": []})
                }
                "machine_overview" => {
                    json!({"counts": {"repository": 4, "package": 300}, "system": {"memory_total": 16e9, "gpus": [{"name": "Apple M2"}]}})
                }
                "ai_ecosystem" => {
                    json!({"tools": [{"name": "Claude Code", "installed_as": "npm"}], "providers": [], "mcp_servers": [], "files": [], "local_models": []})
                }
                "security_findings" => json!({"findings": []}),
                _ => return Err("unexpected".into()),
            })
        }
    }

    #[test]
    fn agents_and_cleanup() {
        let a = answer("What are my coding agents doing?", &Host);
        assert_eq!(a.steps[0].tool, "agent_sessions");
        assert!(a.answer.contains("Claude Code (PID 10) in web"));
        assert!(a.answer.contains("vite (PID 40)"));
        assert!(
            a.proposals.is_empty(),
            "only proposes when asked to clean up"
        );
        let c = answer("Clean up after my agents", &Host);
        assert_eq!(c.proposals.len(), 1);
        assert_eq!(c.proposals[0].request["type"], "terminate_processes");
        assert_eq!(c.proposals[0].request["targets"][0]["start_time"], 1040);
        // Instruction-file questions still go to the AI ecosystem.
        assert_eq!(
            answer("Which AGENTS.md files are duplicated?", &Host).steps[0].tool,
            "ai_ecosystem"
        );
    }

    #[test]
    fn amounts_parse() {
        assert_eq!(parse_amount("free 10 gb safely"), Some(10.0 * GIB));
        assert_eq!(parse_amount("free 500MB"), Some(500.0 * 1024.0 * 1024.0));
        assert_eq!(parse_amount("free some space"), None);
    }

    #[test]
    fn memory_question_uses_processes() {
        let a = answer("What is using my RAM?", &Host);
        assert_eq!(a.steps[0].tool, "top_processes");
        assert!(a.answer.contains("node (PID 42): 2.1 GB"));
        assert!(a.answer.contains("12.0 GB of 16.0 GB"));
        assert!(a.proposals.is_empty(), "never proposes termination unasked");
        assert_eq!(a.mode, OperatorMode::Builtin);
        assert!(a.data_sent_to.is_none());
    }

    #[test]
    fn free_space_proposes_only_eligible_paths() {
        let a = answer("Free 10 GB safely", &Host);
        assert_eq!(a.proposals.len(), 1);
        let paths = a.proposals[0].request["paths"].as_array().unwrap();
        assert_eq!(paths.len(), 2, "6 GB + 5 GB reach 10 GB");
        assert!(
            !paths.iter().any(|p| p.as_str().unwrap().contains("/c/")),
            "ineligible path excluded"
        );
        assert_eq!(a.proposals[0].request["type"], "quarantine_paths");
    }

    #[test]
    fn port_not_running() {
        let a = answer("my dev server on port 3000 is not running", &Host);
        assert!(a.answer.contains("port 3000"));
        assert!(a.answer.contains("not running"));
    }

    #[test]
    fn docker_never_volumes() {
        let a = answer("Is Docker using a lot of space?", &Host);
        assert_eq!(a.proposals.len(), 1);
        assert_eq!(a.proposals[0].request["target"], "build_cache");
        assert!(a.answer.contains("never touched"));
    }

    #[test]
    fn routes_other_intents() {
        assert!(
            answer("Which repositories have I not touched in 90 days?", &Host)
                .answer
                .contains("uncommitted")
        );
        let a = answer("Which projects depend on openssl@3?", &Host);
        assert_eq!(
            a.steps.iter().map(|s| s.tool.as_str()).collect::<Vec<_>>(),
            ["search", "removal_impact"]
        );
        assert!(a.answer.contains("4 projects would break"));
        assert!(
            answer("Which AI coding assistants are installed?", &Host)
                .answer
                .contains("Claude Code")
        );
        assert!(
            answer("optimize my machine for local AI", &Host)
                .answer
                .contains("Apple M2")
        );
        assert!(answer("hello", &Host).answer.contains("Try:"));
    }
}
