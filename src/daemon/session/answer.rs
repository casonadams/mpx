//! Response builders that the request handlers share.

use crate::protocol::{NodeInfo, Response};

/// The answer to a request that the connection serves, not the session.
pub(super) fn connection_only() -> Response {
    Response::Error("Request is handled by the connection".to_string())
}

pub(super) fn not_found(name: &str) -> String {
    format!("Node '{}' not found", name)
}

pub(super) fn unit(result: Result<(), String>) -> Response {
    match result {
        Ok(()) => Response::Ok,
        Err(e) => Response::Error(e),
    }
}

pub(super) fn check(ok: bool, error: &str) -> Response {
    if ok {
        Response::Ok
    } else {
        Response::Error(error.to_string())
    }
}

pub(super) fn node_reply(result: Result<NodeInfo, String>) -> Response {
    match result {
        Ok(info) => Response::Node(Box::new(info)),
        Err(e) => Response::Error(e),
    }
}

/// `ok` applied to the value, or the error.
pub(in crate::daemon) fn reply<T>(
    result: Result<T, String>,
    ok: impl FnOnce(T) -> Response,
) -> Response {
    match result {
        Ok(value) => ok(value),
        Err(e) => Response::Error(e),
    }
}
