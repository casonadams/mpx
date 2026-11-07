//! Draws the tree rows and the preview of the selected pane.
use std::io::{self, Write};

use super::{Node, TreePane, TreeView, TreeWindow};
use crate::layout::Rect;
use crate::tui::overlay::widgets::{
    self, Span, Tone, age_label, fit, join_meta, now_ms, short_path, tail_lines,
};

const HINTS: &str = "j/k move  l/Enter open  h collapse  / filter  x kill  p preview  q close";
const PREVIEW_MIN_HEIGHT: u16 = 14;
const TAG_COLS: usize = 10;
const DETAIL_COLS: usize = 24;
/// Rows narrower than this drop the detail column.
const DETAIL_MIN_WIDTH: usize = 72;
const TALLY_COLS: usize = 9;
const AGE_COLS: usize = 4;
const FRESH_MS: u64 = 60_000;

/// Pane count, the exited-pane span and the newest output time over a set of panes.
fn tally_of<'a>(panes: impl Iterator<Item = &'a TreePane>) -> (usize, Vec<Span>, u64) {
    let (mut count, mut exited, mut last_output_ms) = (0, 0, 0);
    for pane in panes {
        count += 1;
        exited += usize::from(!pane.running);
        last_output_ms = last_output_ms.max(pane.last_output_ms);
    }
    let spans = if exited > 0 {
        vec![Span::toned(format!("{exited} exited"), Tone::Failed)]
    } else {
        Vec::new()
    };
    (count, spans, last_output_ms)
}

/// One rendered row before it is laid out into columns.
struct Cells {
    lead: Vec<Span>,
    name: Span,
    tag: Span,
    detail: String,
    tally: Vec<Span>,
    last_output_ms: u64,
}

fn plural(n: usize, one: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {one}s")
    }
}

/// Lays `cells` out as: bar, lead, name, tag, detail, tally, age.
fn row_spans(cells: Cells, width: usize, selected: bool) -> Vec<Span> {
    let show_detail = width >= DETAIL_MIN_WIDTH;
    let detail_w = if show_detail { DETAIL_COLS + 1 } else { 0 };
    let lead_w: usize = cells.lead.iter().map(|s| s.text.chars().count()).sum();
    let fixed = 1 + lead_w + 1 + TAG_COLS + 1 + detail_w + TALLY_COLS + AGE_COLS;
    let name_w = width.saturating_sub(fixed);
    let bar = if selected {
        Span::bold("▌", Tone::Accent)
    } else {
        Span::plain(" ")
    };
    let mut spans = vec![bar];
    spans.extend(cells.lead);
    spans.push(Span {
        text: fit(&cells.name.text, name_w),
        ..cells.name
    });
    spans.push(Span::plain(" "));
    spans.push(Span {
        text: format!("{} ", fit(&cells.tag.text, TAG_COLS)),
        ..cells.tag
    });
    if show_detail {
        spans.push(Span::dim(format!("{} ", fit(&cells.detail, DETAIL_COLS))));
    }
    let tally_w: usize = cells.tally.iter().map(|s| s.text.chars().count()).sum();
    spans.extend(cells.tally);
    spans.push(Span::plain(" ".repeat(TALLY_COLS.saturating_sub(tally_w))));
    spans.push(age_span(cells.last_output_ms));
    spans
}

fn age_span(last_ms: u64) -> Span {
    if last_ms == 0 {
        return Span::plain(" ".repeat(AGE_COLS));
    }
    let elapsed = now_ms().saturating_sub(last_ms);
    let age = format!("{:>w$}", age_label(elapsed), w = AGE_COLS);
    if elapsed < FRESH_MS {
        Span::plain(age)
    } else {
        Span::dim(age)
    }
}

impl TreeView {
    /// Draws inside `area`: the tree, plus a preview of the selected pane when there is room.
    pub fn render<W: Write>(&self, out: &mut W, area: Rect) -> io::Result<()> {
        if self.preview_off || self.filter.editing || area.height < PREVIEW_MIN_HEIGHT {
            return self.render_list(out, area);
        }
        let (list, preview) = widgets::split_rows(area, area.height / 2);
        self.render_list(out, list)?;
        self.render_preview(out, preview)
    }

    fn render_list<W: Write>(&self, out: &mut W, area: Rect) -> io::Result<()> {
        let title = format!("Sessions · {}", self.sessions.len());
        widgets::draw_panel(out, area, &title, HINTS)?;
        let prompt = self.filter.prompt();
        let rows = widgets::body(area, prompt.is_some());
        if let Some(text) = prompt {
            widgets::draw_prompt(out, area, &text)?;
        }
        let nodes = self.rows();
        if nodes.is_empty() {
            return widgets::put_row(out, rows, 0, &[Span::dim("no sessions")], false);
        }
        let width = rows.width as usize;
        let cursor = self.list.cursor;
        for (row, idx) in self
            .list
            .window(cursor, nodes.len(), rows.height as usize)
            .enumerate()
        {
            let on = idx == cursor;
            let spans = row_spans(self.cells(nodes[idx], on), width, on);
            widgets::put_row(out, rows, row as u16, &spans, false)?;
        }
        Ok(())
    }

