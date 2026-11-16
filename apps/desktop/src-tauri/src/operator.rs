//! AI Operator commands and the tool host that answers the Operator's tools
//! from AGASTHY's own data.
//!
//! Every tool is read-only except `propose_action`, which only dry-runs an
//! action to describe it. Nothing the Operator does executes an action: the
//! user opens each proposal in the action engine's review dialog.
//!
//! Tool results show the home directory as `~` and never include the text of
//! AI instruction files. They are what a model sees when one is configured.

use std::sync::Arc;

use agasthy_actions::ActionRequest;
use agasthy_ai::providers::{self, ProviderConfig};
use agasthy_ai::{ChatTurn, OperatorAnswer, Preset, ToolHost, secrets};
use agasthy_cleanup::AnalysisInput;
use agasthy_core::{EntityKind, EntityQuery, RelationKind};
use agasthy_process::RepoRef;
use serde::Serialize;
use serde_json::{Value, json};
use tauri::State;
use ts_rs::TS;

use crate::ops;
use crate::state::{AppState, Inner};

type CmdResult<T> = Result<T, String>;

const PROVIDER_SETTING: &str = "ai.provider";
const CONSENT_SETTING: &str = "ai.cloud_consent";

pub struct Host {
    inner: Arc<Inner>,
    home: String,
}

impl Host {
    pub fn new(inner: Arc<Inner>) -> Self {
        let home = inner
            .platform
            .home_dir()
            .map(|h| h.display().to_string())
            .unwrap_or_default();
        Self { inner, home }
    }

    fn short(&self, v: Value) -> Value {
        if self.home.len() < 2 {
            return v;
        }
        match v {
            Value::String(s) => Value::String(s.replace(&self.home, "~")),
            Value::Array(a) => Value::Array(a.into_iter().map(|x| self.short(x)).collect()),
            Value::Object(o) => {
                Value::Object(o.into_iter().map(|(k, x)| (k, self.short(x))).collect())
            }
            other => other,
        }
    }

    fn expand(&self, p: &str) -> String {
        match p.strip_prefix("~/") {
            Some(rest) if !self.home.is_empty() => format!("{}/{rest}", self.home),
            _ => p.to_string(),
        }
    }

    fn store<T>(
        &self,
        f: impl FnOnce(&agasthy_core::Store) -> agasthy_core::CoreResult<T>,
    ) -> Result<T, String> {
        let s = self
            .inner
            .store
            .lock()
            .map_err(|_| "database lock poisoned")?;
        f(&s).map_err(|e| e.to_string())
    }

    fn entities(&self, kind: EntityKind) -> Result<Vec<agasthy_core::Entity>, String> {
        self.store(|s| {
            s.list_entities(&EntityQuery {
                kind: Some(kind),
                text: None,
                limit: Some(5000),
                offset: None,
            })
        })
    }

    fn telemetry(&self) -> Result<agasthy_system_monitor::TelemetrySnapshot, String> {
        Ok(self
            .inner
            .monitor
            .lock()
            .map_err(|_| "telemetry lock poisoned")?
            .sample())
    }

    fn process_snapshot(&self) -> Result<agasthy_process::ProcessSnapshot, String> {
        let repos: Vec<RepoRef> = ops::repositories(&self.inner)?
            .into_iter()
            .map(|(id, name, path, _)| (id, name, path))
            .collect();
        let mut m = self
            .inner
            .processes
            .lock()
            .map_err(|_| "process monitor lock poisoned")?;
        Ok(m.snapshot(&self.inner.runner, &repos))
    }

    fn process_json(p: &agasthy_process::ProcessInfo) -> Value {
        json!({
            "pid": p.pid,
            "start_time": p.start_time,
            "name": p.name,
            "memory": p.memory_bytes,
            "cpu": p.cpu_percent.value(),
            "project": p.project.as_ref().map(|l| l.name.clone()).unwrap_or_default(),
            "protected": p.protected.clone().unwrap_or_default(),
            "ports": p.ports.iter().map(|x| x.port).collect::<Vec<_>>(),
        })
    }

