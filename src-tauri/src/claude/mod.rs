//! Background Claude Code jobs: `!claude <task>` runs the user's own `claude` CLI headless in
//! a registered project folder, one `claude -p` process per turn. Follow-ups resume the same
//! session. Permission prompts reach Zephyr through an HTTP hook ([`hook`]) and wait for the
//! user. Nothing here knows about any particular project, language or app.

pub mod binary;
pub mod hook;

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;

// Settings

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    /// Edits inside the folder run; other shell commands and network ask.
    #[default]
    Edit,
    /// Claude Code's classifier decides; commit and push still ask.
    Auto,
    /// Reads only and returns a plan.
    Plan,
}

impl Profile {
    fn mode(self) -> &'static str {
        match self {
            Profile::Edit => "acceptEdits",
            Profile::Auto => "auto",
            Profile::Plan => "plan",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Project {
    pub id: String,
    pub folder: String,
    /// What follows `!claude` to pick this project, e.g. `zephyr`.
    pub alias: String,
    pub profile: Profile,
    /// Permission rules this project always allows, grown by "Always" answers.
    pub allow: Vec<String>,
    pub model: String,
    pub effort: String,
    /// Appended to the brief Claude gets for every job here.
    pub note: String,
}

impl Project {
    pub fn name(&self) -> String {
        if !self.alias.is_empty() {
            return self.alias.clone();
        }
        Path::new(&self.folder)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.folder.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ClaudeSettings {
    /// Path to the CLI; empty finds it automatically.
    pub binary: String,
    pub projects: Vec<Project>,
    /// The project a task without an alias goes to.
    pub last_project: String,
    pub max_turns: u32,
    /// Minutes a turn may run, not counting time waiting on an approval.
    pub timeout_minutes: u32,
    /// Minutes an approval waits before it is denied.
    pub approval_minutes: u32,
    pub notifications: bool,
}

impl Default for ClaudeSettings {
    fn default() -> Self {
        Self {
            binary: String::new(),
            projects: Vec::new(),
            last_project: String::new(),
            max_turns: 80,
            timeout_minutes: 30,
            approval_minutes: 30,
            notifications: true,
        }
    }
}

impl ClaudeSettings {
    pub fn normalized(mut self) -> Result<Self, String> {
        let mut seen_aliases = Vec::<String>::new();
        let mut seen_folders = Vec::<String>::new();
        for project in &mut self.projects {
            // Same rules as file search folders: ~ is the home folder on every OS (HOME or
            // USERPROFILE), and the path must be an existing absolute folder.
            project.folder =
                crate::files::normalize_folders(std::slice::from_ref(&project.folder))?
                    .pop()
                    .ok_or("Choose a folder for the project")?;
            if seen_folders.contains(&project.folder) {
                return Err(format!("{} is already a project", project.folder));
            }
            seen_folders.push(project.folder.clone());
            project.alias = project.alias.trim().trim_start_matches('!').to_lowercase();
            if !project.alias.is_empty() {
                if !crate::destination::is_trigger(&project.alias) {
                    return Err("An alias can only use letters, numbers and hyphens".into());
                }
                if seen_aliases.contains(&project.alias) {
                    return Err(format!("Two projects use the alias {}", project.alias));
                }
                seen_aliases.push(project.alias.clone());
            }
            if project.id.is_empty() {
                project.id = format!("project-{}", uuid::Uuid::new_v4().simple());
            }
            project.allow.retain(|rule| !rule.trim().is_empty());
            project.note = project.note.trim().to_string();
        }
        self.max_turns = self.max_turns.clamp(1, 500);
        self.timeout_minutes = self.timeout_minutes.clamp(1, 600);
        self.approval_minutes = self.approval_minutes.clamp(1, 120);
        Ok(self)
    }
}

// Jobs

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Queued,
    Running,
    Waiting,
    Done,
    Failed,
    Cancelled,
    /// Zephyr quit while it ran.
    Interrupted,
}

impl Status {
    fn active(self) -> bool {
        matches!(self, Status::Running | Status::Waiting)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    pub prompt: String,
    pub started: Option<i64>,
    pub ended: Option<i64>,
    /// The full final message.
    pub result: String,
    pub denials: Vec<String>,
    pub exit: Option<i32>,
    /// `git rev-parse HEAD` before the turn, for "what changed".
    pub head_before: Option<String>,
    /// `git stash create` before the turn: a commit of the dirty tree that touches nothing.
    pub snapshot: Option<String>,
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    /// The Claude Code session, chosen by Zephyr so follow-ups can resume it.
    pub session: String,
    pub project_id: String,
    pub project: String,
    pub folder: String,
    pub title: String,
    pub status: Status,
    /// What it's doing now, e.g. "Editing src/list.ts".
    pub activity: String,
    /// One line for notifications and the job list.
    pub summary: String,
    pub created: i64,
    pub updated: i64,
    pub turns: Vec<Turn>,
}

impl Job {
    fn pending_turn(&self) -> bool {
        self.turns.last().is_some_and(|turn| turn.started.is_none())
    }
}

struct Store {
    jobs: Vec<Job>,
    dir: PathBuf,
    /// The live process for each running job.
    children: HashMap<String, u32>,
    cancelled: Vec<String>,
}

fn store() -> &'static Mutex<Store> {
    static STORE: OnceLock<Mutex<Store>> = OnceLock::new();
    STORE.get_or_init(|| {
        Mutex::new(Store {
            jobs: Vec::new(),
            dir: PathBuf::new(),
            children: HashMap::new(),
            cancelled: Vec::new(),
        })
    })
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

impl Store {
    fn save(&self) {
        let path = self.dir.join("jobs.json");
        let result = serde_json::to_vec_pretty(&self.jobs)
            .map_err(|err| err.to_string())
            .and_then(|data| {
                fs::create_dir_all(&self.dir).map_err(|err| err.to_string())?;
                let tmp = path.with_extension("json.tmp");
                fs::write(&tmp, data).map_err(|err| err.to_string())?;
                fs::rename(&tmp, &path).map_err(|err| err.to_string())
            });
        if let Err(err) = result {
            log::error!("couldn't save Claude jobs: {err}");
        }
    }

    fn job_mut(&mut self, id: &str) -> Option<&mut Job> {
        self.jobs.iter_mut().find(|job| job.id == id)
    }
}

/// Loads saved jobs; any that were running when Zephyr quit become "interrupted".
pub fn init(app: &AppHandle, dir: PathBuf) {
    let Ok(mut store) = store().lock() else {
        return;
    };
    store.dir = dir.clone();
    if let Ok(bytes) = fs::read(dir.join("jobs.json"))
        && let Ok(jobs) = serde_json::from_slice::<Vec<Job>>(&bytes)
    {
        store.jobs = jobs;
    }
    for job in &mut store.jobs {
        if job.status.active() {
            job.status = Status::Interrupted;
            job.activity = "Zephyr quit while this ran".into();
        }
    }
    store.save();
    drop(store);
    hook::start(app.clone());
    pump(app);
}

fn changed(app: &AppHandle) {
    let _ = app.emit("claude-changed", ());
}

fn settings(app: &AppHandle) -> ClaudeSettings {
    app.state::<AppState>()
        .snapshot()
        .map(|snapshot| snapshot.claude)
        .unwrap_or_default()
}

pub fn jobs() -> Vec<Job> {
    let mut jobs = store()
        .lock()
        .map(|store| store.jobs.clone())
        .unwrap_or_default();
    jobs.sort_by_key(|job| std::cmp::Reverse(job.created));
    jobs
}

/// Starts a new job in `project`, or queues it behind that folder's running job.
pub fn submit(app: &AppHandle, project_id: &str, prompt: &str) -> Result<String, String> {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err("Say what Claude should do".into());
    }
    let config = settings(app);
    let project = config
        .projects
        .iter()
        .find(|project| project.id == project_id)
        .ok_or("That project isn't set up in Zephyr")?;
    let now = now_secs();
    let id = format!("job-{}", uuid::Uuid::new_v4().simple());
    let job = Job {
        id: id.clone(),
        session: uuid::Uuid::new_v4().to_string(),
        project_id: project.id.clone(),
        project: project.name(),
        folder: project.folder.clone(),
        title: title_of(prompt),
        status: Status::Queued,
        activity: "Queued".into(),
        summary: String::new(),
        created: now,
        updated: now,
        turns: vec![new_turn(prompt)],
    };
    {
        let mut store = store().lock().map_err(|_| "Jobs are unavailable")?;
        store.jobs.push(job);
        store.save();
    }
    remember_project(app, project_id);
    changed(app);
    pump(app);
    Ok(id)
}

/// Continues a job's session with another prompt; queued if it is still running.
pub fn follow_up(app: &AppHandle, job_id: &str, prompt: &str) -> Result<(), String> {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err("Say what Claude should do next".into());
    }
    {
        let mut store = store().lock().map_err(|_| "Jobs are unavailable")?;
        let job = store.job_mut(job_id).ok_or("That job is gone")?;
        if job.pending_turn() {
            return Err("That job already has a follow-up waiting".into());
        }
        job.turns.push(new_turn(prompt));
        if !job.status.active() {
            job.status = Status::Queued;
            job.activity = "Queued".into();
        }
        job.updated = now_secs();
        store.save();
    }
    changed(app);
    pump(app);
    Ok(())
}

pub fn cancel(app: &AppHandle, job_id: &str) -> Result<(), String> {
    let mut store = store().lock().map_err(|_| "Jobs are unavailable")?;
    if let Some(pid) = store.children.get(job_id).copied() {
        store.cancelled.push(job_id.to_string());
        interrupt(pid);
    } else if let Some(job) = store.job_mut(job_id)
        && job.status == Status::Queued
    {
        job.turns.retain(|turn| turn.started.is_some());
        job.status = if job.turns.is_empty() {
            Status::Cancelled
        } else {
            Status::Done
        };
        job.activity = "Cancelled".into();
        store.save();
    }
    drop(store);
    hook::deny_all_for(job_id, "The user cancelled this job");
    changed(app);
    Ok(())
}

pub fn remove(app: &AppHandle, job_id: &str) -> Result<(), String> {
    let mut store = store().lock().map_err(|_| "Jobs are unavailable")?;
    if store.children.contains_key(job_id) {
        return Err("Cancel the job before removing it".into());
    }
    store.jobs.retain(|job| job.id != job_id);
    let _ = fs::remove_file(store.dir.join("jobs").join(format!("{job_id}.ndjson")));
    store.save();
    drop(store);
    changed(app);
    Ok(())
}

#[cfg(unix)]
fn interrupt(pid: u32) {
    // SIGINT lets Claude Code end the turn and save the session cleanly.
    unsafe {
        libc::kill(pid as i32, libc::SIGINT);
    }
}

#[cfg(windows)]
fn interrupt(pid: u32) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let _ = std::process::Command::new("taskkill")
        .args(["/T", "/F", "/PID", &pid.to_string()])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
}

fn new_turn(prompt: &str) -> Turn {
    Turn {
        prompt: prompt.to_string(),
        started: None,
        ended: None,
        result: String::new(),
        denials: Vec::new(),
        exit: None,
        head_before: None,
        snapshot: None,
        mode: String::new(),
    }
}

fn title_of(prompt: &str) -> String {
    let line = prompt.lines().next().unwrap_or(prompt).trim();
    let mut title: String = line.chars().take(60).collect();
    if line.chars().count() > 60 {
        title.push('…');
    }
    title
}

fn remember_project(app: &AppHandle, project_id: &str) {
    let state = app.state::<AppState>();
    let result = state.update(|persisted| {
        persisted.claude.last_project = project_id.to_string();
        Ok(())
    });
    if let Ok(snapshot) = result {
        let _ = app.emit("state-changed", &snapshot);
    }
}

/// Starts the next waiting turn in every folder that has nothing running.
fn pump(app: &AppHandle) {
    let Ok(mut store) = store().lock() else {
        return;
    };
    let busy: Vec<String> = store
        .jobs
        .iter()
        .filter(|job| job.status.active())
        .map(|job| job.folder.clone())
        .collect();
    let mut started_folders = busy;
    let mut to_start = Vec::new();
    let mut order: Vec<usize> = (0..store.jobs.len()).collect();
    order.sort_by_key(|&index| store.jobs[index].updated);
    for index in order {
        let job = &mut store.jobs[index];
        if job.status != Status::Queued || !job.pending_turn() {
            continue;
        }
        if started_folders.contains(&job.folder) {
            continue;
        }
        started_folders.push(job.folder.clone());
        job.status = Status::Running;
        job.activity = "Starting".into();
        to_start.push(job.id.clone());
    }
    if !to_start.is_empty() {
        store.save();
    }
    drop(store);
    for id in to_start {
        let app = app.clone();
        std::thread::spawn(move || run_turn(&app, &id));
    }
    changed(app);
}

const BRIEF: &str = "You are running unattended, started from the Zephyr launcher. The user \
approves tool permissions from a notification but can't answer questions mid-run, so make \
reasonable choices and say which you made. If you are blocked on a decision only the user can \
make, stop and put the question as the last line. Never commit, push, switch branches, stash, \
reset, or delete untracked files unless this message explicitly asks you to. End with one line \
of 100 characters or fewer summarizing the outcome, then list the files you changed and \
anything the user must do next.";

/// Rules that are never allowed, whatever the profile or an approval says.
const HARD_DENY: &[&str] = &[
    "Bash(git reset *)",
    "Bash(git checkout *)",
    "Bash(git switch *)",
    "Bash(git restore *)",
    "Bash(git clean *)",
    "Bash(git stash *)",
    "Bash(git rebase *)",
    "Bash(git merge *)",
    "Bash(git branch *)",
    "Bash(git tag *)",
    "Bash(git push --force*)",
    "Bash(git push -f*)",
    "Bash(git -C *)",
    "Bash(rm -rf *)",
];

fn run_turn(app: &AppHandle, job_id: &str) {
    let config = settings(app);
    let Some(job) = store()
        .lock()
        .ok()
        .and_then(|store| store.jobs.iter().find(|job| job.id == job_id).cloned())
    else {
        return;
    };
    let project = config
        .projects
        .iter()
        .find(|project| project.id == job.project_id)
        .cloned();
    let outcome = match (binary::resolve(&config.binary), project) {
        (None, _) => Err(
            "Zephyr couldn't find the claude command. Set its path in Settings > Claude."
                .to_string(),
        ),
        (_, None) => Err("This job's project was removed from Zephyr".to_string()),
        (Some(binary), Some(project)) => spawn_turn(app, &config, &project, &binary, &job),
    };
    if let Err(message) = outcome {
        finish(app, job_id, None, Some(message), String::new(), Vec::new());
    }
    pump(app);
}

fn spawn_turn(
    app: &AppHandle,
    config: &ClaudeSettings,
    project: &Project,
    binary: &Path,
    job: &Job,
) -> Result<(), String> {
    let first = job
        .turns
        .iter()
        .filter(|turn| turn.started.is_some())
        .count()
        == 0;
    let prompt = job
        .turns
        .last()
        .map(|turn| turn.prompt.clone())
        .unwrap_or_default();
    let folder = Path::new(&project.folder);
    let (head_before, snapshot) = (
        git(folder, &["rev-parse", "HEAD"]),
        git(folder, &["stash", "create"]),
    );

    let dir = store()
        .lock()
        .map(|store| store.dir.clone())
        .map_err(|_| "Jobs are unavailable")?;
    let token = hook::register(&job.id);
    let settings_path = dir.join("runs").join(format!("{}.json", job.id));
    let settings_json = json!({
        "permissions": {
            "blockReadsOutsideWorkingDirectories": true,
            "ask": ["Bash(git commit *)", "Bash(git push *)"],
        },
        "hooks": {
            "PermissionRequest": [{
                "hooks": [{
                    "type": "http",
                    "url": format!("http://127.0.0.1:{}/claude/permission/{}", hook::port(), job.id),
                    "timeout": config.approval_minutes * 60,
                    "headers": { "Authorization": "Bearer $ZEPHYR_HOOK_TOKEN" },
                    "allowedEnvVars": ["ZEPHYR_HOOK_TOKEN"],
                }],
            }],
        },
    });
    fs::create_dir_all(settings_path.parent().unwrap_or(&dir)).map_err(|err| err.to_string())?;
    fs::write(&settings_path, settings_json.to_string()).map_err(|err| err.to_string())?;

    let mut brief = BRIEF.to_string();
    if !project.note.is_empty() {
        brief.push_str("\n\nNotes from the user about this project:\n");
        brief.push_str(&project.note);
    }

    let mut command = binary::command(binary);
    command
        .current_dir(folder)
        .arg("-p")
        .args(if first {
            ["--session-id", job.session.as_str()]
        } else {
            ["--resume", job.session.as_str()]
        })
        .args(["--output-format", "stream-json", "--verbose"])
        .args(["--permission-mode", project.profile.mode()])
        .args(["--permission-prompts", "none"])
        .arg("--settings")
        .arg(&settings_path)
        .arg("--append-system-prompt")
        .arg(&brief)
        .args(["--max-turns", &config.max_turns.to_string()])
        .arg("--name")
        .arg(format!("zephyr: {}", job.title))
        .arg("--disallowedTools")
        .args(HARD_DENY);
    if !project.allow.is_empty() {
        command.arg("--allowedTools").args(&project.allow);
    }
    if !project.model.is_empty() {
        command.args(["--model", &project.model]);
    }
    if !project.effort.is_empty() {
        command.args(["--effort", &project.effort]);
    }
    command
        .env("ZEPHYR_HOOK_TOKEN", &token)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child: Child = command
        .spawn()
        .map_err(|err| format!("Couldn't start claude: {err}"))?;
    let started = now_secs();
    if let Ok(mut store) = store().lock() {
        store.children.insert(job.id.clone(), child.id());
        if let Some(entry) = store.job_mut(&job.id)
            && let Some(turn) = entry.turns.last_mut()
        {
            turn.started = Some(started);
            turn.head_before = head_before;
            turn.snapshot = snapshot;
            turn.mode = project.profile.mode().into();
            entry.updated = started;
            entry.activity = "Thinking".into();
        }
        store.save();
    }
    changed(app);

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(prompt.as_bytes());
    }
    let stderr = child.stderr.take();
    let stderr_tail = std::thread::spawn(move || {
        let mut tail = String::new();
        if let Some(stderr) = stderr {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                tail.push_str(&line);
                tail.push('\n');
                if tail.len() > 4000 {
                    tail.drain(..tail.len() - 4000);
                }
            }
        }
        tail
    });

    watchdog(
        app.clone(),
        job.id.clone(),
        child.id(),
        config.timeout_minutes,
    );

    let log_path = dir.join("jobs").join(format!("{}.ndjson", job.id));
    let _ = fs::create_dir_all(log_path.parent().unwrap_or(&dir));
    let mut log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .ok();
    let mut logged: u64 = log_path.metadata().map(|meta| meta.len()).unwrap_or(0);

    let mut result_text = String::new();
    let mut result_error: Option<String> = None;
    let mut denials = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(file) = log.as_mut()
                && logged < 8 << 20
            {
                let _ = writeln!(file, "{line}");
                logged += line.len() as u64 + 1;
            }
            let Ok(event) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            match read_event(&event) {
                Event::Activity(text) => set_activity(app, &job.id, &text),
                Event::Denied(what) => denials.push(what),
                Event::Result {
                    text,
                    error,
                    denied,
                } => {
                    result_text = text;
                    result_error = error;
                    denials.extend(denied);
                }
                Event::Other => {}
            }
        }
    }
    let status = child.wait().ok();
    let tail = stderr_tail.join().unwrap_or_default();
    hook::unregister(&job.id);
    let _ = fs::remove_file(&settings_path);

    let code = status.and_then(|status| status.code());
    let cancelled = store()
        .lock()
        .map(|mut store| {
            store.children.remove(&job.id);
            let was = store.cancelled.contains(&job.id);
            store.cancelled.retain(|id| id != &job.id);
            was
        })
        .unwrap_or(false);
    let failure = if cancelled {
        Some("Cancelled".to_string())
    } else if let Some(error) = result_error {
        Some(error)
    } else if code != Some(0) {
        let detail = tail
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("");
        Some(if detail.is_empty() {
            format!("claude exited with code {}", code.unwrap_or(-1))
        } else {
            detail.trim().to_string()
        })
    } else {
        None
    };
    finish(app, &job.id, code, failure, result_text, denials);
    Ok(())
}

