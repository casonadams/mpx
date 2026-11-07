//! Session-wide subscribers: what each one receives, the attached terminals (clients) among them,
//! and the fanout of session events to them.

use std::sync::atomic::Ordering;

use super::Session;
use super::answer::{not_found, unit};
use crate::clients::effective_size;
use crate::daemon::clients;
use crate::node::now_ms;
use crate::node::pane::PaneHandle;
use crate::node::sink::Sink;
use crate::protocol::{ClientInfo, ClientRequest, Response, StreamEvent};

/// Which session-wide events a subscriber receives.
pub(in crate::daemon) enum Filter {
    Everything,
    /// What a TUI shows: output of the active window's panes, every other pane event, and
    /// `Layout` events when `layout` is set.
    ActiveWindow {
        layout: bool,
    },
}

impl Filter {
    /// Whether `event` goes to this subscriber; `shown` says if a pane is in the active window.
    fn accepts(&self, event: &StreamEvent, shown: impl Fn(&str) -> bool) -> bool {
        match (self, event) {
            (_, StreamEvent::Layout | StreamEvent::Bindings | StreamEvent::Respawned { .. }) => {
                matches!(self, Filter::ActiveWindow { layout: true })
            }
            (Filter::Everything, _) => true,
            (Filter::ActiveWindow { .. }, StreamEvent::Output { name, .. }) => shown(name),
            (Filter::ActiveWindow { .. }, _) => true,
        }
    }

    /// What a subscription asked for: a terminal's view (`active_window`), with or without layout
    /// events, or everything.
    pub(in crate::daemon) fn subscriber(active_window: bool, layout: bool) -> Self {
        if active_window {
            Filter::ActiveWindow { layout }
        } else {
            Filter::Everything
        }
    }

    /// Session-wide: wants events from every pane, so panes must relay them.
    fn session_wide(&self) -> bool {
        matches!(self, Filter::Everything | Filter::ActiveWindow { .. })
    }
}

/// A session-wide subscriber: what it sees and, when it is a terminal, how it attaches.
pub(in crate::daemon) struct Subscription {
    pub filter: Filter,
    /// The terminal attaching; None for a subscriber that is not a client.
    pub client: Option<ClientInfo>,
    /// Attaching detaches the clients attached before it (`attach -d`).
    pub exclusive: bool,
    /// With `exclusive`, also hangs up those clients' parents (`attach -x`).
    pub hangup: bool,
}

/// A session-wide subscriber: what it receives, where events go, and the terminal it is, if any.
pub(in crate::daemon) struct Subscriber {
    pub filter: Filter,
    pub sink: Sink,
    pub client: Option<ClientInfo>,
}

impl Session {
    /// Adds a session-wide subscriber. A terminal (`client`) is registered with this session; with
    /// `exclusive` (`attach -d`) it first detaches the clients attached before it, and with `hangup`
    /// (`attach -x`) it also hangs up their parents. Its arrival or departure can change the window
    /// size the policy picks, so every subscriber lays out again.
    pub(super) fn subscribe(&mut self, sub: Subscription, sink: Sink) {
        let Subscription {
            filter,
            client,
            exclusive,
            hangup,
        } = sub;
        let client = client.map(|info| ClientInfo {
            session: self.session.clone(),
            activity_ms: now_ms(),
            ..info
        });
        if let (true, Some(info)) = (exclusive, &client) {
            let parents = clients::detach_others(&mut self.filters, &info.name);
            if hangup {
                clients::hang_up(&parents);
            }
        }
        let terminal = client.is_some();
        self.filters.push(Subscriber {
            filter,
            sink,
            client,
        });
        self.sync_relay();
        if terminal {
            self.fanout(StreamEvent::Layout);
        }
    }

    /// Attaches a subscriber to a pane, or holds it until a pane of that name is spawned.
    pub(super) fn subscribe_node(
        &mut self,
        name: String,
        since: Option<u64>,
        sink: Sink,
    ) -> Result<(), String> {
        match self.handle_of(&name) {
            Ok(handle) => {
                handle.attach(since, sink);
                Ok(())
            }
            Err(_) if name.starts_with('%') => Err(not_found(&name)),
            Err(_) => {
                self.pending.push((name, since, sink));
                Ok(())
            }
        }
    }

