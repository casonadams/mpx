use mpx::config::IdSource;
use mpx::store::{claude_session_id, lookup_session_id, omp_session_id};
use std::fs;

#[test]
fn claude_id_requires_matching_pid() {
    let json = r#"{"pid":40368,"sessionId":"9d70037d-9306","cwd":"/x"}"#;
    assert_eq!(claude_session_id(json, 40368).as_deref(), Some("9d70037d-9306"));
    assert_eq!(claude_session_id(json, 1), None);
    assert_eq!(claude_session_id("not json", 40368), None);
    assert_eq!(claude_session_id(r#"{"pid":1,"sessionId":""}"#, 1), None);
}

#[test]
fn omp_id_comes_from_session_file_name() {
    let text = "/work/mpx\n/home/.omp/agent/sessions/-work-mpx/2026-10-30T15-43-50-480Z_01a18d7b-4cd0.jsonl\ncwdstat 1 2\n";
    assert_eq!(omp_session_id(text).as_deref(), Some("01a18d7b-4cd0"));
    assert_eq!(omp_session_id("/work/mpx\n"), None);
    assert_eq!(omp_session_id("/work\n/x/noseparator.jsonl\n"), None);
}

#[test]
fn lookup_reads_each_tools_state_file() {
    let home = tempfile::tempdir().unwrap();
    let claude = home.path().join(".claude/sessions");
    fs::create_dir_all(&claude).unwrap();
    fs::write(claude.join("77.json"), r#"{"pid":77,"sessionId":"abc"}"#).unwrap();
    let omp = home.path().join(".omp/agent/terminal-sessions");
    fs::create_dir_all(&omp).unwrap();
    fs::write(omp.join("ttys004"), "/w\n/s/2026_def.jsonl\n").unwrap();

    let id = |source, pid, tty| lookup_session_id(source, home.path(), pid, tty);
    assert_eq!(id(IdSource::Claude, Some(77), None).as_deref(), Some("abc"));
    assert_eq!(id(IdSource::Claude, Some(78), None), None);
    assert_eq!(id(IdSource::Omp, None, Some("ttys004")).as_deref(), Some("def"));
    assert_eq!(id(IdSource::Omp, None, Some("ttys005")), None);
    assert_eq!(id(IdSource::Omp, None, Some("../ttys004")), None);
}