    fn dispatch(&self, name: &str, args: &Value) -> Result<Value, String> {
        let str_arg = |k: &str| args.get(k).and_then(|v| v.as_str()).map(str::to_string);
        let num_arg = |k: &str| args.get(k).and_then(|v| v.as_u64());
        match name {
            "machine_overview" => {
                let counts = self.store(|s| s.counts_by_kind())?;
                let scans = self.store(|s| s.latest_scans())?;
                let t = self.telemetry()?;
                let mem = t.memory.value();
                let gpus = self
                    .inner
                    .gpus
                    .get_or_init(|| self.inner.platform.gpus(&self.inner.runner))
                    .clone();
                Ok(json!({
                    "counts": counts.iter().map(|c| (c.kind.as_str().to_string(), json!(c.count))).collect::<serde_json::Map<_, _>>(),
                    "latest_scans": scans.iter().map(|s| json!({"kind": s.kind, "status": s.status, "finished_at": s.finished_at, "found": s.entities_found})).collect::<Vec<_>>(),
                    "system": {
                        "cpu_percent": t.cpu.value().map(|c| c.usage_percent),
                        "cpu": t.cpu.value().and_then(|c| c.brand.clone()),
                        "memory_total": mem.map(|m| m.total_bytes),
                        "memory_used": mem.map(|m| m.used_bytes),
                        "memory_available": mem.map(|m| m.available_bytes),
                        "swap_used": mem.map(|m| m.swap_used_bytes),
                        "disks": t.disks.value().map(|d| d.iter().map(|x| json!({"mount": x.mount_point, "total": x.total_bytes, "available": x.available_bytes})).collect::<Vec<_>>()),
                        "gpus": gpus.value().map(|g| g.iter().map(|x| json!({"name": x.model, "vendor": x.vendor, "cores": x.cores, "vram": x.vram})).collect::<Vec<_>>()).unwrap_or_default(),
                    }
                }))
            }
            "top_processes" => {
                let by_cpu = str_arg("by").as_deref() == Some("cpu");
                let limit = num_arg("limit").unwrap_or(10).clamp(1, 25) as usize;
                let mut snap = self.process_snapshot()?;
                if by_cpu && snap.processes.iter().all(|p| !p.cpu_percent.is_available()) {
                    // CPU use is a rate: the first sample only sets the baseline.
                    std::thread::sleep(std::time::Duration::from_millis(1200));
                    snap = self.process_snapshot()?;
                }
                let mut list: Vec<_> = snap
                    .processes
                    .iter()
                    .filter(|p| p.pid != snap.own_pid)
                    .collect();
                if by_cpu {
                    list.sort_by(|a, b| {
                        b.cpu_percent
                            .value()
                            .copied()
                            .unwrap_or(0.0)
                            .total_cmp(&a.cpu_percent.value().copied().unwrap_or(0.0))
                    });
                } else {
                    list.sort_by_key(|p| std::cmp::Reverse(p.memory_bytes));
                }
                let t = self.telemetry()?;
                let mem = t.memory.value();
                Ok(json!({
                    "system": {
                        "memory_total": mem.map(|m| m.total_bytes),
                        "memory_used": mem.map(|m| m.used_bytes),
                        "swap_used": mem.map(|m| m.swap_used_bytes),
                        "cpu_percent": t.cpu.value().map(|c| c.usage_percent),
                    },
                    "processes": list.into_iter().take(limit).map(Self::process_json).collect::<Vec<_>>(),
                }))
            }
            "listening_ports" => {
                let port = num_arg("port");
                let snap = self.process_snapshot()?;
                let list: Vec<Value> = snap
                    .processes
                    .iter()
                    .filter(|p| !p.ports.is_empty())
                    .filter(|p| port.is_none_or(|q| p.ports.iter().any(|x| u64::from(x.port) == q)))
                    .map(Self::process_json)
                    .collect();
                Ok(json!({"port_data": snap.ports.is_available(), "processes": list}))
            }
            "search" => {
                let text = str_arg("text").ok_or("text is required")?;
                let kind: Option<EntityKind> =
                    str_arg("kind").and_then(|k| serde_json::from_value(json!(k)).ok());
                let limit = num_arg("limit").unwrap_or(15).clamp(1, 50) as u32;
                let found = self.store(|s| {
                    s.list_entities(&EntityQuery {
                        kind,
                        text: Some(text.clone()),
                        limit: Some(limit),
                        offset: None,
                    })
                })?;
                Ok(
                    json!({"results": found.iter().map(|e| json!({"id": e.id, "kind": e.kind, "name": e.name, "path": e.path, "version": e.version})).collect::<Vec<_>>()}),
                )
            }
            "entity_details" => {
                let id = str_arg("id").ok_or("id is required")?;
                let d = self
                    .store(|s| s.entity_detail(&id))?
                    .ok_or_else(|| format!("no item with id {id}"))?;
                let mut attrs = d.entity.attributes.clone();
                if let Some(o) = attrs.as_object_mut() {
                    // Instruction-file text stays out of the Operator's view.
                    o.remove("text");
                }
                let rel = |r: &agasthy_core::Relationship| json!({"kind": r.kind, "other": r.other.as_ref().map(|o| json!({"id": o.id, "name": o.name, "kind": o.kind}))});
                Ok(json!({
                    "id": d.entity.id, "kind": d.entity.kind, "name": d.entity.name, "path": d.entity.path,
                    "version": d.entity.version, "source": d.entity.source, "attributes": attrs,
                    "outgoing": d.outgoing.iter().take(40).map(rel).collect::<Vec<_>>(),
                    "incoming": d.incoming.iter().take(40).map(rel).collect::<Vec<_>>(),
                    "evidence_count": d.evidence.len(),
                }))
            }
            "repositories" => {
                let now = agasthy_core::time::now_ms();
                let stale = num_arg("stale_days");
                let mut rows: Vec<(i64, Value)> = Vec::new();
                for e in self.entities(EntityKind::Repository)? {
                    let at = e
                        .attributes
                        .pointer("/head/value/last_commit_at")
                        .and_then(|v| v.as_i64());
                    if let Some(days) = stale {
                        match at {
                            Some(t) if now - t >= days as i64 * 86_400_000 => {}
                            _ => continue,
                        }
                    }
                    let dirty = e
                        .attributes
                        .pointer("/working_tree/value/clean")
                        .and_then(|v| v.as_bool())
                        .map(|c| !c);
                    rows.push((
                        at.unwrap_or(0),
                        json!({
                            "id": e.id, "name": e.name, "path": e.path,
                            "last_commit": at.map(date).unwrap_or_else(|| "unknown".into()),
                            "dirty": dirty,
                            "size": e.attributes.pointer("/size_bytes/value").and_then(|v| v.as_u64()),
                            "language": e.attributes.get("primary_language"),
                        }),
                    ));
                }
                rows.sort_by_key(|(t, _)| *t);
                Ok(json!({"repositories": rows.into_iter().map(|(_, v)| v).collect::<Vec<_>>()}))
            }
            "disk_cleanup_candidates" => {
                let refresh = args
                    .get("refresh")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let cached = self
                    .inner
                    .analysis
                    .lock()
                    .map_err(|_| "analysis lock poisoned")?
                    .clone();
                let a = match cached {
                    Some(a) if !refresh => a,
                    _ => {
                        let input = AnalysisInput {
                            home: ops::home(&self.inner)?,
                            scan_roots: ops::scan_roots(&self.inner)?,
                            repos: ops::repositories(&self.inner)?
                                .into_iter()
                                .map(|(_, _, p, d)| (p, d))
                                .collect(),
                        };
                        let a = agasthy_cleanup::analyze(&input, &self.inner.runner);
                        *self
                            .inner
                            .analysis
                            .lock()
                            .map_err(|_| "analysis lock poisoned")? = Some(a.clone());
                        a
                    }
                };
                let t = self.telemetry()?;
                let free = t.disks.value().and_then(|d| {
                    d.iter()
                        .find(|x| x.mount_point == "/" || x.mount_point == "/System/Volumes/Data")
                        .map(|x| x.available_bytes)
                });
                Ok(json!({
                    "analysed_at": a.analysed_at,
                    "free_bytes": free,
                    "eligible_bytes": a.eligible_bytes,
                    "candidates": a.candidates.iter().map(|c| json!({
                        "path": c.path, "size": c.size_bytes.value(), "category": c.category,
                        "eligible": c.eligible, "blocked_by": c.blocked_by, "reason": c.reason, "project": c.project,
                    })).collect::<Vec<_>>(),
                }))
            }
            "removal_impact" => {
                let id = str_arg("id").ok_or("id is required")?;
                let r = self
                    .store(|s| agasthy_graph::removal_impact(s, &id))?
                    .ok_or_else(|| format!("no item with id {id}"))?;
                Ok(json!({
                    "target": r.target.name,
                    "risk": r.risk,
                    "summary": r.summary,
                    "sections": r.sections.iter().map(|s| json!({
                        "title": s.title,
                        "items": s.entries.iter().take(30).map(|e| format!("{} — {}", e.entity.name, e.explanation)).collect::<Vec<_>>(),
                        "total": s.entries.len(),
                    })).collect::<Vec<_>>(),
                    "unknowns": r.unknowns,
                    "plan": r.plan,
                }))
            }
            "ai_ecosystem" => self.ai_ecosystem(),
            "security_findings" => {
                let mut list = self.entities(EntityKind::Finding)?;
                let rank = |e: &agasthy_core::Entity| match e
                    .attributes
                    .get("severity")
                    .and_then(|s| s.as_str())
                {
                    Some("critical") => 0,
                    Some("high") => 1,
                    Some("medium") => 2,
                    Some("low") => 3,
                    _ => 4,
                };
                list.sort_by_key(rank);
                Ok(json!({"findings": list.iter().take(60).map(|e| json!({
                    "id": e.id,
                    "severity": e.attributes.get("severity"),
                    "category": e.attributes.get("category"),
                    "title": e.attributes.get("summary").and_then(|s| s.as_str()).unwrap_or(&e.name),
                    "packages": e.attributes.get("packages"),
                    "remediation": e.attributes.get("remediation"),
                })).collect::<Vec<_>>(), "total": list.len()}))
            }
            "docker_status" => match agasthy_cleanup::docker::usage(&self.inner.runner) {
                agasthy_core::Availability::Available { value } => Ok(json!({
                    "available": true,
                    "usage": value.rows.iter().map(|r| {
                        let kind = docker_kind(&r.kind);
                        json!({
                            "kind": kind, "count": r.total, "active": r.active,
                            "size": parse_size(&r.size), "reclaimable": parse_size(&r.reclaimable),
                            "dangling": if kind == "images" { value.dangling_images } else { 0 },
                        })
                    }).collect::<Vec<_>>(),
                })),
                other => Ok(json!({"available": false, "reason": unavailable_reason(&other)})),
            },
            "propose_action" => self.propose(args),
            _ => Err(format!("unknown tool `{name}`")),
        }
    }

