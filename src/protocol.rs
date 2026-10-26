use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    pub name: String,
    pub pid: Option<u32>,
    pub pty: bool,
    pub command: String,
    pub status: String,
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