/// Ends a turn that runs past the time limit. Time spent waiting on an approval doesn't count.
fn watchdog(app: AppHandle, job_id: String, pid: u32, minutes: u32) {
    std::thread::spawn(move || {
        let limit = i64::from(minutes) * 60;
        let mut active: i64 = 0;
        loop {
            std::thread::sleep(Duration::from_secs(5));
            let running = store()
                .lock()
                .map(|store| store.children.get(&job_id) == Some(&pid))
                .unwrap_or(false);
            if !running {
                return;
            }
            if !hook::waiting(&job_id) {
                active += 5;
            }
            if active >= limit {
                set_activity(&app, &job_id, "Stopped: ran past the time limit");
                if let Ok(mut store) = store().lock() {
                    store.cancelled.push(job_id.clone());
                }
                interrupt(pid);
                return;
            }
        }
    });
}

fn git(folder: &Path, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git")
        .current_dir(folder)
        .args(args)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (output.status.success() && !text.is_empty()).then_some(text)
}

enum Event {
    Activity(String),
    Denied(String),
    Result {
        text: String,
        error: Option<String>,
        denied: Vec<String>,
    },
    Other,
}

/// Reads one stream-json event from `claude -p --output-format stream-json --verbose`.
fn read_event(event: &Value) -> Event {
    match (event["type"].as_str(), event["subtype"].as_str()) {
        (Some("assistant"), _) => {
            let blocks = event["message"]["content"].as_array();
            let tool = blocks
                .into_iter()
                .flatten()
                .rfind(|block| block["type"] == "tool_use");
            match tool {
                Some(tool) => Event::Activity(describe_tool(
                    tool["name"].as_str().unwrap_or("a tool"),
                    &tool["input"],
                )),
                None => Event::Activity("Writing".into()),
            }
        }
        (Some("system"), Some("api_retry")) => {
            let why = event["error"]
                .as_str()
                .or(event["reason"].as_str())
                .unwrap_or("error");
            Event::Activity(format!("Retrying ({why})"))
        }
        (Some("system"), Some("permission_denied")) => {
            Event::Denied(event["tool_name"].as_str().unwrap_or("a tool").to_string())
        }
        (Some("result"), subtype) => {
            let is_error = event["is_error"].as_bool().unwrap_or(false);
            let text = event["result"].as_str().unwrap_or_default().to_string();
            let denied = event["permission_denials"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|denial| {
                    describe_tool(
                        denial["tool_name"].as_str().unwrap_or("a tool"),
                        &denial["tool_input"],
                    )
                })
                .collect();
            let error = is_error.then(|| {
                if text.is_empty() {
                    match subtype {
                        Some("error_max_turns") => "Stopped at the turn limit".to_string(),
                        Some(other) => other.replace('_', " "),
                        None => "Claude reported an error".to_string(),
                    }
                } else {
                    text.lines().next().unwrap_or_default().to_string()
                }
            });
            Event::Result {
                text,
                error,
                denied,
            }
        }
        _ => Event::Other,
    }
}

