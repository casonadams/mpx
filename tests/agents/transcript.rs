use std::path::PathBuf;

use mpx::agents::{ResumableSession, transcript_preview};

/// omp-style message line with a text content block.
fn omp_line(role: &str, text: &str) -> String {
    let text = serde_json::to_string(text).expect("json string");
    format!(
        r#"{{"type":"message","message":{{"role":"{role}","content":[{{"type":"text","text":{text}}}]}}}}"#
    )
}

/// Claude-style message line with a plain string content.
fn claude_line(role: &str, text: &str) -> String {
    let text = serde_json::to_string(text).expect("json string");
    format!(r#"{{"type":"{role}","message":{{"role":"{role}","content":{text}}}}}"#)
}

fn session_at(path: PathBuf) -> ResumableSession {
    ResumableSession {
        harness: "omp".into(),
        id: "s1".into(),
        title: "t".into(),
        cwd: None,
        modified_ms: 0,
        path,
    }
}

fn write_session(lines: &[String]) -> (tempfile::TempDir, ResumableSession) {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("session.jsonl");
    std::fs::write(&path, lines.join("\n")).expect("write session");
    (dir, session_at(path))
}

#[test]
fn omp_transcript_returns_last_messages_in_order_capped_at_max_lines() {
    let lines = [
        omp_line("user", "one"),
        omp_line("assistant", "two"),
        omp_line("user", "three"),
        omp_line("assistant", "four"),
        omp_line("user", "five"),
    ];
    let (_dir, session) = write_session(&lines);
    assert_eq!(
        transcript_preview(&session, 3),
        ["user> three", "assistant> four", "user> five"]
    );
}

#[test]
fn claude_transcript_keeps_the_most_recent_exchange() {
    let lines = [
        claude_line("user", "first"),
        claude_line("assistant", "reply one"),
        claude_line("user", "second"),
        claude_line("assistant", "reply two"),
    ];
    let (_dir, session) = write_session(&lines);
    assert_eq!(
        transcript_preview(&session, 2),
        ["user> second", "assistant> reply two"]
    );
}

#[test]
fn transcript_cap_counts_display_lines_of_multi_line_messages() {
    let lines = [
        omp_line("user", "first"),
        omp_line("assistant", "line a\nline b"),
    ];
    let (_dir, session) = write_session(&lines);
    assert_eq!(
        transcript_preview(&session, 2),
        ["assistant> line a", "  line b"]
    );
}

#[test]
fn short_transcript_returns_every_message() {
    let lines = [omp_line("user", "only"), omp_line("assistant", "reply")];
    let (_dir, session) = write_session(&lines);
    assert_eq!(
        transcript_preview(&session, 10),
        ["user> only", "assistant> reply"]
    );
}

#[test]
fn missing_session_file_has_no_preview() {
    let dir = tempfile::tempdir().expect("temp dir");
    let session = session_at(dir.path().join("gone.jsonl"));
    assert!(transcript_preview(&session, 5).is_empty());
}
