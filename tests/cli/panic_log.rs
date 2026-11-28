use mpx::cli::panic_log::{PANIC_LOG, install_panic_log};

#[test]
fn a_panic_is_appended_to_the_log_with_its_message_and_backtrace() {
    let dir = tempfile::tempdir().expect("temp dir");
    install_panic_log(dir.path().join("state"));
    let result = std::thread::Builder::new()
        .name("crashy".into())
        .spawn(|| panic!("scrollback exploded"))
        .expect("spawn")
        .join();
    assert!(result.is_err());
    let log = std::fs::read_to_string(dir.path().join("state").join(PANIC_LOG)).expect("log");
    assert!(log.contains("thread 'crashy'"), "{log}");
    assert!(log.contains("scrollback exploded"), "{log}");
    assert!(log.contains("install_panic_log"), "{log}");
}