/// "Editing src/list.ts", "Running npm test" and the like.
pub fn describe_tool(name: &str, input: &Value) -> String {
    let path = || {
        input["file_path"]
            .as_str()
            .or(input["path"].as_str())
            .or(input["notebook_path"].as_str())
            .unwrap_or_default()
            .to_string()
    };
    let short = |text: &str| -> String {
        let line = text.lines().next().unwrap_or_default();
        let mut cut: String = line.chars().take(80).collect();
        if line.chars().count() > 80 || text.lines().count() > 1 {
            cut.push('…');
        }
        cut
    };
    match name {
        "Bash" => format!(
            "Running {}",
            short(input["command"].as_str().unwrap_or("a command"))
        ),
        "Edit" | "MultiEdit" => format!("Editing {}", path()),
        "Write" => format!("Writing {}", path()),
        "NotebookEdit" => format!("Editing {}", path()),
        "Read" => format!("Reading {}", path()),
        "Glob" | "Grep" => format!(
            "Searching {}",
            short(input["pattern"].as_str().unwrap_or(""))
        ),
        "WebFetch" => format!("Fetching {}", short(input["url"].as_str().unwrap_or(""))),
        "WebSearch" => format!(
            "Searching the web for {}",
            short(input["query"].as_str().unwrap_or(""))
        ),
        "Task" | "Agent" => "Working with a subagent".into(),
        "TodoWrite" => "Planning".into(),
        other => format!("Using {other}"),
    }
}

