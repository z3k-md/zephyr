//! The permission bridge. Each `claude -p` run gets a PermissionRequest HTTP hook pointing at
//! this listener on 127.0.0.1 with a per-job token. A request waits here, shown in the bar
//! and as a notification, until the user allows or denies it, or the approval times out.

use std::collections::HashMap;
use std::io::Read;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager};

use super::{Status, describe_tool, job_project, notify, set_status};
use crate::state::AppState;

static PORT: AtomicU16 = AtomicU16::new(0);

pub fn port() -> u16 {
    PORT.load(Ordering::SeqCst)
}

#[derive(Debug, Clone)]
pub enum Decision {
    Allow,
    /// Allow, and keep allowing this kind of call in the project.
    AllowAlways,
    Deny(String),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Approval {
    pub id: String,
    pub job_id: String,
    pub project: String,
    pub tool: String,
    /// "Running npm test", "Editing src/a.ts"…
    pub summary: String,
    /// The command, or the edit, for the card's preview.
    pub detail: String,
    pub created: i64,
}

struct Pending {
    approval: Approval,
    reply: Sender<Decision>,
    suggestions: Value,
    tool_input: Value,
}

#[derive(Default)]
struct Bridge {
    tokens: HashMap<String, String>,
    pending: Vec<Pending>,
}

fn bridge() -> &'static Mutex<Bridge> {
    static BRIDGE: OnceLock<Mutex<Bridge>> = OnceLock::new();
    BRIDGE.get_or_init(|| Mutex::new(Bridge::default()))
}

/// A fresh token for a job's run; only that run's hook can answer for it.
pub fn register(job_id: &str) -> String {
    let token = uuid::Uuid::new_v4().simple().to_string();
    if let Ok(mut bridge) = bridge().lock() {
        bridge.tokens.insert(job_id.to_string(), token.clone());
    }
    token
}

pub fn unregister(job_id: &str) {
    if let Ok(mut bridge) = bridge().lock() {
        bridge.tokens.remove(job_id);
    }
    deny_all_for(job_id, "The run ended");
}

pub fn waiting(job_id: &str) -> bool {
    bridge()
        .lock()
        .map(|bridge| {
            bridge
                .pending
                .iter()
                .any(|pending| pending.approval.job_id == job_id)
        })
        .unwrap_or(false)
}

/// Pending approvals, oldest first.
pub fn approvals() -> Vec<Approval> {
    bridge()
        .lock()
        .map(|bridge| {
            bridge
                .pending
                .iter()
                .map(|pending| pending.approval.clone())
                .collect()
        })
        .unwrap_or_default()
}

pub fn deny_all_for(job_id: &str, reason: &str) {
    if let Ok(mut bridge) = bridge().lock() {
        bridge.pending.retain(|pending| {
            if pending.approval.job_id == job_id {
                let _ = pending.reply.send(Decision::Deny(reason.to_string()));
                false
            } else {
                true
            }
        });
    }
}

/// Answers an approval. "Always" also adds the rule to the project's allow list in Zephyr's
/// settings (never in the repo).
pub fn answer(app: &AppHandle, approval_id: &str, decision: Decision) -> Result<(), String> {
    let pending = {
        let mut bridge = bridge().lock().map_err(|_| "Approvals are unavailable")?;
        let index = bridge
            .pending
            .iter()
            .position(|pending| pending.approval.id == approval_id)
            .ok_or("That request already ended")?;
        bridge.pending.remove(index)
    };
    if matches!(decision, Decision::AllowAlways) {
        let rules = rules_for(
            &pending.approval.tool,
            &pending.tool_input,
            &pending.suggestions,
        );
        if let Some((project_id, _)) = job_project(&pending.approval.job_id) {
            let state = app.state::<AppState>();
            let result = state.update(|persisted| {
                if let Some(project) = persisted
                    .claude
                    .projects
                    .iter_mut()
                    .find(|project| project.id == project_id)
                {
                    for rule in &rules {
                        if !project.allow.contains(rule) {
                            project.allow.push(rule.clone());
                        }
                    }
                }
                Ok(())
            });
            if let Ok(snapshot) = result {
                let _ = app.emit("state-changed", &snapshot);
            }
        }
    }
    let _ = pending.reply.send(decision);
    let _ = app.emit("claude-changed", ());
    Ok(())
}