    fn render_preview<W: Write>(&self, out: &mut W, area: Rect) -> io::Result<()> {
        let Some((s, w, p)) = self.previewed() else {
            return widgets::draw_preview(out, area, None, &[], None);
        };
        let pane = self.pane(s, w, p);
        let key = self.pane_target(s, w, p);
        let meta = vec![
            Span::bold(pane.name.clone(), Tone::Plain),
            Span::dim(format!(
                " · {}",
                join_meta(&[&pane.program, &self.short_cwd(pane), &key])
            )),
        ];
        let text = self
            .screen
            .as_ref()
            .filter(|(k, _)| *k == key)
            .map_or("", |(_, t)| t.as_str());
        let lines: Vec<Span> = tail_lines(text, widgets::preview_height(area, true))
            .into_iter()
            .map(Span::plain)
            .collect();
        widgets::draw_preview(
            out,
            area,
            Some(meta),
            &lines,
            Some("nothing to preview yet"),
        )
    }

    fn short_cwd(&self, pane: &TreePane) -> String {
        short_path(pane.cwd.as_deref().unwrap_or(""), self.home.as_deref())
    }

    fn arrow(&self, node: Node) -> char {
        if self.children(node).is_empty() {
            ' '
        } else if self.expanded(node) {
            '▾'
        } else {
            '▸'
        }
    }

    fn cells(&self, node: Node, selected: bool) -> Cells {
        match node {
            Node::Session(s) => self.session_cells(node, s, selected),
            Node::Window(s, w) => self.window_cells(node, self.window(s, w), selected),
            Node::Pane(s, w, p) => self.pane_cells(self.pane(s, w, p), selected),
        }
    }

    fn session_cells(&self, node: Node, s: usize, selected: bool) -> Cells {
        let session = &self.sessions[s];
        let current = session.name == self.current;
        let tone = if current { Tone::Active } else { Tone::Plain };
        let name = if current || selected {
            Span::bold(session.name.clone(), tone)
        } else {
            Span::plain(session.name.clone())
        };
        let tag = if current {
            Span::toned("current", Tone::Active)
        } else {
            Span::plain("")
        };
        let (panes, tally, last_output_ms) =
            tally_of(session.windows.iter().flat_map(|w| &w.panes));
        Cells {
            lead: vec![Span::plain(format!("{} ", self.arrow(node)))],
            name,
            tag,
            detail: format!(
                "{} · {}",
                plural(session.windows.len(), "window"),
                plural(panes, "pane")
            ),
            tally,
            last_output_ms,
        }
    }

    fn window_cells(&self, node: Node, window: &TreeWindow, selected: bool) -> Cells {
        let (panes, tally, last_output_ms) = tally_of(window.panes.iter());
        let label = format!("{} {}", window.index, window.name);
        Cells {
            lead: vec![Span::plain(format!("  {} ", self.arrow(node)))],
            name: if selected {
                Span::bold(label, Tone::Plain)
            } else {
                Span::plain(label)
            },
            tag: Span::dim(if window.active { "active" } else { "" }),
            detail: plural(panes, "pane"),
            tally,
            last_output_ms,
        }
    }

    fn pane_cells(&self, pane: &TreePane, selected: bool) -> Cells {
        let focus = if pane.active { '›' } else { ' ' };
        let (glyph, tone) = if pane.running {
            ('●', Tone::Accent)
        } else {
            ('×', Tone::Failed)
        };
        let name = match (selected, pane.running) {
            (true, _) => Span::bold(pane.name.clone(), Tone::Plain),
            (false, true) => Span::plain(pane.name.clone()),
            (false, false) => Span::dim(pane.name.clone()),
        };
        let tag = match (pane.running, pane.exit_code) {
            (true, _) => Span::toned(pane.program.clone(), Tone::Accent),
            (false, Some(code)) => Span::toned(format!("exit {code}"), Tone::Failed),
            (false, None) => Span::toned("exited", Tone::Failed),
        };
        Cells {
            lead: vec![
                Span::toned(format!("     {focus}"), Tone::Accent),
                Span::toned(format!("{glyph} "), tone),
            ],
            name,
            tag,
            detail: self.short_cwd(pane),
            tally: Vec::new(),
            last_output_ms: pane.last_output_ms,
        }
    }
}
