use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, MutexGuard};

/// Distinct hyperlink targets kept for the whole process. A target past this limit is not stored:
/// its text is drawn without a link.
pub const MAX_LINKS: usize = 4096;

/// A hyperlink target (OSC 8), interned so a cell names it by a small id. Equal targets share an
/// id, so screens and the renderer compare links by value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LinkId(u32);

#[derive(Default)]
struct Table {
    ids: HashMap<String, u32>,
    bodies: Vec<String>,
}

fn table() -> MutexGuard<'static, Table> {
    static TABLE: LazyLock<Mutex<Table>> = LazyLock::new(|| Mutex::new(Table::default()));
    TABLE.lock().unwrap_or_else(|e| e.into_inner())
}

impl LinkId {
    /// The id of an OSC 8 target `body` (`params;uri`, as sent after `8;`). `None` once
    /// `MAX_LINKS` targets are stored.
    pub fn intern(body: &str) -> Option<Self> {
        let mut table = table();
        if let Some(&id) = table.ids.get(body) {
            return Some(Self(id));
        }
        if table.bodies.len() >= MAX_LINKS {
            return None;
        }
        let id = u32::try_from(table.bodies.len()).ok()?;
        table.bodies.push(body.to_owned());
        table.ids.insert(body.to_owned(), id);
        Some(Self(id))
    }

    /// The OSC 8 target body this id stands for.
    pub fn with_body<T>(self, f: impl FnOnce(&str) -> T) -> T {
        let table = table();
        f(&table.bodies[self.0 as usize])
    }
}
