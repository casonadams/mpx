use serde::{Deserialize, Serialize};
use crate::layout::{Direction, Divider, Rect, WindowTab};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    pub name: String,
    pub pid: Option<u32>,
    pub pty: bool,
    pub command: String,
    pub status: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitOpts {
    pub target: Option<String>,
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub pty: bool,
    pub direction: Direction,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewWindowOpts {
    pub name: Option<String>,
    pub command: String,
    pub args: Vec<String>,
    pub pty: bool,
    pub cwd: Option<String>,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum Request {
    Ping,
    Spawn {
        name: String,
        command: String,
        args: Vec<String>,
        pty: bool,
        cwd: Option<String>,
    },
    List,
    Logs {
        name: String,
        tail: Option<usize>,
    },
    Send {
        name: String,
        data: String,
    },
    Kill {
        name: String,
    },
    Wait {
        name: String,
    },
    Subscribe {
        name: Option<String>,
    },
    Screen {
        name: String,
    },
    Split(SplitOpts),
    Layout {
        width: u16,
        height: u16,
    },
    FocusNext,
    FocusPrev,
    Focus {
        name: String,
    },
    ResizePane {
        name: String,
        cols: u16,
        rows: u16,
    },
    NewWindow(NewWindowOpts),
    NextWindow,
    PrevWindow,
    SelectWindow {
        index: usize,
    },
    GetCwd {
        name: String,
    },
    CreatePipe {
        name: String,
        capacity: Option<usize>,
    },
    WritePipe {
        name: String,
        data: String,
    },
    ReadPipe {
        name: String,
    },
    ListPipes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", content = "data")]
pub enum Response {
    Ok,
    Pong,
    Node(NodeInfo),
    NodeList(Vec<NodeInfo>),
    Logs(String),
    Exit { code: Option<i32> },
    Error(String),
    Screen(String),
    Layout(Vec<(String, Rect, bool)>, Vec<Divider>, Vec<WindowTab>),
    PipeData(String),
    PipeList(Vec<String>),
    Cwd(Option<String>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", content = "data")]
pub enum StreamEvent {
    Output { name: String, data: String },
    Exit { name: String, code: Option<i32> },
    Spawned { name: String, pid: Option<u32> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DaemonMessage {
    Response(Response),
    Event(StreamEvent),
}
