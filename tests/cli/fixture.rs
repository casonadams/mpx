use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use clap::Parser;
use mpx::cli::{Cli, main_entry, run_cli};
use mpx::client::Client;
use mpx::protocol::{NodeInfo, SessionInfo};

/// Upper bound for every wait in these tests.
pub const TIMEOUT: Duration = Duration::from_secs(5);

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

/// Session `t` whose daemon runs in-process on a background thread, isolated under a temp dir
/// (config file, state dir, socket). The daemon is stopped on drop.
pub struct Fixture {
    dir: tempfile::TempDir,
    daemon: Option<mpsc::Receiver<i32>>,
}

impl Fixture {
    /// Starts the daemon (through `main_entry`) and returns once it answers `has-session`.
    pub fn start() -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("config.toml"), "").expect("config file");
        let mut fixture = Self { dir, daemon: None };
        let (tx, rx) = mpsc::channel();
        let argv = fixture.argv(&["daemon"]);
        std::thread::spawn(move || {
            let _ = tx.send(main_entry(argv));
        });
        fixture.daemon = Some(rx);
        fixture.wait_ready();
        fixture
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    pub fn config_path(&self) -> PathBuf {
        self.dir.path().join("config.toml")
    }

    pub fn state_dir(&self) -> PathBuf {
        self.dir.path().join("state")
    }

    pub fn endpoint(&self) -> String {
        self.dir
            .path()
            .join("s.sock")
            .to_string_lossy()
            .into_owned()
    }

    /// `args` behind the isolation flags: this session's config, state dir and socket.
    pub fn argv(&self, args: &[&str]) -> Vec<String> {
        self.argv_with_config(&self.config_path(), args)
    }

    pub fn argv_with_config(&self, config: &Path, args: &[&str]) -> Vec<String> {
        let mut argv = vec![
            "mpx".to_string(),
            "-f".to_string(),
            config.to_string_lossy().into_owned(),
            "--state-dir".to_string(),
            self.state_dir().to_string_lossy().into_owned(),
            "--endpoint".to_string(),
            self.endpoint(),
            "-s".to_string(),
            "t".to_string(),
        ];
        argv.extend(args.iter().map(|a| a.to_string()));
        argv
    }

    /// Runs `f` with a client connected to this session's daemon.
    pub fn query<T, Fut>(&self, f: impl FnOnce(Client) -> Fut) -> T
    where
        Fut: Future<Output = T>,
    {
        let endpoint = self.endpoint();
        runtime().block_on(async move {
            let client = Client::connect(&endpoint).await.expect("connect to daemon");
            f(client).await
        })
    }

    pub fn info(&self) -> SessionInfo {
        self.query(|mut c| async move { c.info().await.expect("info") })
    }

    pub fn nodes(&self) -> Vec<NodeInfo> {
        self.query(|mut c| async move { c.list().await.expect("list") })
    }

    /// The node called `name`; panics when the daemon does not list it.
    pub fn node(&self, name: &str) -> NodeInfo {
        self.nodes()
            .into_iter()
            .find(|n| n.name == name)
            .unwrap_or_else(|| panic!("no node named {name}"))
    }

    /// Captured output of the node so far, as the daemon stores it.
    pub fn logs(&self, name: &str) -> String {
        let name = name.to_string();
        self.query(|mut c| async move { c.logs(&name, None, true, None).await.expect("logs").0 })
    }

    fn wait_ready(&mut self) {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            if run_command(self, &["has-session"]) == 0 {
                return;
            }
            if let Some(code) = self.daemon.as_ref().and_then(|rx| rx.try_recv().ok()) {
                panic!("daemon exited before it answered (code {code})");
            }
            assert!(Instant::now() < deadline, "daemon did not start in time");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if self.daemon.is_none() {
            return;
        }
        run_command(self, &["kill-server"]);
        if let Some(rx) = self.daemon.take() {
            let _ = rx.recv_timeout(TIMEOUT);
        }
    }
}

/// Runs `args` against `f`'s session through the public CLI dispatch and returns the exit code.
/// Usage errors exit with clap's code; command errors print their message and exit 1, as
/// `main_entry` does.
pub fn run_command(f: &Fixture, args: &[&str]) -> i32 {
    let cli = match Cli::try_parse_from(f.argv(args)) {
        Ok(cli) => cli,
        Err(e) => return e.exit_code(),
    };
    runtime().block_on(run_cli(cli)).unwrap_or_else(|message| {
        eprintln!("{message}");
        1
    })
}