fn set_activity(app: &AppHandle, job_id: &str, text: &str) {
    if let Ok(mut store) = store().lock()
        && let Some(job) = store.job_mut(job_id)
    {
        job.activity = text.to_string();
        job.updated = now_secs();
    }
    changed(app);
}

pub(crate) fn set_status(app: &AppHandle, job_id: &str, status: Status) {
    if let Ok(mut store) = store().lock()
        && let Some(job) = store.job_mut(job_id)
        && job.status.active()
    {
        job.status = status;
        store.save();
    }
    changed(app);
}

pub(crate) fn job_project(job_id: &str) -> Option<(String, String)> {
    store().lock().ok().and_then(|store| {
        store
            .jobs
            .iter()
            .find(|job| job.id == job_id)
            .map(|job| (job.project_id.clone(), job.project.clone()))
    })
}

fn finish(
    app: &AppHandle,
    job_id: &str,
    exit: Option<i32>,
    failure: Option<String>,
    result: String,
    denials: Vec<String>,
) {
    let mut note: Option<(String, String)> = None;
    if let Ok(mut store) = store().lock()
        && let Some(job) = store.job_mut(job_id)
    {
        let now = now_secs();
        // The turn that ran, or for a failure before it could start, the one waiting.
        let index = job
            .turns
            .iter()
            .rposition(|turn| turn.started.is_some() && turn.ended.is_none())
            .or_else(|| job.turns.iter().position(|turn| turn.started.is_none()));
        if let Some(turn) = index.and_then(|index| job.turns.get_mut(index)) {
            turn.ended = Some(now);
            turn.exit = exit;
            turn.result = result.clone();
            turn.denials = denials;
            if turn.started.is_none() {
                turn.started = Some(now);
            }
        }
        job.updated = now;
        job.summary = summary_line(&result, failure.as_deref());
        let cancelled = failure.as_deref() == Some("Cancelled");
        job.status = match (&failure, cancelled) {
            (_, true) => Status::Cancelled,
            (Some(_), false) => Status::Failed,
            (None, false) => Status::Done,
        };
        job.activity = match job.status {
            Status::Done => "Done".into(),
            Status::Cancelled => "Cancelled".into(),
            _ => failure.clone().unwrap_or_default(),
        };
        // A follow-up typed while it ran goes next.
        if job.pending_turn() {
            job.status = Status::Queued;
        }
        let mark = match job.status {
            Status::Done => "✓",
            Status::Cancelled => "◌",
            Status::Queued => "✓",
            _ => "✗",
        };
        note = Some((
            format!("{mark} {} · {}", job.project, job.title),
            job.summary.clone(),
        ));
        store.save();
    }
    changed(app);
    if let Some((title, body)) = note {
        notify(app, &title, &body);
    }
}

