use super::fixture::{Fixture, run_command};

#[test]
fn if_shell_runs_the_branch_its_shell_condition_picks() {
    let f = Fixture::start();
    let yes = f.path().join("yes");
    let no = f.path().join("no");
    let then = format!("run-shell touch {}", yes.display());
    let otherwise = format!("run-shell touch {}", no.display());
    assert_eq!(run_command(&f, &["if-shell", "true", &then, &otherwise]), 0);
    assert!(yes.exists());
    assert!(!no.exists());
}

#[test]
fn if_shell_with_a_format_takes_the_else_branch_for_zero() {
    let f = Fixture::start();
    let yes = f.path().join("yes");
    let no = f.path().join("no");
    let then = format!("run-shell touch {}", yes.display());
    let otherwise = format!("run-shell touch {}", no.display());
    assert_eq!(run_command(&f, &["if-shell", "-F", "0", &then, &otherwise]), 0);
    assert!(!yes.exists());
    assert!(no.exists());
}

#[test]
fn if_shell_refuses_a_branch_that_does_not_parse() {
    let f = Fixture::start();
    assert_ne!(run_command(&f, &["if-shell", "true", "nosuchcommand"]), 0);
}
