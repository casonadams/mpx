use std::path::Path;

use mpx::node::process::cwd_of;

#[test]
fn own_pid_reports_the_working_directory() {
    if !cfg!(any(target_os = "macos", target_os = "linux")) {
        assert_eq!(cwd_of(std::process::id()), None);
        return;
    }
    let expected = std::env::current_dir()
        .and_then(|p| p.canonicalize())
        .expect("current dir");
    let reported = cwd_of(std::process::id()).expect("own working directory");
    let reported = Path::new(&reported).canonicalize().expect("reported dir");
    assert_eq!(reported, expected);
}

#[test]
fn unknown_pid_has_no_working_directory() {
    assert_eq!(cwd_of(i32::MAX as u32), None);
}
