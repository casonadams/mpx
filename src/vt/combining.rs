use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::{LazyLock, Mutex, MutexGuard};

/// Distinct runs of combining characters kept for the whole process. A run past this limit is not
/// stored: the character it would join is drawn without it.
pub const MAX_COMBINING: usize = 4096;

/// Characters at most in one cell's run, so a flood of stacked marks cannot grow a cell unbounded.
pub const MAX_COMBINING_LEN: usize = 16;

/// VARIATION SELECTOR-16: asks for the emoji presentation of the character before it, which the
/// terminals that support it draw two columns wide (tmux `variation-selector-always-wide`).
pub const EMOJI_PRESENTATION: char = '\u{fe0f}';

/// Set in an id whose run holds `EMOJI_PRESENTATION`, so `widens` needs no table lookup.
const WIDENS: u32 = 1 << 31;

/// The zero-width characters drawn on a cell's character (combining accents, variation
/// selectors, joiners), interned so a cell names them by a small id. Equal runs share an id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Combining(NonZeroU32);

#[derive(Default)]
struct Table {
    ids: HashMap<Box<str>, NonZeroU32>,
    runs: Vec<Box<str>>,
}

fn table() -> MutexGuard<'static, Table> {
    static TABLE: LazyLock<Mutex<Table>> = LazyLock::new(|| Mutex::new(Table::default()));
    TABLE.lock().unwrap_or_else(|e| e.into_inner())
}

impl Combining {
    /// The run `before` (none when `None`) followed by `c`. `None` once the run would pass
    /// `MAX_COMBINING_LEN` characters or `MAX_COMBINING` runs are stored.
    pub fn append(before: Option<Self>, c: char) -> Option<Self> {
        thread_local! {
            static LAST: std::cell::Cell<Option<(Option<Combining>, char, Combining)>> =
                const { std::cell::Cell::new(None) };
        }
        if let Some((b, k, id)) = LAST.get()
            && (b, k) == (before, c)
        {
            return Some(id);
        }
        let id = Self::intern(before, c)?;
        LAST.set(Some((before, c, id)));
        Some(id)
    }

    fn intern(before: Option<Self>, c: char) -> Option<Self> {
        let mut table = table();
        let mut run = String::new();
        if let Some(before) = before {
            run.push_str(&table.runs[before.index()]);
        }
        run.push(c);
        if run.chars().count() > MAX_COMBINING_LEN {
            return None;
        }
        if let Some(&id) = table.ids.get(run.as_str()) {
            return Some(Self(id));
        }
        if table.runs.len() >= MAX_COMBINING {
            return None;
        }
        let index = u32::try_from(table.runs.len() + 1).ok()?;
        let flag = if run.contains(EMOJI_PRESENTATION) {
            WIDENS
        } else {
            0
        };
        let id = NonZeroU32::new(index | flag)?;
        let run: Box<str> = run.into();
        table.runs.push(run.clone());
        table.ids.insert(run, id);
        Some(Self(id))
    }

    /// The characters this id stands for.
    pub fn with_str<T>(self, f: impl FnOnce(&str) -> T) -> T {
        let table = table();
        f(&table.runs[self.index()])
    }

    /// Whether the run asks for emoji presentation, which makes its character two columns wide.
    pub fn widens(self) -> bool {
        self.0.get() & WIDENS != 0
    }

    fn index(self) -> usize {
        (self.0.get() & !WIDENS) as usize - 1
    }
}
