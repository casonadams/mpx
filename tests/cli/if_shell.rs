use std::path::Path;

use super::fixture::{Fixture, run_command};

/// `path` as a shell command spells it: `sh` reads backslashes as escapes, so Windows separators
/// become forward slashes.
fn sh_path(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

#[test]
fn if_shell_runs_the_branch_its_shell_condition_picks() {
    let f = Fixture::start();
    let yes = f.path().join("yes");
    let no = f.path().join("no");
    let then = format!("run-shell touch {}", sh_path(&yes));
    let otherwise = format!("run-shell touch {}", sh_path(&no));
    assert_eq!(run_command(&f, &["if-shell", "true", &then, &otherwise]), 0);
    assert!(yes.exists());
    assert!(!no.exists());
}

#[test]
fn if_shell_with_a_format_takes_the_else_branch_for_zero() {
    let f = Fixture::start();
    let yes = f.path().join("yes");
    let no = f.path().join("no");
    let then = format!("run-shell touch {}", sh_path(&yes));
    let otherwise = format!("run-shell touch {}", sh_path(&no));
    assert_eq!(run_command(&f, &["if-shell", "-F", "0", &then, &otherwise]), 0);
    assert!(!yes.exists());
    assert!(no.exists());
}

#[test]
fn if_shell_refuses_a_branch_that_does_not_parse() {
    let f = Fixture::start();
    assert_ne!(run_command(&f, &["if-shell", "true", "nosuchcommand"]), 0);
}