/// The line for notifications: Claude's closing one-liner, or the failure.
fn summary_line(result: &str, failure: Option<&str>) -> String {
    if let Some(failure) = failure {
        return failure.chars().take(140).collect();
    }
    // The brief asks for a one-line summary before the list of changed files; take the last
    // short prose line before any list.
    let lines: Vec<&str> = result
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let line = lines
        .iter()
        .find(|line| line.chars().count() <= 100 && !line.starts_with(['-', '*', '#', '`']))
        .or(lines.first())
        .copied()
        .unwrap_or("Finished");
    line.chars().take(140).collect()
}

pub(crate) fn notify(app: &AppHandle, title: &str, body: &str) {
    if !settings(app).notifications {
        return;
    }
    use tauri_plugin_notification::NotificationExt;
    if let Err(err) = app.notification().builder().title(title).body(body).show() {
        log::error!("couldn't show a notification: {err}");
    }
}

/// Opens a terminal in the project running `claude --resume <session>`.
pub fn open_in_terminal(app: &AppHandle, job_id: &str) -> Result<(), String> {
    let job = jobs()
        .into_iter()
        .find(|job| job.id == job_id)
        .ok_or("That job is gone")?;
    let binary =
        binary::resolve(&settings(app).binary).ok_or("Zephyr couldn't find the claude command")?;
    terminal(&job.folder, &binary, &job.session)
}