    fn ai_ecosystem(&self) -> Result<Value, String> {
        let tools: Vec<Value> = self
            .entities(EntityKind::AiTool)?
            .iter()
            .filter(|e| e.attributes.get("installed").and_then(|v| v.as_bool()) != Some(false))
            .map(|e| json!({"id": e.id, "name": e.name, "installed_as": e.attributes.get("category"), "vendor": e.attributes.get("vendor")}))
            .collect();
        let providers: Vec<Value> = self
            .entities(EntityKind::AiProvider)?
            .iter()
            .map(|e| json!({"id": e.id, "name": e.name, "evidence": e.attributes.get("highest").and_then(|v| v.as_str()).unwrap_or("referenced").replace('_', " ")}))
            .collect();
        let mcp: Vec<Value> = self
            .entities(EntityKind::McpServer)?
            .iter()
            .map(|e| {
                let a = &e.attributes;
                json!({"id": e.id, "name": a.get("server").and_then(|v| v.as_str()).unwrap_or(&e.name), "transport": a.get("transport"), "scope": a.get("scope"),
                       "config": a.get("config_file"), "tool": a.get("tool"), "disabled": a.get("disabled"), "env_names": a.get("env_names")})
            })
            .collect();
        let same = self.store(|s| s.edges_of_kind(RelationKind::SameContent, None))?;
        let conflicts = self.store(|s| s.edges_of_kind(RelationKind::PotentialConflict, None))?;
        let files_e = self.entities(EntityKind::AiConfigFile)?;
        let path_of = |id: &str| {
            files_e
                .iter()
                .find(|f| f.id == id)
                .and_then(|f| f.path.clone())
                .unwrap_or_else(|| id.to_string())
        };
        let files: Vec<Value> = files_e
            .iter()
            .map(|f| {
                let dup = same.iter().find(|e| e.from_id == f.id).map(|e| path_of(&e.to_id));
                let n_conf = conflicts.iter().filter(|e| e.from_id == f.id || e.to_id == f.id).count();
                json!({"id": f.id, "path": f.path, "type": f.attributes.get("file_type"), "tools": f.attributes.get("tools"),
                       "orphaned": f.attributes.get("orphaned"), "duplicate_of": dup, "conflicts": n_conf})
            })
            .collect();
        let models: Vec<Value> = self
            .entities(EntityKind::LocalModel)?
            .iter()
            .map(|e| json!({"id": e.id, "name": e.name, "runtime": e.attributes.get("store"), "size": e.attributes.get("size_bytes")}))
            .collect();
        Ok(
            json!({"tools": tools, "providers": providers, "mcp_servers": mcp, "files": files, "local_models": models}),
        )
    }

