//! Harness for app-level TUI tests. Each test gets a temp socket dir holding in-process daemons
//! (one per session), an `App` attached to the main session on a fixed screen with an in-memory
//! clipboard, and a probe client for reading daemon state. No real terminal is involved.
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use tempfile::TempDir;

use mpx::client::Client;
use mpx::config::Config;
use mpx::daemon::{DaemonState, handle_client};
use mpx::ipc::{self, IpcListener};
use mpx::layout::Direction;
use mpx::layout::geometry::Rect;
use mpx::protocol::{NewWindowOpts, SpawnOpts, SplitOpts, WaitCondition};
use mpx::tui::app::{App, AppOptions, Clipboard, Flow, Mode, StartView, StreamRx};
use mpx::tui::frame;
use mpx::vt::{Cursor, VtScreen};

pub const SESSION: &str = "test";
pub const MAIN: &str = "main";
/// Screen the app lays out for; the last row is the status row.
pub const SIZE: (u16, u16) = (100, 30);
/// Pane program: raw mode without echo, prints READY, then echoes its input back.
pub const ECHO: &str = "stty raw -echo; printf READY; exec cat";
const WAIT: Duration = Duration::from_secs(2);

/// A pty pane running `sh -c script`.
pub fn pane(name: &str, script: &str) -> SpawnOpts {
    SpawnOpts {
        name: name.to_string(),
        command: "sh".to_string(),
        args: vec!["-c".to_string(), script.to_string()],
        pty: true,
        ..SpawnOpts::default()
    }
}

/// Temp dirs for one test: the socket dir shared by all sessions, and their durable state.
pub struct Sandbox {
    pub socket_dir: PathBuf,
    state_dir: PathBuf,
    _socket: TempDir,
    _state: TempDir,
}

impl Sandbox {
    pub fn new() -> Self {
        let socket = tempfile::tempdir().expect("socket dir");
        let state = tempfile::tempdir().expect("state dir");
        Self {
            socket_dir: socket.path().to_path_buf(),
            state_dir: state.path().to_path_buf(),
            _socket: socket,
            _state: state,
        }
    }

    pub fn endpoint(&self, session: &str) -> String {
        ipc::endpoint_in(&self.socket_dir, session)
    }

    /// Starts an in-process daemon for `session` on its socket in the socket dir.
    pub async fn serve(&self, session: &str, config: &Config) {
        let endpoint = self.endpoint(session);
        let listener = IpcListener::bind(&endpoint).expect("bind socket");
        let state = DaemonState::new(
            session.to_string(),
            endpoint,
            config.clone(),
            self.state_dir.join(session),
        )
        .await;
        tokio::spawn(async move {
            while let Ok(stream) = listener.accept().await {
                tokio::spawn(handle_client(Arc::clone(&state), stream));
            }
        });
    }
}

pub async fn connect(endpoint: &str) -> Client {
    Client::connect(endpoint).await.expect("connect")
}