#[cfg(target_os = "macos")]
fn terminal(folder: &str, binary: &Path, session: &str) -> Result<(), String> {
    let quote = |text: &str| format!("'{}'", text.replace('\'', "'\\''"));
    let dir = std::env::temp_dir().join("zephyr-claude");
    fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    let script = dir.join(format!("resume-{session}.command"));
    fs::write(
        &script,
        format!(
            "#!/bin/sh\ncd {} && exec {} --resume {}\n",
            quote(folder),
            quote(&binary.to_string_lossy()),
            quote(session)
        ),
    )
    .map_err(|err| err.to_string())?;
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700))
        .map_err(|err| err.to_string())?;
    std::process::Command::new("open")
        .arg(&script)
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("Couldn't open Terminal: {err}"))
}

#[cfg(windows)]
fn terminal(folder: &str, binary: &Path, session: &str) -> Result<(), String> {
    std::process::Command::new("cmd")
        .args(["/C", "start", "", "/D", folder])
        .arg(binary)
        .args(["--resume", session])
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("Couldn't open a terminal: {err}"))
}

#[cfg(not(any(target_os = "macos", windows)))]
fn terminal(_folder: &str, _binary: &Path, _session: &str) -> Result<(), String> {
    Err("Opening a terminal isn't supported here yet".into())
}