    fn propose(&self, args: &Value) -> Result<Value, String> {
        let mut req = args.get("request").cloned().ok_or("request is required")?;
        let reason = args
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .chars()
            .take(300)
            .collect::<String>();
        let kind = req
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();
        if !matches!(
            kind.as_str(),
            "terminate_process" | "quarantine_paths" | "docker_prune"
        ) {
            return Err(
                "the Operator can only propose terminate_process, quarantine_paths or docker_prune"
                    .into(),
            );
        }
        if let Some(paths) = req.get_mut("paths").and_then(|p| p.as_array_mut()) {
            for p in paths.iter_mut() {
                if let Some(s) = p.as_str() {
                    *p = Value::String(self.expand(s));
                }
            }
        }
        if kind == "terminate_process" && req.get("force").is_none() {
            req["force"] = json!(false);
        }
        let request: ActionRequest =
            serde_json::from_value(req).map_err(|e| format!("invalid request: {e}"))?;
        // A dry run checks it against the same rules as the review dialog.
        let ctx = ops::context(&self.inner)?;
        let plan = self.inner.engine.plan(&ctx, request.clone());
        Ok(json!({
            "request": request,
            "title": plan.title,
            "can_execute": plan.can_execute,
            "reason": if plan.can_execute { reason } else { plan.warnings.join("; ") },
            "summary": plan.summary,
        }))
    }
}

