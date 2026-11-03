//! `capture-pane` through the daemon: a pane's screen and history as text.
use std::time::{Duration, Instant};

use mpx::client::Client;
use mpx::vt::capture::{CaptureLine, CaptureSpec};

use super::support::{fresh, logs_until, pty};

/// The capture of `pane` once it contains `needle`, polled for at most two seconds.
async fn capture_until(client: &mut Client, pane: &str, spec: CaptureSpec, needle: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let text = client.capture_pane(pane, spec).await.expect("capture");
        if text.contains(needle) || Instant::now() >= deadline {
            return text;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn the_screen_and_the_history_of_a_pty_pane_are_captured() {
    let (_daemon, mut client) = fresh().await;
    let script = "i=1; while [ $i -le 30 ]; do echo line$i; i=$((i+1)); done; sleep 5";
    client.spawn(pty("p", "sh", &["-c", script])).await.unwrap();
    logs_until(&mut client, "p", "line30").await;

    let screen = capture_until(&mut client, "p", CaptureSpec::default(), "line30").await;
    assert!(screen.contains("line30\n"), "{screen}");
    assert!(!screen.contains("line1\n"), "{screen}");

    let whole = CaptureSpec {
        start: Some(CaptureLine::First),
        ..CaptureSpec::default()
    };
    let text = client.capture_pane("p", whole).await.unwrap();
    assert!(text.starts_with("line1\n"), "{text}");
    assert!(text.contains("line30\n"), "{text}");
}

#[tokio::test]
async fn join_keeps_a_wrapped_line_whole_and_escapes_keep_its_colors() {
    let (_daemon, mut client) = fresh().await;
    let script = "printf '\\033[31m'; printf '%0100d\\n' 0; printf 'plain\\n'; sleep 5";
    client.spawn(pty("p", "sh", &["-c", script])).await.unwrap();
    logs_until(&mut client, "p", "plain").await;

    let from_start = CaptureSpec {
        start: Some(CaptureLine::First),
        ..CaptureSpec::default()
    };
    let zeros = "0".repeat(100);
    let joined = CaptureSpec {
        join: true,
        ..from_start
    };
    let text = capture_until(&mut client, "p", joined, "plain").await;
    assert!(text.contains(&format!("{zeros}\n")), "{text}");

    let wrapped = client.capture_pane("p", from_start).await.unwrap();
    assert!(!wrapped.contains(&zeros), "{wrapped}");
    assert!(
        wrapped.contains(&format!("{}\n", "0".repeat(80))),
        "{wrapped}"
    );

    let escaped = CaptureSpec {
        escapes: true,
        ..from_start
    };
    let text = client.capture_pane("p", escaped).await.unwrap();
    assert!(text.contains("\x1b[0;31m0"), "{text:?}");
}
