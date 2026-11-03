//! display-menu: a list of command lines drawn over the panes. Enter or an item's key runs its
//! command; `q` and Esc close the menu.
use std::io::{self, Write};

use crossterm::cursor::MoveTo;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use crossterm::{
    queue,
    style::{Attribute, Print, SetAttribute},
};

use super::overlay::widgets::{draw_box, inner};
use super::placement::Offset;
use crate::keys::KeySpec;
use crate::layout::geometry::Rect;

/// One `display-menu` entry: its text, the key that picks it and the command line it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    /// Text shown; empty for a separator line.
    pub name: String,
    /// `-` before the name in the command: shown, but never selected.
    pub disabled: bool,
    /// Key that runs the item, in tmux notation.
    pub key: Option<String>,
    /// Command line the item runs; `None` for a separator.
    pub line: Option<String>,
}

impl MenuItem {
    fn selectable(&self) -> bool {
        !self.disabled && self.line.is_some()
    }
}

/// What a key does to an open menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuAction {
    Stay,
    Close,
    /// Close the menu, then run this command line.
    Run(String),
}

/// An open menu: its items, the selected one and the box it is drawn in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuState {
    pub title: String,
    pub items: Vec<MenuItem>,
    pub selected: usize,
    pub rect: Rect,
}

impl MenuState {
    /// A menu placed at `x` and `y` on a `screen` of the given size, with its first selectable item
    /// selected.
    pub fn new(
        title: String,
        items: Vec<MenuItem>,
        x: Offset,
        y: Offset,
        screen: (u16, u16),
    ) -> Self {
        let rect = menu_rect(&title, &items, x, y, screen);
        let selected = items.iter().position(MenuItem::selectable).unwrap_or(0);
        Self {
            title,
            items,
            selected,
            rect,
        }
    }

    /// The selectable item an item key picks, if any.
    pub fn item_for_key(&self, key: &KeyEvent) -> Option<usize> {
        self.items.iter().position(|item| {
            item.selectable()
                && item
                    .key
                    .as_deref()
                    .and_then(|spec| KeySpec::parse(spec).ok())
                    .is_some_and(|spec| spec.matches(key))
        })
    }

    /// Moves the selection by `delta` items, skipping separators and disabled items; stops at the
    /// ends of the menu.
    pub fn move_by(&mut self, delta: isize) {
        let step = delta.signum();
        let mut at = self.selected;
        let mut left = delta.unsigned_abs();
        while left > 0 {
            let next = at as isize + step;
            if next < 0 || next >= self.items.len() as isize {
                break;
            }
            at = next as usize;
            if self.items[at].selectable() {
                self.selected = at;
                left -= 1;
            }
        }
    }

    /// The command line of the item at `index`, when it is selectable.
    fn line_at(&self, index: usize) -> MenuAction {
        match self.items.get(index) {
            Some(item) if item.selectable() => {
                item.line.clone().map_or(MenuAction::Stay, MenuAction::Run)
            }
            _ => MenuAction::Stay,
        }
    }

    /// What a key does: an item's key runs the item; the arrows and `j`/`k` move; Enter runs the
    /// selected item; `q` and Esc close.
    pub fn handle_key(&mut self, key: KeyEvent) -> MenuAction {
        if let Some(index) = self.item_for_key(&key) {
            return self.line_at(index);
        }
        let plain = key.modifiers == KeyModifiers::NONE;
        match key.code {
            KeyCode::Esc => MenuAction::Close,
            KeyCode::Char('q') if plain => MenuAction::Close,
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_by(1);
                MenuAction::Stay
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_by(-1);
                MenuAction::Stay
            }
            KeyCode::Enter => self.line_at(self.selected),
            _ => MenuAction::Stay,
        }
    }

    /// Draws the box and its items; the selected item is reversed.
    pub fn render<W: Write>(&self, out: &mut W) -> io::Result<()> {
        draw_box(out, self.rect, &self.title, "")?;
        let body = inner(self.rect);
        let width = body.width as usize;
        let key_width = key_width(&self.items);
        for (row, item) in self.items.iter().enumerate().take(body.height as usize) {
            let y = body.y + row as u16;
            queue!(out, MoveTo(body.x, y), SetAttribute(Attribute::Reset))?;
            if row == self.selected {
                queue!(out, SetAttribute(Attribute::Reverse))?;
            }
            let text = if item.name.is_empty() && item.line.is_none() {
                "─".repeat(width)
            } else {
                let name = if item.disabled {
                    format!("  {}", item.name)
                } else {
                    format!(" {}", item.name)
                };
                let key = item.key.as_deref().unwrap_or("");
                let gap = width.saturating_sub(name.chars().count() + key_width + 1);
                let line = format!("{name}{}{key:>key_width$} ", " ".repeat(gap));
                line.chars().take(width).collect()
            };
            queue!(out, Print(text), SetAttribute(Attribute::Reset))?;
        }
        Ok(())
    }
}

/// The width of the widest key shortcut in `items`, in characters.
fn key_width(items: &[MenuItem]) -> usize {
    items
        .iter()
        .filter_map(|item| item.key.as_deref())
        .map(|key| key.chars().count())
        .max()
        .unwrap_or(0)
}

/// The box a menu with `title` and `items` is drawn in at `x` and `y` on a `screen`.
fn menu_rect(title: &str, items: &[MenuItem], x: Offset, y: Offset, screen: (u16, u16)) -> Rect {
    let key_width = key_width(items);
    let name_width = items
        .iter()
        .map(|item| item.name.chars().count() + if item.disabled { 2 } else { 0 })
        .max()
        .unwrap_or(0);
    let content = (name_width + key_width + 4).max(title.chars().count() + 4);
    let width = (content + 2).min(usize::from(screen.0)) as u16;
    let height = ((items.len() + 2) as u16).min(screen.1);
    Rect::new(
        x.start(screen.0, width),
        y.start(screen.1, height),
        width,
        height,
    )
}