impl ToolHost for Host {
    fn call(&self, name: &str, args: &Value) -> Result<Value, String> {
        self.dispatch(name, args).map(|v| self.short(v))
    }
}

fn date(ms: i64) -> String {
    let days = ms.div_euclid(86_400_000);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{}-{m:02}-{d:02}", if m <= 2 { y + 1 } else { y })
}

fn docker_kind(k: &str) -> &'static str {
    match k.to_ascii_lowercase().as_str() {
        "images" => "images",
        "containers" => "containers",
        "local volumes" | "volumes" => "volumes",
        "build cache" => "build_cache",
        _ => "other",
    }
}

/// "1.2GB", "512.3MB (40%)", "0B" → bytes (decimal units, as Docker prints them).
fn parse_size(s: &str) -> Option<u64> {
    let s = s.split_whitespace().next()?;
    let idx = s.find(|c: char| !(c.is_ascii_digit() || c == '.'))?;
    let (num, unit) = s.split_at(idx);
    let n: f64 = num.parse().ok()?;
    let mult = match unit.to_ascii_uppercase().as_str() {
        "B" => 1.0,
        "KB" | "K" => 1e3,
        "MB" | "M" => 1e6,
        "GB" | "G" => 1e9,
        "TB" | "T" => 1e12,
        _ => return None,
    };
    Some((n * mult).round() as u64)
}