    /// Attaches subscribers that were waiting for a pane named `name`, replaying from the start so
    /// nothing the pane printed before the attach is lost.
    pub(super) fn attach_pending(&mut self, name: &str, handle: &PaneHandle) {
        let (ready, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut self.pending)
            .into_iter()
            .filter(|(_, _, sink)| !sink.is_closed())
            .partition(|(want, _, _)| want.as_str() == name);
        self.pending = waiting;
        for (_, since, sink) in ready {
            handle.attach(Some(since.unwrap_or(0)), sink);
        }
    }

    /// The attached clients of this session, and the requests that detach, resize or message them.
    pub(super) fn client_request(&mut self, req: ClientRequest) -> Response {
        match req {
            ClientRequest::List => Response::Clients(clients::list(&self.filters, &self.session)),
            ClientRequest::Detach { target } => {
                let detached = clients::detach(&mut self.filters, &target);
                self.relayout_if(detached.map(|_| true))
            }
            ClientRequest::Activity { name } => unit(clients::touch(&mut self.filters, &name)),
            ClientRequest::Resized {
                name,
                width,
                height,
            } => {
                let resized = clients::resize(&mut self.filters, &name, width, height);
                self.relayout_if(resized)
            }
            ClientRequest::Refresh { name, size } => {
                let refreshed = clients::refresh(&mut self.filters, &name, size);
                self.relayout_if(refreshed)
            }
            ClientRequest::Command { name, line } => unit(clients::send(
                &mut self.filters,
                &name,
                StreamEvent::Run { line },
            )),
        }
    }

    /// Lays out again when a client's change moved a size.
    fn relayout_if(&mut self, changed: Result<bool, String>) -> Response {
        match changed {
            Ok(changed) => {
                if changed {
                    self.fanout(StreamEvent::Layout);
                }
                Response::Ok
            }
            Err(e) => Response::Error(e),
        }
    }

    /// Re-reports the layout when a client that sized a window has gone.
    pub(super) fn check_clients(&mut self) {
        if clients::gone(&self.filters) {
            self.fanout(StreamEvent::Layout);
        }
    }

    /// The size the window is laid out for: the window-size policy over the attached clients, or
    /// `requested` when none is attached.
    pub(super) fn window_size(&self, requested: (u16, u16)) -> (u16, u16) {
        effective_size(
            self.config.window.window_size,
            self.config.window.default_size,
            &clients::live(&self.filters),
            requested,
        )
    }

    /// Panes relay output to the session only while session-wide subscribers exist: relaying every
    /// chunk of every pane costs a flood about half its speed, and an attached terminal subscribes
    /// anyway.
    pub(super) fn sync_relay(&self) {
        let subscribed = self.filters.iter().any(|sub| sub.filter.session_wide());
        self.relay.store(subscribed, Ordering::Relaxed);
    }

    /// Sends `event` to every subscriber it reaches, the last of them getting the event itself
    /// rather than a copy.
    pub(super) fn fanout(&mut self, event: StreamEvent) {
        let in_active_window = match &event {
            StreamEvent::Output { name, .. } => self
                .workspace
                .active_window()
                .is_some_and(|w| w.contains(name)),
            _ => false,
        };
        let reaches = |sub: &Subscriber| sub.filter.accepts(&event, |_| in_active_window);
        let last = self.filters.iter().rposition(reaches);
        let mut pending = Some(event);
        let mut index = 0;
        self.filters.retain_mut(|sub| {
            let at = index;
            index += 1;
            let owned = match &pending {
                Some(event) if sub.filter.accepts(event, |_| in_active_window) => {
                    if Some(at) == last {
                        pending.take()
                    } else {
                        Some(event.clone())
                    }
                }
                _ => None,
            };
            match owned {
                Some(event) => sub.sink.push(event),
                None => !sub.sink.is_closed(),
            }
        });
        self.sync_relay();
    }
}
