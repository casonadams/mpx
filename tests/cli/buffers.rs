use mpx::cli::{capture_line, capture_spec};
use mpx::vt::capture::{CaptureLine, CaptureSpec};

#[test]
fn capture_lines_use_tmux_numbering_and_a_dash_names_the_edge() {
    assert_eq!(capture_line("-", false), Ok(CaptureLine::First));
    assert_eq!(capture_line("-", true), Ok(CaptureLine::Last));
    assert_eq!(capture_line("-3", false), Ok(CaptureLine::Line(-3)));
    assert_eq!(capture_line("0", true), Ok(CaptureLine::Line(0)));
    assert_eq!(
        capture_line("x", false),
        Err("capture-pane: invalid line 'x'".to_string())
    );
}

#[test]
fn capture_flags_become_the_capture_the_daemon_takes() {
    assert_eq!(
        capture_spec(Some("-"), Some("-2"), true, true),
        Ok(CaptureSpec {
            start: Some(CaptureLine::First),
            end: Some(CaptureLine::Line(-2)),
            escapes: true,
            join: true,
        })
    );
    assert_eq!(
        capture_spec(None, None, false, false),
        Ok(CaptureSpec::default())
    );
}