fn unavailable_reason<T>(a: &agasthy_core::Availability<T>) -> String {
    match a {
        agasthy_core::Availability::Available { .. } => String::new(),
        agasthy_core::Availability::Unavailable { reason } => reason.clone(),
        agasthy_core::Availability::PermissionDenied { detail } => detail.clone(),
        agasthy_core::Availability::Unsupported { platform } => {
            format!("not supported on {platform}")
        }
    }
}

// ----- commands -----

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct OperatorSettings {
    pub presets: Vec<Preset>,
    pub provider: Option<ProviderConfig>,
    pub provider_is_local: bool,
    pub has_key: bool,
    pub cloud_consent: bool,
    pub suggestions: Vec<String>,
}

fn load_provider(inner: &Inner) -> CmdResult<Option<ProviderConfig>> {
    let s = inner.store.lock().map_err(|_| "database lock poisoned")?;
    Ok(s.get_setting(PROVIDER_SETTING)
        .map_err(|e| e.to_string())?
        .and_then(|t| serde_json::from_str(&t).ok()))
}

fn consent(inner: &Inner) -> CmdResult<bool> {
    let s = inner.store.lock().map_err(|_| "database lock poisoned")?;
    Ok(s.get_setting(CONSENT_SETTING)
        .map_err(|e| e.to_string())?
        .as_deref()
        == Some("true"))
}

fn audit(inner: &Inner, action: &str, target: &str, outcome: &str, detail: Option<&str>) {
    if let Ok(s) = inner.store.lock() {
        let _ = s.audit("user", action, target, outcome, detail);
    }
}

#[tauri::command]
pub fn operator_settings(state: State<'_, AppState>) -> CmdResult<OperatorSettings> {
    let provider = load_provider(&state.0)?;
    Ok(OperatorSettings {
        presets: providers::PRESETS.to_vec(),
        provider_is_local: provider.as_ref().is_some_and(|p| p.is_local()),
        has_key: provider
            .as_ref()
            .is_some_and(|p| secrets::has_key(&p.preset)),
        provider,
        cloud_consent: consent(&state.0)?,
        suggestions: agasthy_ai::builtin::SUGGESTIONS
            .iter()
            .map(|s| s.to_string())
            .collect(),
    })
}

/// Save the model provider, or `None` to use only the built-in Operator.
#[tauri::command]
pub fn operator_set_provider(
    state: State<'_, AppState>,
    provider: Option<ProviderConfig>,
) -> CmdResult<()> {
    if let Some(p) = &provider {
        p.validate()?;
        if providers::preset(&p.preset).is_none() {
            return Err("unknown provider".into());
        }
    }
    let s = state.0.store.lock().map_err(|_| "database lock poisoned")?;
    let value = match &provider {
        Some(p) => serde_json::to_string(p).map_err(|e| e.to_string())?,
        None => String::new(),
    };
    s.set_setting(PROVIDER_SETTING, &value)
        .map_err(|e| e.to_string())?;
    let target = provider
        .as_ref()
        .map(|p| format!("{} / {} ({})", p.preset, p.model, p.base_url))
        .unwrap_or_else(|| "built-in".into());
    let _ = s.audit("user", "settings.ai_provider", &target, "ok", None);
    Ok(())
}

#[tauri::command]
pub fn operator_set_key(state: State<'_, AppState>, preset: String, key: String) -> CmdResult<()> {
    let r = secrets::set_key(&preset, &key);
    audit(
        &state.0,
        "settings.ai_key_saved",
        &preset,
        if r.is_ok() { "ok" } else { "error" },
        None,
    );
    r
}

#[tauri::command]
pub fn operator_delete_key(state: State<'_, AppState>, preset: String) -> CmdResult<()> {
    let r = secrets::delete_key(&preset);
    audit(
        &state.0,
        "settings.ai_key_deleted",
        &preset,
        if r.is_ok() { "ok" } else { "error" },
        None,
    );
    r
}

