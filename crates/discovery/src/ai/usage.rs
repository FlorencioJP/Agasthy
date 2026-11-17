//! Token usage of coding agents, read from their own local session logs.
//!
//! * Claude Code: `~/.claude/projects/<folder>/<session>.jsonl`. Assistant
//!   entries carry `message.usage`; one API response is written as several
//!   lines with the same `message.id`, so each id is counted once.
//! * Codex: `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`. `token_count`
//!   events carry a running total; the last one is the session's total.
//!
//! * GitHub Copilot CLI: `~/.copilot/session-state/<session>/events.jsonl`
//!   (or `$COPILOT_HOME/session-state`). `session.shutdown` events carry the
//!   session's per-model totals (`data.modelMetrics`), cumulative across
//!   resumes; each snapshot is counted as the change from the previous one.
//!   A session that is still running (or was killed) has no shutdown yet.
//!
//! * Hermes Agent: `~/.hermes/state.db`, table `sessions`, opened read-only;
//!   only its token and cost columns are selected (never prompts or titles).
//!
//! Only counters, model names, timestamps and the session folder are read.
//! Message text is never stored, returned or logged. Files are read
//! incrementally: on refresh only bytes appended since the last read are parsed.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TokenCounts {
    pub input: u64,
    pub output: u64,
    /// Prompt tokens written to the provider's cache.
    pub cache_write: u64,
    /// Prompt tokens served from the provider's cache.
    pub cache_read: u64,
    pub requests: u64,
}

impl TokenCounts {
    pub fn add(&mut self, o: &TokenCounts) {
        self.input += o.input;
        self.output += o.output;
        self.cache_write += o.cache_write;
        self.cache_read += o.cache_read;
        self.requests += o.requests;
    }
    /// Every token processed (input of all kinds plus output).
    pub fn total(&self) -> u64 {
        self.input + self.output + self.cache_write + self.cache_read
    }
}

/// Published API list prices, USD per million tokens. An estimate only:
/// subscription plans (Claude Max, ChatGPT Pro, …) are billed differently.
struct Price {
    model: &'static str,
    input: f64,
    output: f64,
    cache_read: f64,
}

const PRICES: &[Price] = &[
    Price {
        model: "claude-fable-5-2",
        input: 8.0,
        output: 40.0,
        cache_read: 0.20,
    },
    Price {
        model: "claude-mythos-5-2",
        input: 8.0,
        output: 40.0,
        cache_read: 0.20,
    },
    Price {
        model: "claude-fable-5-1",
        input: 10.0,
        output: 50.0,
        cache_read: 1.0,
    },
    Price {
        model: "claude-fable-5",
        input: 10.0,
        output: 50.0,
        cache_read: 1.0,
    },
    Price {
        model: "claude-opus-5-5",
        input: 4.0,
        output: 20.0,
        cache_read: 0.20,
    },
    Price {
        model: "claude-opus-5",
        input: 5.0,
        output: 25.0,
        cache_read: 0.50,
    },
    Price {
        model: "claude-opus-4-8",
        input: 5.0,
        output: 25.0,
        cache_read: 0.50,
    },
    Price {
        model: "claude-opus-4-7",
        input: 5.0,
        output: 25.0,
        cache_read: 0.50,
    },
    Price {
        model: "claude-opus-4-6",
        input: 5.0,
        output: 25.0,
        cache_read: 0.50,
    },
    Price {
        model: "claude-sonnet-5-5",
        input: 2.0,
        output: 10.0,
        cache_read: 0.10,
    },
    Price {
        model: "claude-sonnet-5",
        input: 2.0,
        output: 10.0,
        cache_read: 0.20,
    },
    Price {
        model: "claude-sonnet-4-6",
        input: 3.0,
        output: 15.0,
        cache_read: 0.30,
    },
    Price {
        model: "claude-haiku-5-5",
        input: 0.10,
        output: 0.50,
        cache_read: 0.01,
    },
    Price {
        model: "claude-haiku-4-5",
        input: 1.0,
        output: 5.0,
        cache_read: 0.10,
    },
];

