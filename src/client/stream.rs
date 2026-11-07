use futures_util::StreamExt;
use tokio::sync::mpsc;
use tokio_util::codec::{Framed, FramedParts, LengthDelimitedCodec};

use super::Client;
use crate::protocol::{ClientInfo, DaemonMessage, Request, Response, StreamEvent};
use crate::wire;

impl Client {
    /// Streams node events; the handler returns false to stop. With `name` and `since`, output from
    /// that byte offset is replayed first without a gap.
    pub async fn stream_logs<F>(
        &mut self,
        name: Option<String>,
        since: Option<u64>,
        mut handler: F,
    ) -> Result<(), String>
    where
        F: FnMut(StreamEvent) -> bool,
    {
        let req = Request::Subscribe {
            name,
            since,
            binary: false,
            active_window: false,
            layout: false,
            client: None,
            exclusive: false,
            hangup: false,
        };
        request!(self, "Subscribe", req, Response::Ok => ())?;
        while let Some(event) = self.next_event().await? {
            if !handler(event) {
                break;
            }
        }
        Ok(())
    }

    /// Like `stream_logs`, but hands each event, wrapped by `wrap`, to `tx` and waits while it is
    /// full: a reader that falls behind stops reading, which slows the daemon's delivery instead of
    /// piling events up in memory. Events come as binary frames (`wire`) when the daemon offers
    /// them, as JSON lines otherwise. With `active_window`, pane output comes only for the active
    /// window's panes (a daemon that predates it sends all), and `Layout` events come when windows
    /// or panes are rearranged. Ends when the daemon or `tx`'s receiver goes away.
    pub async fn stream_logs_into<T>(
        self,
        (name, since): (Option<String>, Option<u64>),
        active_window: bool,
        tx: mpsc::Sender<T>,
        wrap: impl Fn(StreamEvent) -> T,
    ) -> Result<(), String> {
        let req = Request::Subscribe {
            name,
            since,
            binary: true,
            active_window,
            layout: active_window,
            client: None,
            exclusive: false,
            hangup: false,
        };
        self.stream_subscription(req, tx, wrap).await
    }

    /// Subscribes as the terminal `info`: the session registers it as a client and sends it the
    /// session's events, with its window laid out and its size counted. With `exclusive` the
    /// session first detaches the clients attached before it (`attach -d`); with `hangup` too, it
    /// hangs up their parents (`attach -x`).
    pub async fn stream_as_client<T>(
        self,
        info: ClientInfo,
        exclusive: bool,
        hangup: bool,
        tx: mpsc::Sender<T>,
        wrap: impl Fn(StreamEvent) -> T,
    ) -> Result<(), String> {
        let req = Request::Subscribe {
            name: None,
            since: None,
            binary: true,
            active_window: true,
            layout: true,
            client: Some(info),
            exclusive,
            hangup,
        };
        self.stream_subscription(req, tx, wrap).await
    }

    async fn stream_subscription<T>(
        mut self,
        req: Request,
        tx: mpsc::Sender<T>,
        wrap: impl Fn(StreamEvent) -> T,
    ) -> Result<(), String> {
        match self.call(req).await? {
            Response::BinaryStream => return self.forward_frames(tx, wrap).await,
            Response::Ok => {}
            other => return Err(crate::client::unexpected("Subscribe", other)),
        }
        while let Some(event) = self.next_event().await? {
            if tx.send(wrap(event)).await.is_err() {
                break;
            }
        }
        Ok(())
    }

    /// Reads the rest of the connection as binary frames, starting with any bytes already buffered
    /// after the daemon's answer.
    async fn forward_frames<T>(
        self,
        tx: mpsc::Sender<T>,
        wrap: impl Fn(StreamEvent) -> T,
    ) -> Result<(), String> {
        let parts = self.framed.into_parts();
        let codec = LengthDelimitedCodec::builder()
            .max_frame_length(wire::MAX_FRAME)
            .new_codec();
        // Built from parts so bytes read along with the daemon's answer are decoded at once,
        // without waiting for more to arrive.
        let mut binary = FramedParts::new::<bytes::Bytes>(parts.io, codec);
        binary.read_buf = parts.read_buf;
        let mut frames = Framed::from_parts(binary);
        while let Some(frame) = frames.next().await {
            let frame = frame.map_err(|e| e.to_string())?;
            // An event this client does not know (from a newer daemon) is skipped, as the JSON
            // stream does; a skipped output chunk shows up as a gap in the offsets.
            let Ok(event) = wire::decode_event(&frame) else {
                continue;
            };
            if tx.send(wrap(event)).await.is_err() {
                break;
            }
        }
        Ok(())
    }

    /// The next event of a JSON stream; other lines are skipped. `None` once the connection ends.
    async fn next_event(&mut self) -> Result<Option<StreamEvent>, String> {
        while let Some(line) = self.framed.next().await {
            let line = line.map_err(|e| e.to_string())?;
            if let Ok(DaemonMessage::Event(event)) = serde_json::from_str(&line) {
                return Ok(Some(event));
            }
        }
        Ok(None)
    }
}
