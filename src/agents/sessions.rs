use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde_json::Value;

use super::{Harness, ResumableSession, SessionStore};
use crate::config::home_dir;

/// Bytes read from the start of a session file to find its header and first prompt.
const HEAD_BYTES: u64 = 64 * 1024;
/// Bytes read from the end of a session file for transcript previews.
const TAIL_BYTES: u64 = 256 * 1024;
const TITLE_CHARS: usize = 80;
/// Directory depth of session files below each store root (`<slug>/<file>` or `YYYY/MM/DD/<file>`).
const CLAUDE_OMP_DEPTH: usize = 1;
const CODEX_DEPTH: usize = 3;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionMeta {
    pub id: String,
    pub title: String,
    pub cwd: Option<String>,
}

fn json_values(text: &str) -> impl Iterator<Item = Value> + '_ {
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
}

fn truncate_title(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    line.chars().take(TITLE_CHARS).collect()
}

/// Text of a message entry (`message.content` string or text blocks) plus its role.
fn message_of(v: &Value) -> Option<(String, String)> {
    if let Some(msg) = v.get("message").filter(|m| m.get("role").is_some()) {
        let role = msg.get("role")?.as_str()?.to_string();
        return Some((role, content_text(msg.get("content")?)));
    }
    let payload = v.get("payload")?;
    match (v.get("type")?.as_str()?, payload.get("type")?.as_str()?) {
        ("response_item", "message") => {
            let role = payload.get("role")?.as_str()?.to_string();
            Some((role, content_text(payload.get("content")?)))
        }
        ("event_msg", "user_message") => {
            let text = payload.get("message")?.as_str()?.to_string();
            Some(("user".to_string(), text))
        }
        _ => None,
    }
}

fn content_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| item.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// First user prompt that is not injected context (those start with `<`).
fn first_user_prompt(head: &str) -> Option<String> {
    json_values(head)
        .filter_map(|v| message_of(&v))
        .find(|(role, text)| {
            role == "user" && !text.trim_start().starts_with('<') && !text.trim().is_empty()
        })
        .map(|(_, text)| truncate_title(&text))
}

/// omp header: line 1 `{"type":"title"}`, line 2 `{"type":"session","id","cwd"}`.
pub fn parse_omp_meta(head: &str) -> Option<SessionMeta> {
    let mut title = None;
    let mut session: Option<SessionMeta> = None;
    for v in json_values(head).take(8) {
        match v.get("type").and_then(Value::as_str) {
            Some("title") => title = v.get("title").and_then(Value::as_str).map(truncate_title),
            Some("session") => {
                session = Some(SessionMeta {
                    id: v.get("id")?.as_str()?.to_string(),
                    title: String::new(),
                    cwd: v.get("cwd").and_then(Value::as_str).map(str::to_string),
                })
            }
            _ => {}
        }
    }
    let mut meta = session?;
    meta.title = title
        .filter(|t| !t.is_empty())
        .or_else(|| first_user_prompt(head))
        .unwrap_or_else(|| meta.id.clone());
    Some(meta)
}

/// Claude transcript: `sessionId`/`cwd` on entries, title from a `summary` entry or the first prompt.
pub fn parse_claude_meta(head: &str, file_stem: &str) -> SessionMeta {
    let mut id = None;
    let mut cwd = None;
    let mut summary = None;
    for v in json_values(head) {
        id = id.or_else(|| {
            v.get("sessionId")
                .and_then(Value::as_str)
                .map(str::to_string)
        });
        cwd = cwd.or_else(|| v.get("cwd").and_then(Value::as_str).map(str::to_string));
        if summary.is_none() && v.get("type").and_then(Value::as_str) == Some("summary") {
            summary = v.get("summary").and_then(Value::as_str).map(truncate_title);
        }
    }
    let id = id.unwrap_or_else(|| file_stem.to_string());
    let title = summary
        .filter(|s| !s.is_empty())
        .or_else(|| first_user_prompt(head))
        .unwrap_or_else(|| id.clone());
    SessionMeta { id, title, cwd }
}