/// The allow rules an "Always" answer adds: Claude Code's own suggestions when it sent any,
/// otherwise the tool itself, narrowed to the command's first word for the shell.
fn rules_for(tool: &str, input: &Value, suggestions: &Value) -> Vec<String> {
    let mut rules = Vec::new();
    for suggestion in suggestions.as_array().into_iter().flatten() {
        if suggestion["type"] != "addRules" || suggestion["behavior"] != "allow" {
            continue;
        }
        for rule in suggestion["rules"].as_array().into_iter().flatten() {
            let name = rule["toolName"].as_str().unwrap_or(tool);
            match rule["ruleContent"].as_str() {
                Some(content) if !content.is_empty() => rules.push(format!("{name}({content})")),
                _ => rules.push(name.to_string()),
            }
        }
    }
    if rules.is_empty() {
        let command = input["command"].as_str().unwrap_or_default();
        let word = command.split_whitespace().next().unwrap_or_default();
        rules.push(if tool == "Bash" && !word.is_empty() {
            format!("Bash({word} *)")
        } else {
            tool.to_string()
        });
    }
    rules
}

fn response(decision: &Decision, suggestions: &Value) -> Value {
    let decision = match decision {
        Decision::Allow => json!({ "behavior": "allow" }),
        Decision::AllowAlways => {
            let updates: Vec<Value> = suggestions
                .as_array()
                .into_iter()
                .flatten()
                .map(|suggestion| {
                    let mut suggestion = suggestion.clone();
                    suggestion["destination"] = json!("session");
                    suggestion
                })
                .collect();
            if updates.is_empty() {
                json!({ "behavior": "allow" })
            } else {
                json!({ "behavior": "allow", "updatedPermissions": updates })
            }
        }
        Decision::Deny(reason) => json!({
            "behavior": "deny",
            "message": if reason.is_empty() { "The user denied this." } else { reason.as_str() },
        }),
    };
    json!({ "hookSpecificOutput": { "hookEventName": "PermissionRequest", "decision": decision } })
}

/// Binds the listener on a random local port and serves it on its own threads.
pub fn start(app: AppHandle) {
    let server = match tiny_http::Server::http("127.0.0.1:0") {
        Ok(server) => server,
        Err(err) => {
            log::error!("couldn't start the Claude permission listener: {err}");
            return;
        }
    };
    if let Some(addr) = server.server_addr().to_ip() {
        PORT.store(addr.port(), Ordering::SeqCst);
    }
    std::thread::Builder::new()
        .name("claude-hook".into())
        .spawn(move || {
            for request in server.incoming_requests() {
                let app = app.clone();
                std::thread::spawn(move || handle(&app, request));
            }
        })
        .ok();
}