/// Estimated cost in USD at API list prices, or `None` when the model's price is unknown.
pub fn estimate_cost(model: &str, t: &TokenCounts) -> Option<f64> {
    // Exact id, or a dated / suffixed variant of it (`claude-opus-4-6-20260101`, `…[1m]`).
    let p = PRICES
        .iter()
        .filter(|p| {
            model == p.model
                || model.starts_with(&format!("{}-", p.model))
                || model.starts_with(&format!("{}[", p.model))
        })
        .max_by_key(|p| p.model.len())?;
    let m = 1_000_000.0;
    Some(
        t.input as f64 * p.input / m
            + t.output as f64 * p.output / m
            // 5-minute cache writes cost 1.25× input.
            + t.cache_write as f64 * p.input * 1.25 / m
            + t.cache_read as f64 * p.cache_read / m,
    )
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ModelUsage {
    pub model: String,
    pub tokens: TokenCounts,
    pub estimated_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SessionUsage {
    /// `claude-code` or `codex`.
    pub agent: String,
    pub session_id: String,
    pub cwd: Option<String>,
    pub first_at: Option<i64>,
    pub last_at: Option<i64>,
    pub tokens: TokenCounts,
    pub models: Vec<ModelUsage>,
    pub estimated_cost_usd: Option<f64>,
    /// Some models had no known price, so the estimate is partial.
    pub cost_partial: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DayUsage {
    /// `YYYY-MM-DD` (UTC).
    pub day: String,
    pub agent: String,
    pub cwd: Option<String>,
    pub model: String,
    pub tokens: TokenCounts,
    pub estimated_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct UsageReport {
    pub sessions: Vec<SessionUsage>,
    pub days: Vec<DayUsage>,
    /// Log folders that were looked at, and whether each exists.
    pub sources: Vec<UsageSource>,
    pub files_read: u32,
    pub window_days: u32,
    /// Sources that exist but could not be read.
    pub problems: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct UsageSource {
    pub agent: String,
    pub path: String,
    pub found: bool,
}

/// Per (day, model) counters for one file.
type Buckets = BTreeMap<(String, String), TokenCounts>;

#[derive(Debug, Default)]
struct FileState {
    offset: u64,
    agent: &'static str,
    session_id: String,
    cwd: Option<String>,
    first_at: Option<i64>,
    last_at: Option<i64>,
    buckets: Buckets,
    /// Claude Code: message ids already counted.
    seen_ids: HashSet<String>,
    /// Codex: model in effect and the running total last counted.
    codex_model: Option<String>,
    codex_last_total: TokenCounts,
    /// Copilot CLI: per-model totals from the last `session.shutdown`.
    copilot_last: HashMap<String, TokenCounts>,
}

/// Copilot CLI's data folder: `$COPILOT_HOME`, else `~/.copilot`.
fn copilot_root(home: &Path) -> PathBuf {
    std::env::var_os("COPILOT_HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".copilot"))
        .join("session-state")
}

/// Incremental reader over the agents' session logs.
#[derive(Debug, Default)]
pub struct UsageIndex {
    files: HashMap<PathBuf, FileState>,
}

const MAX_LINE: usize = 8 * 1024 * 1024;

impl UsageIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Read new data from every log modified in the last `window_days` and summarise.
    pub fn refresh(&mut self, home: &Path, window_days: u32, now_ms: i64) -> UsageReport {
        let cutoff = now_ms - i64::from(window_days) * 86_400_000;
        let claude_root = home.join(".claude/projects");
        let codex_root = home.join(".codex/sessions");
        let mut files: Vec<(PathBuf, &'static str)> = Vec::new();
        collect_jsonl(&claude_root, 1, cutoff, &mut |p| {
            files.push((p, "claude-code"))
        });
        collect_jsonl(&codex_root, 3, cutoff, &mut |p| files.push((p, "codex")));
        collect_jsonl(&copilot_root(home), 1, cutoff, &mut |p| {
            if p.file_name().is_some_and(|n| n == "events.jsonl") {
                files.push((p, "copilot-cli"))
            }
        });

        let live: HashSet<&PathBuf> = files.iter().map(|(p, _)| p).collect();
        self.files.retain(|p, _| live.contains(p));
        for (path, agent) in &files {
            let st = self.files.entry(path.clone()).or_insert_with(|| FileState {
                agent,
                session_id: session_id_of(path, agent),
                ..Default::default()
            });
            let len = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            if len < st.offset {
                // Rewritten (not appended): start over.
                *st = FileState {
                    agent,
                    session_id: st.session_id.clone(),
                    ..Default::default()
                };
            }
            if len > st.offset {
                let _ = read_from(path, st);
            }
        }
        let mut report = self.report(home, window_days, &files);
        let hermes_db = home.join(".hermes/state.db");
        report.sources.push(UsageSource {
            agent: "hermes".into(),
            found: hermes_db.is_file(),
            path: hermes_db.display().to_string(),
        });
        if hermes_db.is_file() {
            match read_hermes(&hermes_db, cutoff) {
                Ok((sessions, days)) => {
                    report.sessions.extend(sessions);
                    report.days.extend(days);
                    report.sessions.sort_by(|a, b| b.last_at.cmp(&a.last_at));
                    report
                        .days
                        .sort_by(|a, b| (&a.day, &a.agent).cmp(&(&b.day, &b.agent)));
                }
                Err(e) => report.problems.push(format!("Hermes: {e}")),
            }
        }
        report
    }

    fn report(
        &self,
        home: &Path,
        window_days: u32,
        files: &[(PathBuf, &'static str)],
    ) -> UsageReport {
        let mut sessions = Vec::new();
        let mut days: BTreeMap<(String, String, Option<String>, String), TokenCounts> =
            BTreeMap::new();
        for (path, _) in files {
            let Some(st) = self.files.get(path) else {
                continue;
            };
            if st.buckets.is_empty() {
                continue;
            }
            let mut by_model: BTreeMap<String, TokenCounts> = BTreeMap::new();
            for ((day, model), t) in &st.buckets {
                by_model.entry(model.clone()).or_default().add(t);
                days.entry((
                    day.clone(),
                    st.agent.to_string(),
                    st.cwd.clone(),
                    model.clone(),
                ))
                .or_default()
                .add(t);
            }
            let mut tokens = TokenCounts::default();
            let mut cost = 0.0;
            let mut partial = false;
            let models: Vec<ModelUsage> = by_model
                .into_iter()
                .map(|(model, t)| {
                    tokens.add(&t);
                    let c = estimate_cost(&model, &t);
                    match c {
                        Some(c) => cost += c,
                        None => partial = true,
                    }
                    ModelUsage {
                        model,
                        tokens: t,
                        estimated_cost_usd: c,
                    }
                })
                .collect();
            let all_unknown = models.iter().all(|m| m.estimated_cost_usd.is_none());
            sessions.push(SessionUsage {
                agent: st.agent.into(),
                session_id: st.session_id.clone(),
                cwd: st.cwd.clone(),
                first_at: st.first_at,
                last_at: st.last_at,
                tokens,
                models,
                estimated_cost_usd: (!all_unknown).then_some(cost),
                cost_partial: partial && !all_unknown,
            });
        }
        sessions.sort_by(|a, b| b.last_at.cmp(&a.last_at));
        let days = days
            .into_iter()
            .map(|((day, agent, cwd, model), tokens)| DayUsage {
                estimated_cost_usd: estimate_cost(&model, &tokens),
                day,
                agent,
                cwd,
                model,
                tokens,
            })
            .collect();
        let src = |agent: &str, p: PathBuf| UsageSource {
            agent: agent.into(),
            found: p.is_dir(),
            path: p.display().to_string(),
        };
        UsageReport {
            sessions,
            days,
            sources: vec![
                src("claude-code", home.join(".claude/projects")),
                src("codex", home.join(".codex/sessions")),
                src("copilot-cli", copilot_root(home)),
            ],
            files_read: self.files.len() as u32,
            window_days,
            problems: Vec::new(),
        }
    }
}

/// Hermes Agent keeps per-session totals in SQLite. Opened read-only; if the
/// database is locked by a running Hermes, it is read as an immutable snapshot.
fn read_hermes(db: &Path, cutoff_ms: i64) -> Result<(Vec<SessionUsage>, Vec<DayUsage>), String> {
    use rusqlite::{Connection, OpenFlags};
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
        | OpenFlags::SQLITE_OPEN_URI
        | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let uri = |q: &str| format!("file:{}?{q}", db.display());
    let conn = Connection::open_with_flags(uri("mode=ro"), flags)
        .and_then(|c| {
            c.query_row("SELECT count(*) FROM sessions", [], |_| Ok(()))
                .map(|_| c)
        })
        .or_else(|_| Connection::open_with_flags(uri("immutable=1"), flags))
        .map_err(|e| e.to_string())?;
    let cols: HashSet<String> = conn
        .prepare("SELECT name FROM pragma_table_info('sessions')")
        .and_then(|mut s| s.query_map([], |r| r.get::<_, String>(0))?.collect())
        .map_err(|e| e.to_string())?;
    if !cols.contains("started_at") || !cols.contains("input_tokens") {
        return Err("unrecognised Hermes database layout".into());
    }
    // Only these columns are ever read; a missing optional one reads as NULL.
    let pick = |c: &str| {
        if cols.contains(c) {
            c.to_string()
        } else {
            "NULL".to_string()
        }
    };
    let sql = format!(
        "SELECT id, {model}, started_at, {ended}, input_tokens, {out}, {cr}, {cw}, {calls}, {cwd}, {est}, {act} FROM sessions WHERE started_at >= ?1",
        model = pick("model"),
        ended = pick("ended_at"),
        out = pick("output_tokens"),
        cr = pick("cache_read_tokens"),
        cw = pick("cache_write_tokens"),
        calls = pick("api_call_count"),
        cwd = pick("cwd"),
        est = pick("estimated_cost_usd"),
        act = pick("actual_cost_usd"),
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([cutoff_ms as f64 / 1000.0], |r| {
            let n = |i: usize| {
                r.get::<_, Option<i64>>(i)
                    .map(|v| v.unwrap_or(0).max(0) as u64)
            };
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, f64>(2)?,
                r.get::<_, Option<f64>>(3)?,
                TokenCounts {
                    input: n(4)?,
                    output: n(5)?,
                    cache_read: n(6)?,
                    cache_write: n(7)?,
                    requests: n(8)?,
                },
                r.get::<_, Option<String>>(9)?,
                r.get::<_, Option<f64>>(10)?,
                r.get::<_, Option<f64>>(11)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut sessions = Vec::new();
    let mut days = Vec::new();
    for row in rows.flatten() {
        let (id, model, started, ended, tokens, cwd, est, act) = row;
        if tokens.total() == 0 {
            continue;
        }
        let model = model.unwrap_or_else(|| "unknown".into());
        // Hermes' own cost (actual, else its estimate), else ours.
        let cost = act.or(est).or_else(|| estimate_cost(&model, &tokens));
        let started_ms = (started * 1000.0) as i64;
        let day = date_of(started_ms);
        days.push(DayUsage {
            day,
            agent: "hermes".into(),
            cwd: cwd.clone(),
            model: model.clone(),
            tokens,
            estimated_cost_usd: cost,
        });
        sessions.push(SessionUsage {
            agent: "hermes".into(),
            session_id: id,
            cwd,
            first_at: Some(started_ms),
            last_at: Some(ended.map(|e| (e * 1000.0) as i64).unwrap_or(started_ms)),
            tokens,
            models: vec![ModelUsage {
                model,
                tokens,
                estimated_cost_usd: cost,
            }],
            estimated_cost_usd: cost,
            cost_partial: false,
        });
    }
    Ok((sessions, days))
}

/// `YYYY-MM-DD` (UTC) for a millisecond timestamp.
fn date_of(ms: i64) -> String {
    let z = ms.div_euclid(86_400_000) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y}-{m:02}-{d:02}")
}

/// `*.jsonl` files up to `depth` folders below `root`, modified after `cutoff`. Symlinks are not followed.
fn collect_jsonl(root: &Path, depth: usize, cutoff: i64, push: &mut dyn FnMut(PathBuf)) {
    let walker = walkdir::WalkDir::new(root)
        .follow_links(false)
        .max_depth(depth + 1)
        .into_iter()
        .filter_map(Result::ok);
    for e in walker {
        if !e.file_type().is_file()
            || e.path().extension().and_then(|x| x.to_str()) != Some("jsonl")
        {
            continue;
        }
        let recent = e
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(agasthy_core::time::system_time_ms)
            .is_some_and(|t| t >= cutoff);
        if recent {
            push(e.into_path());
        }
    }
}

fn read_from(path: &Path, st: &mut FileState) -> std::io::Result<()> {
    let mut f = std::fs::File::open(path)?;
    f.seek(SeekFrom::Start(st.offset))?;
    let mut reader = BufReader::new(f.take(u64::MAX));
    let mut buf = Vec::new();
    loop {
        buf.clear();
        let n = reader.read_until(b'\n', &mut buf)?;
        if n == 0 {
            break;
        }
        // A partial last line (still being written) is read again next time.
        if buf.last() != Some(&b'\n') {
            break;
        }
        st.offset += n as u64;
        if buf.len() > MAX_LINE {
            continue;
        }
        let line = String::from_utf8_lossy(&buf);
        match st.agent {
            "claude-code" => claude_line(&line, st),
            "copilot-cli" => copilot_line(&line, st),
            _ => codex_line(&line, st),
        }
    }
    Ok(())
}

/// The file name for Claude Code and Codex; the session folder for Copilot CLI
/// (every Copilot session file is called `events.jsonl`).
fn session_id_of(path: &Path, agent: &str) -> String {
    let p = if agent == "copilot-cli" {
        path.parent().and_then(|d| d.file_name())
    } else {
        path.file_stem()
    };
    p.map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn ts_ms(s: &str) -> Option<i64> {
    // `2026-11-16T03:55:11.726Z`
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-').map(|x| x.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let t: Vec<f64> = time
        .trim_end_matches('Z')
        .split(':')
        .filter_map(|x| x.parse().ok())
        .collect();
    if t.len() < 3 {
        return None;
    }
    // Days from civil (Howard Hinnant).
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400_000 + ((t[0] * 3600.0 + t[1] * 60.0 + t[2]) * 1000.0) as i64)
}

fn note_time(st: &mut FileState, ts: Option<&str>) -> String {
    let day = ts
        .map(|t| t.chars().take(10).collect())
        .unwrap_or_else(|| "unknown".to_string());
    if let Some(ms) = ts.and_then(ts_ms) {
        st.first_at = Some(st.first_at.map_or(ms, |f| f.min(ms)));
        st.last_at = Some(st.last_at.map_or(ms, |l| l.max(ms)));
    }
    day
}

fn num(v: &serde_json::Value, k: &str) -> u64 {
    v.get(k).and_then(|x| x.as_u64()).unwrap_or(0)
}

fn claude_line(line: &str, st: &mut FileState) {
    // Cheap pre-filter: only assistant entries with usage are parsed.
    if !line.contains("\"usage\"") {
        if st.cwd.is_none()
            && line.contains("\"cwd\"")
            && let Ok(v) = serde_json::from_str::<serde_json::Value>(line)
        {
            st.cwd = v.get("cwd").and_then(|c| c.as_str()).map(str::to_string);
        }
        return;
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
        return;
    };
    if st.cwd.is_none() {
        st.cwd = v.get("cwd").and_then(|c| c.as_str()).map(str::to_string);
    }
    if let Some(s) = v.get("sessionId").and_then(|s| s.as_str()) {
        st.session_id = s.to_string();
    }
    let Some(msg) = v.get("message") else { return };
    let Some(u) = msg.get("usage") else { return };
    if v.get("type").and_then(|t| t.as_str()) != Some("assistant") {
        return;
    }
    let id = msg
        .get("id")
        .and_then(|x| x.as_str())
        .or_else(|| v.get("requestId").and_then(|x| x.as_str()));
    if let Some(id) = id
        && !st.seen_ids.insert(id.to_string())
    {
        return;
    }
    let model = msg
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("unknown")
        .to_string();
    if model == "<synthetic>" {
        return;
    }
    let day = note_time(st, v.get("timestamp").and_then(|t| t.as_str()));
    let t = TokenCounts {
        input: num(u, "input_tokens"),
        output: num(u, "output_tokens"),
        cache_write: num(u, "cache_creation_input_tokens"),
        cache_read: num(u, "cache_read_input_tokens"),
        requests: 1,
    };
    st.buckets.entry((day, model)).or_default().add(&t);
}

fn codex_line(line: &str, st: &mut FileState) {
    let interesting = line.contains("\"token_count\"")
        || line.contains("\"session_meta\"")
        || line.contains("\"turn_context\"");
    if !interesting {
        return;
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
        return;
    };
    let p = v.get("payload").cloned().unwrap_or_default();
    match v.get("type").and_then(|t| t.as_str()) {
        Some("session_meta") => {
            st.cwd = p
                .get("cwd")
                .and_then(|c| c.as_str())
                .map(str::to_string)
                .or(st.cwd.take());
            if let Some(id) = p.get("id").and_then(|c| c.as_str()) {
                st.session_id = id.to_string();
            }
        }
        Some("turn_context") => {
            st.codex_model = p.get("model").and_then(|m| m.as_str()).map(str::to_string);
            if let Some(c) = p.get("cwd").and_then(|c| c.as_str()) {
                st.cwd = Some(c.to_string());
            }
        }
        _ if p.get("type").and_then(|t| t.as_str()) == Some("token_count") => {
            let Some(total) = p.pointer("/info/total_token_usage") else {
                return;
            };
            let cached = num(total, "cached_input_tokens");
            // Codex reports input including cached tokens.
            let now = TokenCounts {
                input: num(total, "input_tokens").saturating_sub(cached),
                output: num(total, "output_tokens"),
                cache_write: num(total, "cache_write_input_tokens"),
                cache_read: cached,
                requests: 0,
            };
            let prev = st.codex_last_total;
            if now.total() <= prev.total() {
                return;
            }
            let delta = TokenCounts {
                input: now.input.saturating_sub(prev.input),
                output: now.output.saturating_sub(prev.output),
                cache_write: now.cache_write.saturating_sub(prev.cache_write),
                cache_read: now.cache_read.saturating_sub(prev.cache_read),
                requests: 1,
            };
            st.codex_last_total = now;
            let model = st.codex_model.clone().unwrap_or_else(|| "unknown".into());
            let day = note_time(st, v.get("timestamp").and_then(|t| t.as_str()));
            st.buckets.entry((day, model)).or_default().add(&delta);
        }
        _ => {}
    }
}

fn copilot_line(line: &str, st: &mut FileState) {
    // Only two event types matter; the rest (messages, tool calls) are skipped unparsed.
    let start = line.contains("\"session.start\"");
    if !start && !line.contains("\"session.shutdown\"") {
        return;
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
        return;
    };
    let ts = v.get("timestamp").and_then(|t| t.as_str());
    match v.get("type").and_then(|t| t.as_str()) {
        Some("session.start") => {
            let d = v.get("data").cloned().unwrap_or_default();
            if let Some(c) = d.pointer("/context/cwd").and_then(|c| c.as_str()) {
                st.cwd = Some(c.to_string());
            }
            if let Some(id) = d.get("sessionId").and_then(|c| c.as_str()) {
                st.session_id = id.to_string();
            }
            note_time(st, d.get("startTime").and_then(|t| t.as_str()).or(ts));
        }
        Some("session.shutdown") => {
            let Some(metrics) = v.pointer("/data/modelMetrics").and_then(|m| m.as_object()) else {
                return;
            };
            let day = note_time(st, ts);
            for (model, m) in metrics {
                let u = m.get("usage").cloned().unwrap_or_default();
                let cache_read = num(&u, "cacheReadTokens");
                let cache_write = num(&u, "cacheWriteTokens");
                // `inputTokens` includes both cache buckets.
                let now = TokenCounts {
                    input: num(&u, "inputTokens").saturating_sub(cache_read + cache_write),
                    output: num(&u, "outputTokens"),
                    cache_write,
                    cache_read,
                    requests: m
                        .pointer("/requests/count")
                        .and_then(|c| c.as_u64())
                        .unwrap_or(0),
                };
                let prev = st.copilot_last.get(model).copied().unwrap_or_default();
                // Snapshots are cumulative; if one is smaller, treat it as a fresh total.
                let cumulative = now.input >= prev.input
                    && now.output >= prev.output
                    && now.cache_read >= prev.cache_read
                    && now.cache_write >= prev.cache_write
                    && now.requests >= prev.requests;
                let delta = if cumulative {
                    TokenCounts {
                        input: now.input - prev.input,
                        output: now.output - prev.output,
                        cache_write: now.cache_write - prev.cache_write,
                        cache_read: now.cache_read - prev.cache_read,
                        requests: now.requests - prev.requests,
                    }
                } else {
                    now
                };
                st.copilot_last.insert(model.clone(), now);
                if delta.total() > 0 || delta.requests > 0 {
                    st.buckets
                        .entry((day.clone(), model.clone()))
                        .or_default()
                        .add(&delta);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn copilot_cli_shutdown_totals_count_once_across_resumes() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join(".copilot/session-state/0a1b2c");
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("events.jsonl");
        let shutdown = |ts: &str, input: u64, output: u64, cr: u64, cw: u64, n: u64| {
            format!(
                r#"{{"type":"session.shutdown","timestamp":"{ts}","data":{{"modelMetrics":{{"claude-sonnet-4.5":{{"usage":{{"inputTokens":{input},"outputTokens":{output},"cacheReadTokens":{cr},"cacheWriteTokens":{cw},"reasoningTokens":0}},"requests":{{"count":{n},"cost":1}}}}}}}}}}"#
            )
        };
        let mut w = std::fs::File::create(&f).unwrap();
        writeln!(w, r#"{{"type":"session.start","timestamp":"2026-11-15T09:00:00.000Z","data":{{"sessionId":"0a1b2c","startTime":"2026-11-15T09:00:00.000Z","context":{{"cwd":"C:\\code\\web"}}}}}}"#).unwrap();
        writeln!(w, r#"{{"type":"user.message","timestamp":"2026-11-15T09:00:01.000Z","data":{{"content":"SECRET PROMPT TEXT"}}}}"#).unwrap();
        writeln!(
            w,
            "{}",
            shutdown("2026-11-15T10:00:00.000Z", 1000, 200, 300, 100, 2)
        )
        .unwrap();
        drop(w);
        touch_recent(&f);
        let mut idx = UsageIndex::new();
        let r = idx.refresh(home.path(), 30, NOW);
        let s = &r.sessions[0];
        assert_eq!(s.agent, "copilot-cli");
        assert_eq!(s.session_id, "0a1b2c");
        assert_eq!(s.cwd.as_deref(), Some("C:\\code\\web"));
        assert_eq!(
            s.tokens,
            TokenCounts {
                input: 600,
                output: 200,
                cache_read: 300,
                cache_write: 100,
                requests: 2
            }
        );
        // Resumed the next day: the new shutdown is cumulative; only the change is added.
        let mut w = std::fs::File::options().append(true).open(&f).unwrap();
        writeln!(
            w,
            "{}",
            shutdown("2026-11-16T10:00:00.000Z", 1500, 300, 400, 100, 3)
        )
        .unwrap();
        drop(w);
        touch_recent(&f);
        let r = idx.refresh(home.path(), 30, NOW);
        assert_eq!(r.sessions[0].tokens.total(), 1800);
        assert_eq!(r.sessions[0].tokens.requests, 3);
        let days: Vec<_> = r
            .days
            .iter()
            .map(|d| (d.day.as_str(), d.tokens.total()))
            .collect();
        assert_eq!(days, [("2026-11-15", 1200), ("2026-11-16", 600)]);
        assert!(!format!("{r:?}").contains("SECRET"));
        assert!(
            r.sources
                .iter()
                .any(|s| s.agent == "copilot-cli" && s.found)
        );
    }

    const NOW: i64 = 1_794_800_000_000; // 2026-11-16

    fn claude_line_json(id: &str, ts: &str, input: u64, output: u64, cw: u64, cr: u64) -> String {
        format!(
            r#"{{"type":"assistant","cwd":"/u/web","sessionId":"s-1","timestamp":"{ts}","message":{{"id":"{id}","model":"claude-opus-5-5","content":[{{"type":"text","text":"SECRET PROMPT TEXT"}}],"usage":{{"input_tokens":{input},"output_tokens":{output},"cache_creation_input_tokens":{cw},"cache_read_input_tokens":{cr}}}}}}}"#
        )
    }

    fn touch_recent(p: &Path) {
        let f = std::fs::File::options().append(true).open(p).unwrap();
        f.set_modified(
            std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_millis(NOW as u64),
        )
        .unwrap();
    }

    #[test]
    fn claude_counts_each_message_once_and_reads_incrementally() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join(".claude/projects/-u-web");
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("s-1.jsonl");
        let mut w = std::fs::File::create(&f).unwrap();
        writeln!(
            w,
            r#"{{"type":"user","cwd":"/u/web","message":{{"content":"hello"}}}}"#
        )
        .unwrap();
        // One response split over two lines: counted once.
        writeln!(
            w,
            "{}",
            claude_line_json("m1", "2026-11-15T10:00:00.000Z", 10, 100, 1000, 0)
        )
        .unwrap();
        writeln!(
            w,
            "{}",
            claude_line_json("m1", "2026-11-15T10:00:00.000Z", 10, 100, 1000, 0)
        )
        .unwrap();
        writeln!(
            w,
            "{}",
            claude_line_json("m2", "2026-11-16T09:00:00.000Z", 5, 50, 0, 1000)
        )
        .unwrap();
        drop(w);
        touch_recent(&f);
        let mut idx = UsageIndex::new();
        let r = idx.refresh(home.path(), 30, NOW);
        assert_eq!(r.sessions.len(), 1);
        let s = &r.sessions[0];
        assert_eq!(s.agent, "claude-code");
        assert_eq!(s.cwd.as_deref(), Some("/u/web"));
        assert_eq!(
            s.tokens,
            TokenCounts {
                input: 15,
                output: 150,
                cache_write: 1000,
                cache_read: 1000,
                requests: 2
            }
        );
        assert_eq!(r.days.len(), 2);
        // 15×4 + 150×20 + 1000×4×1.25 + 1000×0.20 per million
        let expected = (15.0 * 4.0 + 150.0 * 20.0 + 1000.0 * 5.0 + 1000.0 * 0.2) / 1e6;
        assert!((s.estimated_cost_usd.unwrap() - expected).abs() < 1e-12);
        assert!(
            !format!("{r:?}").contains("SECRET"),
            "no message text in the report"
        );

        // Appended data is read; earlier lines are not counted again.
        let mut w = std::fs::File::options().append(true).open(&f).unwrap();
        writeln!(
            w,
            "{}",
            claude_line_json("m3", "2026-11-16T09:30:00.000Z", 1, 1, 0, 0)
        )
        .unwrap();
        write!(w, "{{\"type\":\"assistant\",\"partial").unwrap();
        drop(w);
        touch_recent(&f);
        let r = idx.refresh(home.path(), 30, NOW);
        assert_eq!(r.sessions[0].tokens.requests, 3);
        assert_eq!(r.sessions[0].last_at, ts_ms("2026-11-16T09:30:00.000Z"));
    }

    #[test]
    fn codex_running_totals_become_deltas() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join(".codex/sessions/2026/11/16");
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("rollout-x.jsonl");
        let tc = |ts: &str, input: u64, cached: u64, out: u64| {
            format!(
                r#"{{"timestamp":"{ts}","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":{input},"cached_input_tokens":{cached},"output_tokens":{out},"total_tokens":0}}}}}}}}"#
            )
        };
        let lines = [
            r#"{"timestamp":"2026-11-16T08:00:00.000Z","type":"session_meta","payload":{"id":"cx-1","cwd":"/u/api"}}"#.to_string(),
            r#"{"timestamp":"2026-11-16T08:00:01.000Z","type":"turn_context","payload":{"cwd":"/u/api","model":"gpt-x"}}"#.to_string(),
            tc("2026-11-16T08:00:05.000Z", 1000, 0, 100),
            tc("2026-11-16T08:00:05.500Z", 1000, 0, 100), // repeated total: not counted
            tc("2026-11-16T08:01:00.000Z", 3000, 1500, 250),
        ];
        std::fs::write(&f, lines.join("\n") + "\n").unwrap();
        touch_recent(&f);
        let r = UsageIndex::new().refresh(home.path(), 30, NOW);
        let s = &r.sessions[0];
        assert_eq!(s.session_id, "cx-1");
        assert_eq!(s.cwd.as_deref(), Some("/u/api"));
        assert_eq!(
            s.tokens,
            TokenCounts {
                input: 1500,
                output: 250,
                cache_write: 0,
                cache_read: 1500,
                requests: 2
            }
        );
        assert_eq!(s.estimated_cost_usd, None, "no price for unknown models");
    }

    #[test]
    fn hermes_sessions_from_its_database() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".hermes")).unwrap();
        let db = home.path().join(".hermes/state.db");
        let c = rusqlite::Connection::open(&db).unwrap();
        c.execute_batch(
            "CREATE TABLE sessions (id TEXT PRIMARY KEY, source TEXT, model TEXT, system_prompt TEXT, title TEXT, started_at REAL NOT NULL, ended_at REAL,
               input_tokens INTEGER DEFAULT 0, output_tokens INTEGER DEFAULT 0, cache_read_tokens INTEGER DEFAULT 0, cache_write_tokens INTEGER DEFAULT 0,
               cwd TEXT, estimated_cost_usd REAL, actual_cost_usd REAL, api_call_count INTEGER DEFAULT 0);
             INSERT INTO sessions VALUES ('h1','cli','claude-opus-5-5','SECRET SYSTEM PROMPT','SECRET TITLE',1794790000,1794790600,1000,200,5000,0,'/u/bot',NULL,NULL,4);
             INSERT INTO sessions VALUES ('h2','cli','some-model','x','y',1794791000,NULL,10,10,0,0,'/u/bot',0.5,0.42,1);
             INSERT INTO sessions VALUES ('old','cli','m','x','y',1700000000,NULL,10,10,0,0,NULL,NULL,NULL,1);",
        )
        .unwrap();
        drop(c);
        let r = UsageIndex::new().refresh(home.path(), 30, NOW);
        assert_eq!(r.sessions.len(), 2, "{r:?}");
        let h1 = r.sessions.iter().find(|s| s.session_id == "h1").unwrap();
        assert_eq!(
            h1.tokens,
            TokenCounts {
                input: 1000,
                output: 200,
                cache_read: 5000,
                cache_write: 0,
                requests: 4
            }
        );
        assert_eq!(h1.cwd.as_deref(), Some("/u/bot"));
        assert!(
            h1.estimated_cost_usd.is_some(),
            "priced from the model when Hermes has no cost"
        );
        let h2 = r.sessions.iter().find(|s| s.session_id == "h2").unwrap();
        assert_eq!(
            h2.estimated_cost_usd,
            Some(0.42),
            "Hermes' actual cost wins"
        );
        assert!(!format!("{r:?}").contains("SECRET"));
        assert_eq!(date_of(1_794_790_000_000), "2026-11-16");
    }

    #[test]
    fn old_files_are_skipped_and_prices_match_variants() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join(".claude/projects/-u-old");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("old.jsonl"),
            claude_line_json("m", "2025-01-01T00:00:00.000Z", 1, 1, 0, 0) + "\n",
        )
        .unwrap();
        std::fs::File::options()
            .append(true)
            .open(dir.join("old.jsonl"))
            .unwrap()
            .set_modified(
                std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_735_689_600),
            )
            .unwrap();
        let r = UsageIndex::new().refresh(home.path(), 30, NOW);
        assert!(r.sessions.is_empty());
        assert!(
            r.sources
                .iter()
                .any(|s| s.agent == "claude-code" && s.found)
        );
        let t = TokenCounts {
            input: 1_000_000,
            ..Default::default()
        };
        assert_eq!(estimate_cost("claude-opus-5-5", &t), Some(4.0));
        assert_eq!(estimate_cost("claude-opus-4-6-20260101", &t), Some(5.0));
        assert_eq!(estimate_cost("claude-sonnet-5-5[1m]", &t), Some(2.0));
        assert_eq!(
            estimate_cost("claude-sonnet-5", &t),
            Some(2.0),
            "not confused with sonnet-5-5"
        );
        assert_eq!(estimate_cost("gpt-x", &t), None);
    }
}