/// Codex rollout: first `session_meta` entry carries id and cwd.
pub fn parse_codex_meta(head: &str) -> Option<SessionMeta> {
    let payload = json_values(head)
        .find(|v| v.get("type").and_then(Value::as_str) == Some("session_meta"))?
        .get("payload")?
        .clone();
    let id = payload.get("id")?.as_str()?.to_string();
    let title = first_user_prompt(head).unwrap_or_else(|| id.clone());
    Some(SessionMeta {
        id,
        title,
        cwd: payload
            .get("cwd")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

fn format_message(role: &str, body: &str) -> Vec<String> {
    let mut lines = body.lines();
    let Some(first) = lines.next() else {
        return Vec::new();
    };
    std::iter::once(format!("{role}> {first}"))
        .chain(lines.map(|l| format!("  {l}")))
        .collect()
}

/// Last user/assistant messages of a transcript fragment, one display line per text line.
pub fn parse_preview(text: &str, max_lines: usize) -> Vec<String> {
    let mut lines: Vec<String> = json_values(text)
        .filter_map(|v| message_of(&v))
        .filter(|(role, _)| role == "user" || role == "assistant")
        .flat_map(|(role, body)| format_message(&role, body.trim_end()))
        .collect();
    let skip = lines.len().saturating_sub(max_lines);
    lines.drain(..skip);
    lines
}

fn read_window(path: &Path, from_end: Option<u64>) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = match from_end {
        Some(tail) => len.saturating_sub(tail),
        None => 0,
    };
    file.seek(SeekFrom::Start(start)).ok()?;
    let limit = from_end.unwrap_or(HEAD_BYTES);
    let mut buf = Vec::new();
    file.take(limit).read_to_end(&mut buf).ok()?;
    let mut text = String::from_utf8_lossy(&buf).into_owned();
    if let Some(pos) = text.find('\n').filter(|_| start > 0) {
        text.drain(..=pos);
    }
    Some(text)
}

/// What a directory walk does with one entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visit {
    Recurse,
    Keep,
    Skip,
}

/// Walk decision for an entry `depth` levels below the root: directories are entered above `max_depth`,
/// and `.jsonl` files are kept exactly at `max_depth`.
pub fn visit_entry(
    is_dir: bool,
    is_file: bool,
    is_jsonl: bool,
    depth: usize,
    max_depth: usize,
) -> Visit {
    if is_dir && depth < max_depth {
        Visit::Recurse
    } else if is_file && is_jsonl && depth == max_depth {
        Visit::Keep
    } else {
        Visit::Skip
    }
}

fn modified_ms(entry: &std::fs::DirEntry) -> u64 {
    entry
        .metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis() as u64)
}

/// Collects `*.jsonl` files exactly `max_depth` directories below `root`.
fn collect_files(dir: &Path, depth: usize, max_depth: usize, out: &mut Vec<(PathBuf, u64)>) {
    let entries: Vec<std::fs::DirEntry> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().collect())
        .unwrap_or_default();
    for entry in entries {
        let path = entry.path();
        let kind = entry.file_type().ok();
        let is_dir = kind.is_some_and(|k| k.is_dir());
        let is_file = kind.is_some_and(|k| k.is_file());
        let is_jsonl = path.extension().is_some_and(|e| e == "jsonl");
        match visit_entry(is_dir, is_file, is_jsonl, depth, max_depth) {
            Visit::Recurse => collect_files(&path, depth + 1, max_depth, out),
            Visit::Keep => out.push((path, modified_ms(&entry))),
            Visit::Skip => {}
        }
    }
}

fn store_root(store: SessionStore) -> Option<(PathBuf, usize)> {
    let home = home_dir()?;
    Some(match store {
        SessionStore::Claude => (home.join(".claude").join("projects"), CLAUDE_OMP_DEPTH),
        SessionStore::Omp => (
            home.join(".omp").join("agent").join("sessions"),
            CLAUDE_OMP_DEPTH,
        ),
        SessionStore::Codex => (home.join(".codex").join("sessions"), CODEX_DEPTH),
    })
}

struct Candidate {
    harness: String,
    store: SessionStore,
    path: PathBuf,
    modified_ms: u64,
}

fn candidates(harness: &Harness, store: SessionStore) -> Vec<Candidate> {
    let Some((root, depth)) = store_root(store) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    collect_files(&root, 0, depth, &mut files);
    files
        .into_iter()
        .map(|(path, modified_ms)| Candidate {
            harness: harness.name.clone(),
            store,
            path,
            modified_ms,
        })
        .collect()
}

fn session_from(c: &Candidate) -> Option<ResumableSession> {
    let head = read_window(&c.path, None)?;
    let stem = c.path.file_stem()?.to_string_lossy().into_owned();
    let meta = match c.store {
        SessionStore::Claude => parse_claude_meta(&head, &stem),
        SessionStore::Omp => parse_omp_meta(&head)?,
        SessionStore::Codex => parse_codex_meta(&head)?,
    };
    Some(ResumableSession {
        harness: c.harness.clone(),
        id: meta.id,
        title: meta.title,
        cwd: meta.cwd,
        modified_ms: c.modified_ms,
        path: c.path.clone(),
    })
}

/// Most recently modified first. `cwd` restricts to sessions recorded for that directory.
pub fn scan_resumable(
    registry: &[Harness],
    cwd: Option<&Path>,
    limit: usize,
) -> Vec<ResumableSession> {
    let mut files: Vec<Candidate> = registry
        .iter()
        .filter_map(|h| h.store.map(|store| candidates(h, store)))
        .flatten()
        .collect();
    files.sort_by_key(|c| std::cmp::Reverse(c.modified_ms));
    let wanted = cwd.map(|p| p.to_string_lossy().into_owned());
    files
        .iter()
        .filter_map(session_from)
        .filter(|s| wanted.as_ref().is_none_or(|w| s.cwd.as_ref() == Some(w)))
        .take(limit)
        .collect()
}

/// Last user/assistant messages of a stored session as display lines.
pub fn transcript_preview(session: &ResumableSession, max_lines: usize) -> Vec<String> {
    read_window(&session.path, Some(TAIL_BYTES))
        .map(|text| parse_preview(&text, max_lines))
        .unwrap_or_default()
}