fn handle(app: &AppHandle, mut request: tiny_http::Request) {
    let reply = |request: tiny_http::Request, code: u16, body: String| {
        let header = tiny_http::Header::from_bytes("Content-Type", "application/json")
            .expect("static header");
        let _ = request.respond(
            tiny_http::Response::from_string(body)
                .with_status_code(code)
                .with_header(header),
        );
    };
    let Some(job_id) = request
        .url()
        .strip_prefix("/claude/permission/")
        .map(str::to_string)
    else {
        return reply(request, 404, "{}".into());
    };
    let token = request
        .headers()
        .iter()
        .find(|header| header.field.equiv("Authorization"))
        .map(|header| {
            header
                .value
                .as_str()
                .trim_start_matches("Bearer ")
                .to_string()
        });
    let expected = bridge()
        .lock()
        .ok()
        .and_then(|bridge| bridge.tokens.get(&job_id).cloned());
    if expected.is_none() || token != expected {
        return reply(request, 403, "{}".into());
    }

    let mut body = String::new();
    if request
        .as_reader()
        .take(1 << 20)
        .read_to_string(&mut body)
        .is_err()
    {
        return reply(request, 400, "{}".into());
    }
    let payload: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    let tool = payload["tool_name"]
        .as_str()
        .unwrap_or("a tool")
        .to_string();
    let input = payload["tool_input"].clone();
    let suggestions = payload["permission_suggestions"].clone();
    let detail = match tool.as_str() {
        "Bash" => input["command"].as_str().unwrap_or_default().to_string(),
        "Edit" | "MultiEdit" => input["new_string"].as_str().unwrap_or_default().to_string(),
        "Write" => input["content"].as_str().unwrap_or_default().to_string(),
        "WebFetch" => input["url"].as_str().unwrap_or_default().to_string(),
        _ => serde_json::to_string_pretty(&input).unwrap_or_default(),
    };
    let project = job_project(&job_id)
        .map(|(_, name)| name)
        .unwrap_or_default();
    let approval = Approval {
        id: format!("approval-{}", uuid::Uuid::new_v4().simple()),
        job_id: job_id.clone(),
        project: project.clone(),
        summary: describe_tool(&tool, &input),
        tool: tool.clone(),
        detail: detail.chars().take(4000).collect(),
        created: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or(0),
    };
    let (sender, receiver) = mpsc::channel();
    if let Ok(mut bridge) = bridge().lock() {
        bridge.pending.push(Pending {
            approval: approval.clone(),
            reply: sender,
            suggestions: suggestions.clone(),
            tool_input: input,
        });
    }
    set_status(app, &job_id, Status::Waiting);
    notify(
        app,
        &format!("? {project} · {}", approval.summary),
        "Open Zephyr to allow or deny",
    );

    let minutes = app
        .state::<AppState>()
        .snapshot()
        .map(|snapshot| snapshot.claude.approval_minutes)
        .unwrap_or(30);
    // Answer a little before Claude Code's own hook timeout so the reason reaches it.
    let wait = Duration::from_secs(u64::from(minutes) * 60).saturating_sub(Duration::from_secs(5));
    let decision = receiver
        .recv_timeout(wait)
        .unwrap_or_else(|_| Decision::Deny(format!("No answer within {minutes} minutes")));
    if let Ok(mut bridge) = bridge().lock() {
        bridge
            .pending
            .retain(|pending| pending.approval.id != approval.id);
    }
    if !waiting(&job_id) {
        set_status(app, &job_id, Status::Running);
    }
    let _ = app.emit("claude-changed", ());
    reply(request, 200, response(&decision, &suggestions).to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn always_rules_come_from_suggestions_or_the_command() {
        let suggestions = json!([{"type":"addRules","behavior":"allow","destination":"localSettings","rules":[{"toolName":"Bash","ruleContent":"npm test:*"}]}]);
        assert_eq!(
            rules_for("Bash", &json!({}), &suggestions),
            vec!["Bash(npm test:*)"]
        );
        assert_eq!(
            rules_for(
                "Bash",
                &json!({"command":"cargo build --release"}),
                &Value::Null
            ),
            vec!["Bash(cargo *)"]
        );
        assert_eq!(
            rules_for("WebFetch", &json!({}), &Value::Null),
            vec!["WebFetch"]
        );
    }

    #[test]
    fn responses_match_the_hook_contract() {
        let allow = response(&Decision::Allow, &Value::Null);
        assert_eq!(allow["hookSpecificOutput"]["decision"]["behavior"], "allow");
        let deny = response(&Decision::Deny("not now".into()), &Value::Null);
        assert_eq!(deny["hookSpecificOutput"]["decision"]["message"], "not now");
        let always = response(
            &Decision::AllowAlways,
            &json!([{"type":"addRules","destination":"localSettings","rules":[]}]),
        );
        assert_eq!(
            always["hookSpecificOutput"]["decision"]["updatedPermissions"][0]["destination"],
            "session"
        );
    }
}