/// Picks the project for `!claude <alias> <task>`: an alias as the first word, otherwise the
/// last one used. Returns the project and the task without the alias.
pub fn route<'a>(config: &'a ClaudeSettings, input: &'a str) -> (Option<&'a Project>, &'a str) {
    let input = input.trim();
    let (first, rest) = input.split_once(char::is_whitespace).unwrap_or((input, ""));
    let first = first.to_lowercase();
    if let Some(project) = config
        .projects
        .iter()
        .find(|project| !project.alias.is_empty() && project.alias == first)
    {
        return (Some(project), rest.trim());
    }
    let fallback = config
        .projects
        .iter()
        .find(|project| project.id == config.last_project)
        .or(config.projects.first());
    (fallback, input)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> ClaudeSettings {
        ClaudeSettings {
            projects: vec![
                Project {
                    id: "a".into(),
                    folder: "/tmp".into(),
                    alias: "zephyr".into(),
                    ..Project::default()
                },
                Project {
                    id: "b".into(),
                    folder: "/".into(),
                    alias: "wow".into(),
                    ..Project::default()
                },
            ],
            last_project: "b".into(),
            ..ClaudeSettings::default()
        }
    }

    #[test]
    fn routes_by_alias_or_the_last_project() {
        let config = config();
        let (project, task) = route(&config, "zephyr fix the bar");
        assert_eq!((project.unwrap().id.as_str(), task), ("a", "fix the bar"));
        let (project, task) = route(&config, "fix the tooltip");
        assert_eq!(
            (project.unwrap().id.as_str(), task),
            ("b", "fix the tooltip")
        );
    }

    #[test]
    fn reads_stream_events() {
        let tool = json!({"type":"assistant","message":{"content":[{"type":"text","text":"ok"},{"type":"tool_use","name":"Edit","input":{"file_path":"src/a.ts"}}]}});
        assert!(matches!(read_event(&tool), Event::Activity(text) if text == "Editing src/a.ts"));
        let done = json!({"type":"result","subtype":"success","is_error":false,"result":"Fixed it.\n- src/a.ts","permission_denials":[]});
        match read_event(&done) {
            Event::Result { text, error, .. } => {
                assert_eq!(summary_line(&text, error.as_deref()), "Fixed it.");
                assert!(error.is_none());
            }
            _ => panic!("expected a result"),
        }
        let limit = json!({"type":"result","subtype":"error_max_turns","is_error":true});
        assert!(
            matches!(read_event(&limit), Event::Result { error: Some(error), .. } if error == "Stopped at the turn limit")
        );
    }

    #[test]
    fn describes_tools_in_plain_words() {
        assert_eq!(
            describe_tool("Bash", &json!({"command":"npm test"})),
            "Running npm test"
        );
        assert_eq!(
            describe_tool("Write", &json!({"file_path":"a.md"})),
            "Writing a.md"
        );
        assert_eq!(describe_tool("Mystery", &json!({})), "Using Mystery");
    }

    #[test]
    fn rejects_bad_projects() {
        let mut bad = config();
        bad.projects[1].alias = "zephyr".into();
        assert!(bad.normalized().is_err());
        let mut missing = ClaudeSettings::default();
        missing.projects.push(Project {
            folder: "/definitely/not/here".into(),
            ..Project::default()
        });
        assert!(missing.normalized().is_err());
    }
}