/// Output of `pane` once `ready` accepts it; polls the daemon's logs (bounded) otherwise.
pub async fn wait_output(client: &mut Client, pane: &str, ready: impl Fn(&str) -> bool) -> String {
    let deadline = Instant::now() + WAIT;
    loop {
        let (data, _) = client.logs(pane, None, false, None).await.expect("logs");
        if ready(&data) {
            return data;
        }
        assert!(
            Instant::now() < deadline,
            "output condition not met in {pane}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Waits (bounded) until `needle` appears in the output of `pane`.
pub async fn wait_for(client: &mut Client, pane: &str, needle: &str) -> String {
    wait_output(client, pane, |data| data.contains(needle)).await
}

/// Opens a focused window named `name` whose pane runs `script`, and waits until it prints READY.
pub async fn window(client: &mut Client, name: &str, script: &str) {
    let opts = NewWindowOpts {
        name: Some(name.to_string()),
        spawn: pane(name, script),
        focus: true,
    };
    client.new_window(opts).await.expect("new window");
    client
        .rename_window(None, name)
        .await
        .expect("rename window");
    wait_for(client, name, "READY").await;
}

/// Splits `target` with an unfocused pane `name` running `ECHO`, and waits until it prints READY.
pub async fn split(client: &mut Client, target: &str, name: &str, direction: Direction) {
    let opts = SplitOpts {
        target: Some(target.to_string()),
        spawn: pane(name, ECHO),
        direction,
        percent: None,
        focus: false,
    };
    client.split(opts).await.expect("split");
    wait_for(client, name, "READY").await;
}

/// Waits (bounded) for `name` to exit and returns its exit code.
pub async fn exited(client: &mut Client, name: &str) -> Option<i32> {
    let outcome = client
        .wait(name, WaitCondition::Exit, Some(2_000))
        .await
        .expect("wait");
    assert!(outcome.satisfied, "{name} did not exit in time");
    outcome.exit_code
}

/// An attached app plus the daemon state around it.
pub struct Harness {
    pub sandbox: Sandbox,
    pub app: App,
    pub rx: StreamRx,
    /// Separate connection to the main session, for reading daemon state.
    pub probe: Client,
    /// The terminal the app draws on; keeps its contents across frames like a real one.
    pub terminal: VtScreen,
}

impl Harness {
    /// Starts the main session with one window named `MAIN` running `script`, then attaches an app.
    pub async fn start(config: Config, script: &str) -> Self {
        let sandbox = Sandbox::new();
        sandbox.serve(SESSION, &config).await;
        let mut seed = connect(&sandbox.endpoint(SESSION)).await;
        window(&mut seed, MAIN, script).await;
        Self::attach(sandbox, SESSION, config).await
    }

    /// Attaches a fresh app to `session`, which must already be serving.
    pub async fn attach(sandbox: Sandbox, session: &str, config: Config) -> Self {
        let endpoint = sandbox.endpoint(session);
        let opts = AppOptions {
            config,
            socket_dir: sandbox.socket_dir.clone(),
            size: Some(SIZE),
        };
        let (mut app, rx) = App::open(&endpoint, session, StartView::Panes, opts)
            .await
            .expect("attach app");
        app.clipboard = Clipboard::Captured(Vec::new());
        let probe = connect(&endpoint).await;
        let mut terminal = VtScreen::new(SIZE.0 as usize, SIZE.1 as usize);
        terminal.feed(b"\x1b[?7l");
        Self {
            sandbox,
            app,
            rx,
            probe,
            terminal,
        }
    }

    pub async fn event(&mut self, ev: Event) -> Flow {
        self.app.on_event(ev).await
    }

    pub async fn key(&mut self, code: KeyCode) -> Flow {
        self.event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)))
            .await
    }

    pub async fn ctrl(&mut self, c: char) -> Flow {
        self.event(Event::Key(KeyEvent::new(
            KeyCode::Char(c),
            KeyModifiers::CONTROL,
        )))
        .await
    }

    /// Types each character as a key press.
    pub async fn type_str(&mut self, text: &str) {
        for c in text.chars() {
            self.key(KeyCode::Char(c)).await;
        }
    }

    /// Presses the prefix (C-b), then `code`.
    pub async fn prefixed(&mut self, code: KeyCode) -> Flow {
        self.ctrl('b').await;
        self.key(code).await
    }

    pub async fn prefixed_char(&mut self, c: char) -> Flow {
        self.prefixed(KeyCode::Char(c)).await
    }

    pub async fn mouse(&mut self, kind: MouseEventKind, column: u16, row: u16) -> Flow {
        self.event(Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }))
        .await
    }

    /// Refreshes layout (as the event loop does on each wake-up), draws one frame onto the terminal
    /// and returns the text the terminal then shows.
    pub async fn frame(&mut self) -> String {
        self.app.layout.dirty = true;
        assert!(self.app.prepare_frame().await, "nothing to draw");
        let mut out = Vec::new();
        frame::draw(&mut self.app, &mut out).expect("draw");
        self.terminal.feed(&out);
        self.terminal.snapshot()
    }

    /// The terminal's cursor after the last frame.
    pub fn cursor(&self) -> Cursor {
        self.terminal.grid().cursor
    }

    /// Applies pane output until `ready` holds for the app; fails after a bounded wait.
    pub async fn until(&mut self, what: &str, mut ready: impl FnMut(&App) -> bool) {
        let deadline = tokio::time::Instant::now() + WAIT;
        while !ready(&self.app) {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            assert!(!left.is_zero(), "app never reached: {what}");
            if let Ok(item) = tokio::time::timeout(left, self.rx.recv()).await {
                let item = item.expect("event stream closed");
                self.app.on_stream(Some(item), &mut self.rx).await;
            }
        }
    }

    pub fn mode_is_normal(&self) -> bool {
        matches!(self.app.mode, Mode::Normal)
    }

    pub fn clipboard(&self) -> Vec<String> {
        match &self.app.clipboard {
            Clipboard::Captured(texts) => texts.clone(),
            Clipboard::System => panic!("clipboard must be captured in tests"),
        }
    }

    /// Pane names of the daemon's active window, in layout order.
    pub async fn layout_panes(&mut self) -> Vec<String> {
        let (rects, _, _) = self.probe.layout(SIZE.0, SIZE.1 - 1).await.expect("layout");
        rects.into_iter().map(|(name, ..)| name).collect()
    }

    /// Window names in tab order.
    pub async fn windows(&mut self) -> Vec<String> {
        let (_, _, tabs) = self.probe.layout(SIZE.0, SIZE.1 - 1).await.expect("layout");
        tabs.into_iter().map(|tab| tab.name).collect()
    }

    /// Every node name in the session, including exited ones kept on screen.
    pub async fn nodes(&mut self) -> Vec<String> {
        let nodes = self.probe.list().await.expect("list");
        nodes.into_iter().map(|node| node.name).collect()
    }

    pub async fn logs(&mut self, pane: &str) -> String {
        let (data, _) = self
            .probe
            .logs(pane, None, false, None)
            .await
            .expect("logs");
        data
    }

    /// Rectangle of `pane` in the last prepared frame.
    pub fn rect_of(&self, pane: &str) -> Rect {
        self.app
            .panes
            .iter()
            .find(|(name, ..)| name == pane)
            .map(|(_, rect, _)| *rect)
            .unwrap_or_else(|| panic!("{pane} is not visible"))
    }
}