#[tauri::command]
pub fn operator_set_cloud_consent(state: State<'_, AppState>, enabled: bool) -> CmdResult<()> {
    let s = state.0.store.lock().map_err(|_| "database lock poisoned")?;
    s.set_setting(CONSENT_SETTING, if enabled { "true" } else { "false" })
        .map_err(|e| e.to_string())?;
    let _ = s.audit(
        "user",
        if enabled {
            "settings.ai_cloud_on"
        } else {
            "settings.ai_cloud_off"
        },
        "ai",
        "ok",
        None,
    );
    Ok(())
}

/// Answer a question. `use_model` uses the configured provider; otherwise the
/// built-in Operator answers locally.
#[tauri::command]
pub async fn operator_ask(
    state: State<'_, AppState>,
    question: String,
    history: Vec<ChatTurn>,
    use_model: bool,
) -> CmdResult<OperatorAnswer> {
    let question = question.trim().chars().take(4000).collect::<String>();
    if question.is_empty() {
        return Err("ask a question".into());
    }
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let host = Host::new(st.0.clone());
        let mut answer = if use_model {
            let cfg = load_provider(&st.0)?.ok_or("no model is configured; set one up in Settings → AI model")?;
            if !cfg.is_local() && !consent(&st.0)? {
                return Err("sending machine data to a cloud model is off; turn it on in Settings → AI model".into());
            }
            let key = secrets::get_key(&cfg.preset)?;
            let work = st.0.data_dir.join("tmp");
            let r = providers::ask_model(&cfg, key.as_deref(), &history, &question, &host, &st.0.runner, &work);
            let detail = match &r {
                Ok(a) => format!("model {}; {} tool calls; data sent to {}", cfg.model, a.steps.len(), a.data_sent_to.as_deref().unwrap_or("this machine only")),
                Err(e) => e.clone(),
            };
            audit(&st.0, "operator.ask", &cfg.preset, if r.is_ok() { "ok" } else { "error" }, Some(&detail));
            r?
        } else {
            let a = agasthy_ai::builtin::answer(&question, &host);
            audit(&st.0, "operator.ask", "built-in", "ok", Some(&format!("{} tool calls; nothing sent", a.steps.len())));
            a
        };
        // Proposals carry real paths for the review dialog.
        for p in &mut answer.proposals {
            if let Some(paths) = p.request.get_mut("paths").and_then(|x| x.as_array_mut()) {
                for x in paths.iter_mut() {
                    if let Some(s) = x.as_str() {
                        *x = Value::String(host.expand(s));
                    }
                }
            }
        }
        Ok(answer)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(1_700_000_000_000), "2023-11-14");
        assert_eq!(parse_size("1.5GB"), Some(1_500_000_000));
        assert_eq!(parse_size("512.3MB (40%)"), Some(512_300_000));
        assert_eq!(parse_size("0B"), Some(0));
        assert_eq!(docker_kind("Build Cache"), "build_cache");
        assert_eq!(docker_kind("Local Volumes"), "volumes");
    }
}

/// Manual check against a copy of a real database (never the live one):
/// `AGASTHY_LIVE_DATA=/path/to/copy cargo test -p agasthy-desktop live_builtin -- --ignored --nocapture`
#[cfg(test)]
mod live {
    #[test]
    #[ignore]
    fn live_builtin() {
        let Ok(dir) = std::env::var("AGASTHY_LIVE_DATA") else {
            return;
        };
        let st = crate::state::AppState::open(dir.into()).unwrap();
        let host = super::Host::new(st.0.clone());
        let qs = std::env::var("AGASTHY_LIVE_QUESTIONS")
            .unwrap_or_else(|_| agasthy_ai::builtin::SUGGESTIONS.join("\n"));
        for q in qs.lines() {
            let a = agasthy_ai::builtin::answer(q, &host);
            println!("\n===== {q}\n{}", a.answer);
            for s in &a.steps {
                println!(
                    "  [{}] {} -> {}",
                    if s.ok { "ok" } else { "ERR" },
                    s.tool,
                    s.summary
                );
            }
            for p in &a.proposals {
                println!(
                    "  PROPOSAL: {} (can_execute={}) {}",
                    p.title, p.can_execute, p.reason
                );
            }
        }
    }
}
