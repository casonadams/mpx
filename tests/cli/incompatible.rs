//! A session socket served by a daemon from another mpx version must be reported, never misread:
//! a fake daemon answers as mpx 0.1.2 does (no `Info` request) or with a newer protocol number.
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};

use clap::Parser;
use mpx::cli::{Cli, run_cli};
use mpx::client::{Client, Handshake};

use super::fixture::Fixture;

#[derive(Clone, Copy)]
enum Fake {
    /// mpx 0.1.2: answers `Ping` but rejects `Info` as an unknown request.
    Old,
    /// A daemon that reports a protocol this client does not speak.
    Newer,
    /// Accepts and hangs up without answering.
    Mute,
}

fn reply(fake: Fake, request: &str) -> Option<String> {
    let reply = match fake {
        Fake::Mute => return None,
        _ if request.contains("\"Ping\"") => r#"{"status":"Pong"}"#,
        Fake::Old => r#"{"status":"Error","data":"Invalid request JSON: unknown variant `Info`"}"#,
        Fake::Newer => {
            r#"{"status":"Session","data":{"name":"t","endpoint":"e","pid":4242,"created_ms":0,"windows":1,"panes":1,"protocol":99}}"#
        }
    };
    Some(format!("{reply}\n"))
}

/// Serves `fake` on a socket in `dir` from a background thread for the rest of the test.
fn serve(dir: &Path, fake: Fake) -> PathBuf {
    let path = dir.join("old.sock");
    let listener = UnixListener::bind(&path).expect("bind fake daemon");
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut writer = stream.try_clone().expect("clone stream");
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                let Some(answer) = reply(fake, &line) else {
                    break;
                };
                if writer.write_all(answer.as_bytes()).is_err() {
                    break;
                }
            }
        }
    });
    path
}

fn handshake(endpoint: &Path) -> Handshake {
    let endpoint = endpoint.to_string_lossy().into_owned();
    runtime().block_on(async move {
        let mut client = Client::connect(&endpoint).await.expect("connect");
        client.handshake().await
    })
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

/// Runs an mpx command in-process against `endpoint`, isolated under `dir`.
fn cli(dir: &Path, endpoint: &Path, args: &[&str]) -> Result<i32, String> {
    let config = dir.join("config.toml");
    std::fs::write(&config, "").expect("config file");
    let mut argv = vec![
        "mpx".to_string(),
        "-f".to_string(),
        config.to_string_lossy().into_owned(),
        "--state-dir".to_string(),
        dir.join("state").to_string_lossy().into_owned(),
        "--endpoint".to_string(),
        endpoint.to_string_lossy().into_owned(),
        "-s".to_string(),
        "t".to_string(),
    ];
    argv.extend(args.iter().map(|a| a.to_string()));
    runtime().block_on(run_cli(Cli::parse_from(argv)))
}

#[test]
fn handshake_tells_compatible_incompatible_and_silent_daemons_apart() {
    let current = Fixture::start();
    assert_eq!(
        handshake(Path::new(&current.endpoint())),
        Handshake::Compatible
    );

    let dir = tempfile::tempdir().expect("temp dir");
    let old = serve(dir.path(), Fake::Old);
    assert_eq!(
        handshake(&old),
        Handshake::Incompatible {
            pid: Some(std::process::id())
        },
        "the pid of an old daemon comes from the socket peer"
    );

    let dir = tempfile::tempdir().expect("temp dir");
    let newer = serve(dir.path(), Fake::Newer);
    assert_eq!(
        handshake(&newer),
        Handshake::Incompatible { pid: Some(4242) }
    );

    let dir = tempfile::tempdir().expect("temp dir");
    let mute = serve(dir.path(), Fake::Mute);
    assert_eq!(handshake(&mute), Handshake::Gone);
}

#[test]
fn commands_against_an_older_daemon_explain_how_to_stop_it() {
    let dir = tempfile::tempdir().expect("temp dir");
    let old = serve(dir.path(), Fake::Old);
    for args in [&["list-panes"][..], &["run", "-n", "x", "true"][..]] {
        let err = cli(dir.path(), &old, args).expect_err("an old daemon is refused");
        assert!(err.contains("incompatible mpx version"), "{err}");
        assert!(err.contains("mpx kill-server"), "{err}");
    }
    assert_eq!(cli(dir.path(), &old, &["has-session"]), Ok(0));
}

#[test]
fn kill_server_never_signals_its_own_process() {
    let dir = tempfile::tempdir().expect("temp dir");
    let old = serve(dir.path(), Fake::Old);
    let err = cli(dir.path(), &old, &["kill-server"]).expect_err("own pid is refused");
    assert!(err.contains("refusing to stop this process"), "{err}");
}
