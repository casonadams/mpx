//! `choose-client`: the attached clients of every session. Enter detaches the highlighted one.
use std::io::{self, Write};

use crossterm::event::{KeyCode, KeyEvent};

use crate::clients::{self, ClientRow, DEFAULT_CLIENT_FORMAT};
use crate::format;
use crate::layout::Rect;
use crate::tui::command::{ClientCommand, Command};
use crate::tui::overlay::nav::{Filter, ListState, route_list_key};
use crate::tui::overlay::widgets::{self, Chrome, ListPanel, Span, printable};
use crate::tui::overlay::{OverlayAction, OverlayCtx};

const HINTS: &str = "j/k move  Enter detach  / filter  q close";

#[derive(Default)]
pub struct ClientView {
    clients: Vec<ClientRow>,
    filter: Filter,
    list: ListState,
}

impl ClientView {
    /// Reloads the clients; a failed lookup keeps the list shown before.
    pub async fn refresh(&mut self, ctx: &mut OverlayCtx<'_>) {
        self.clients = clients::attached_clients(ctx.socket_dir).await;
        let len = self.visible().len();
        self.list.clamp(len);
    }

    /// Clients that pass the filter, in session order.
    pub fn visible(&self) -> Vec<&ClientRow> {
        self.filter.visible(&self.clients, |r| {
            [r.info.name.as_str(), r.info.session.as_str()]
        })
    }

    /// The line `choose-client` shows for `row`: the default `list-clients` format.
    fn line(&self, row: &ClientRow) -> String {
        let attached = clients::session_rows(&self.clients, &row.info.session).len();
        format::expand(
            DEFAULT_CLIENT_FORMAT,
            &format::client_vars(&row.info, attached),
        )
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> OverlayAction {
        let len = self.visible().len();
        if route_list_key(&mut self.filter, &mut self.list, key, len) {
            return OverlayAction::None;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => OverlayAction::Close,
            KeyCode::Enter => match self.visible().get(self.list.cursor) {
                Some(row) => {
                    OverlayAction::Run(vec![Command::Client(ClientCommand::DetachClients {
                        target: Some(row.info.name.clone()),
                        all_others: false,
                    })])
                }
                None => OverlayAction::Close,
            },
            _ => OverlayAction::None,
        }
    }

    /// Draws inside `area`.
    pub fn render<W: Write>(&self, out: &mut W, area: Rect) -> io::Result<()> {
        widgets::render_list_view(self, out, area)
    }
}

impl widgets::ListView for ClientView {
    fn panel(&self) -> ListPanel<'_> {
        ListPanel {
            chrome: Chrome::Panel,
            title: "Clients",
            footer: HINTS,
            prompt: self.filter.prompt(),
            empty: Some("no clients"),
        }
    }

    fn rows(&self) -> impl Iterator<Item = Vec<Span>> + '_ {
        self.visible()
            .into_iter()
            .map(|row| vec![Span::plain(printable(&self.line(row)))])
    }

    fn cursor(&self) -> &ListState {
        &self.list
    }
}
