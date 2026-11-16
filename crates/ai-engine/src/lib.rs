//! The AI Operator.
//!
//! Workflow for every request: interpret → retrieve machine data through
//! typed, read-only tools → explain → propose actions (never execute) →
//! the user reviews each proposal in the action engine, which re-validates,
//! executes, verifies and audits.
//!
//! Two modes:
//! * **Built-in** (default, no model): a deterministic router answers the
//!   common questions from the same tools. Nothing leaves the machine.
//! * **Model**: a configured provider (local or opt-in cloud) drives a
//!   tool-calling loop. The model can only call the listed tools; there is no
//!   shell, no file access and no way to execute an action.

pub mod builtin;
pub mod providers;
pub mod secrets;
pub mod tools;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use providers::{PRESETS, Preset, ProviderConfig, ProviderKind};
pub use tools::{TOOLS, ToolHost};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ToolStep {
    pub tool: String,
    pub arguments: serde_json::Value,
    /// Short description of what came back (not the full data).
    pub summary: String,
    pub ok: bool,
}

/// A change the Operator suggests. The UI opens the action engine's review
/// dialog for it; the Operator never executes anything.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Proposal {
    /// An `ActionRequest` as JSON (validated again when planned).
    pub request: serde_json::Value,
    pub title: String,
    pub can_execute: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum OperatorMode {
    Builtin,
    Model,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct OperatorAnswer {
    pub answer: String,
    pub steps: Vec<ToolStep>,
    pub proposals: Vec<Proposal>,
    pub mode: OperatorMode,
    /// `provider / model` when a model answered.
    pub model: Option<String>,
    /// Set when machine data was sent to a remote (non-local) endpoint.
    pub data_sent_to: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ChatTurn {
    /// `user` or `assistant`.
    pub role: String,
    pub content: String,
}

/// Run one tool through the host and record the step. Proposals are collected.
pub(crate) fn call_tool(
    host: &dyn ToolHost,
    name: &str,
    args: serde_json::Value,
    steps: &mut Vec<ToolStep>,
    proposals: &mut Vec<Proposal>,
) -> serde_json::Value {
    let known = TOOLS.iter().any(|t| t.name == name);
    let result = if known {
        host.call(name, &args)
    } else {
        Err(format!("unknown tool `{name}`"))
    };
    match result {
        Ok(v) => {
            if name == "propose_action"
                && let Ok(p) = serde_json::from_value::<Proposal>(v.clone())
            {
                proposals.push(p);
            }
            steps.push(ToolStep {
                tool: name.into(),
                summary: tools::summarize(name, &v),
                arguments: args,
                ok: true,
            });
            v
        }
        Err(e) => {
            steps.push(ToolStep {
                tool: name.into(),
                summary: e.clone(),
                arguments: args,
                ok: false,
            });
            serde_json::json!({ "error": e })
        }
    }
}
