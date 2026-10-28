use std::io::Write;

use mpx::protocol::PipeMessage;
use mpx::store::PipeStore;

fn msg(seq: u64, data: &str) -> PipeMessage {
    PipeMessage {
        seq,
        ts_ms: seq * 10,
        data: data.into(),
    }
}

fn store_in(root: &std::path::Path) -> PipeStore {
    PipeStore::new(root.join("pipes"))
}

fn messages(store: &PipeStore, name: &str) -> Vec<PipeMessage> {
    store
        .load_all()
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.messages)
        .unwrap_or_default()
}

#[test]
fn appended_messages_load_in_order_with_next_seq() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    store.create("build", 1024).unwrap();
    store.append("build", &msg(1, "one")).unwrap();
    store.append("build", &msg(2, "two")).unwrap();

    let pipes = store.load_all();
    assert_eq!(pipes.len(), 1);
    let pipe = &pipes[0];
    assert_eq!(pipe.name, "build");
    assert_eq!(pipe.capacity, 1024);
    assert_eq!(pipe.next_seq, 3);
    assert_eq!(pipe.messages, vec![msg(1, "one"), msg(2, "two")]);
}

#[test]
fn append_to_uncreated_pipe_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    assert!(store.append("ghost", &msg(1, "x")).is_err());
    assert!(store.load_all().is_empty());
}

#[test]
fn missing_directory_loads_nothing() {
    let dir = tempfile::tempdir().unwrap();
    assert!(store_in(dir.path()).load_all().is_empty());
}

#[test]
fn rewrite_keeps_next_seq_and_replaces_messages() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    store.create("logs", 1024).unwrap();
    for seq in 1..=3 {
        store.append("logs", &msg(seq, "old")).unwrap();
    }
    store.rewrite("logs", 1024, 10, &[msg(9, "kept")]).unwrap();

    let pipe = store.load_all().remove(0);
    assert_eq!(pipe.next_seq, 10);
    assert_eq!(pipe.messages, vec![msg(9, "kept")]);
}

#[test]
fn appends_after_rewrite_continue_the_sequence() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    store.create("logs", 1024).unwrap();
    store.rewrite("logs", 1024, 5, &[msg(4, "a")]).unwrap();
    store.append("logs", &msg(5, "b")).unwrap();

    let pipe = store.load_all().remove(0);
    assert_eq!(pipe.next_seq, 6);
    assert_eq!(pipe.messages, vec![msg(4, "a"), msg(5, "b")]);
}

#[test]
fn cursors_persist_across_store_instances() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    store.create("tasks", 1024).unwrap();
    store.set_cursor("tasks", "reviewer", 3).unwrap();
    store.set_cursor("tasks", "builder", 1).unwrap();
    store.set_cursor("tasks", "reviewer", 7).unwrap();

    let reopened = store_in(dir.path());
    let mut cursors = reopened.load_all().remove(0).cursors;
    cursors.sort();
    assert_eq!(
        cursors,
        vec![("builder".to_string(), 1), ("reviewer".to_string(), 7)]
    );
}

#[test]
fn cursor_on_unknown_pipe_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    assert!(store_in(dir.path()).set_cursor("ghost", "c", 1).is_err());
}

#[test]
fn torn_trailing_line_is_ignored_and_later_appends_survive() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    store.create("torn", 1024).unwrap();
    store.append("torn", &msg(1, "whole")).unwrap();

    let log = dir.path().join("pipes").join("torn").join("log.jsonl");
    let mut file = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
    file.write_all(b"{\"seq\":2,\"ts_ms\":20,\"da").unwrap();
    drop(file);
    assert_eq!(messages(&store, "torn"), vec![msg(1, "whole")]);

    store.append("torn", &msg(3, "after")).unwrap();
    assert_eq!(
        messages(&store, "torn"),
        vec![msg(1, "whole"), msg(3, "after")]
    );
}

#[test]
fn retained_messages_fit_capacity_keeping_the_newest() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    store.create("small", 4).unwrap();
    store.append("small", &msg(1, "aaa")).unwrap();
    store.append("small", &msg(2, "bbb")).unwrap();
    store.append("small", &msg(3, "c")).unwrap();

    assert_eq!(messages(&store, "small"), vec![msg(2, "bbb"), msg(3, "c")]);
}

#[test]
fn unreadable_pipe_is_skipped_and_others_load() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(dir.path());
    store.create("good", 1024).unwrap();
    store.append("good", &msg(1, "ok")).unwrap();
    let broken = dir.path().join("pipes").join("broken");
    std::fs::create_dir_all(&broken).unwrap();
    std::fs::write(broken.join("meta.json"), "not json").unwrap();

    let names: Vec<String> = store.load_all().into_iter().map(|p| p.name).collect();
    assert_eq!(names, vec!["good".to_string()]);
}

#[test]
fn hostile_names_stay_inside_the_pipe_directory() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    let store = PipeStore::new(root.join("pipes"));
    for name in ["../escape", "a/b/../../c", "..", "/abs"] {
        store.create(name, 1024).unwrap();
        store.append(name, &msg(1, "x")).unwrap();
    }

    let top: Vec<String> = std::fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(top, vec!["pipes".to_string()]);

    let mut names: Vec<String> = store.load_all().into_iter().map(|p| p.name).collect();
    names.sort();
    assert_eq!(names, vec!["..", "../escape", "/abs", "a/b/../../c"]);
}
