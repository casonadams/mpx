//! Binary frames for stream events. Pane output is the bulk of what the daemon streams, and as JSON
//! every control character costs six bytes to escape and parse again; as a frame it is raw bytes.
//! Every other event stays JSON inside its frame. A connection switches to frames only after the
//! client asked for them and the daemon agreed (`Subscribe { binary }` / `Response::BinaryStream`),
//! so either side may be an older mpx that only speaks JSON lines.
//!
//! A frame's payload (its length prefix comes from the codec) starts with a kind byte:
//! - `OUTPUT`: name length (u16, big-endian), name, `offset` (u64), `next` (u64), then the data;
//! - `JSON`: the event as JSON.
use crate::protocol::StreamEvent;

const JSON: u8 = 0;
const OUTPUT: u8 = 1;
/// Kind byte, name length, offset and next.
const OUTPUT_HEADER: usize = 1 + 2 + 8 + 8;

/// Longest frame either side accepts.
pub const MAX_FRAME: usize = 64 * 1024 * 1024;

/// Appends the payload of `event`'s frame to `out`.
pub fn encode_event(event: &StreamEvent, out: &mut Vec<u8>) -> Result<(), String> {
    match event {
        StreamEvent::Output {
            name,
            data,
            offset,
            next,
        } => {
            let name_len = u16::try_from(name.len()).map_err(|_| "pane name too long")?;
            out.reserve(OUTPUT_HEADER + name.len() + data.len());
            out.push(OUTPUT);
            out.extend_from_slice(&name_len.to_be_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&offset.to_be_bytes());
            out.extend_from_slice(&next.to_be_bytes());
            out.extend_from_slice(data.as_bytes());
        }
        other => {
            out.push(JSON);
            serde_json::to_writer(&mut *out, other).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// The event carried by one frame's payload.
pub fn decode_event(frame: &[u8]) -> Result<StreamEvent, String> {
    let (&kind, rest) = frame.split_first().ok_or("empty frame")?;
    match kind {
        OUTPUT => decode_output(rest),
        JSON => serde_json::from_slice(rest).map_err(|e| format!("bad event frame: {e}")),
        other => Err(format!("unknown frame kind {other}")),
    }
}

fn decode_output(rest: &[u8]) -> Result<StreamEvent, String> {
    let short = || "short output frame".to_string();
    let (len, rest) = rest.split_first_chunk::<2>().ok_or_else(short)?;
    let name_len = u16::from_be_bytes(*len) as usize;
    let (name, rest) = rest.split_at_checked(name_len).ok_or_else(short)?;
    let (offset, rest) = rest.split_first_chunk::<8>().ok_or_else(short)?;
    let (next, data) = rest.split_first_chunk::<8>().ok_or_else(short)?;
    let text = |bytes: &[u8], what: &str| {
        String::from_utf8(bytes.to_vec()).map_err(|_| format!("output frame {what} is not UTF-8"))
    };
    Ok(StreamEvent::Output {
        name: text(name, "name")?,
        data: text(data, "data")?,
        offset: u64::from_be_bytes(*offset),
        next: u64::from_be_bytes(*next),
    })
}
