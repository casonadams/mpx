/// Holds input for a terminal until its first output has been handled, so the input follows that
/// output's replies. A ConPTY terminal opens with a cursor query (`ESC[6n`) and drops input
/// written before the query is answered (measured on the Windows CI runners).
#[derive(Debug, Default)]
pub struct InputGate {
    held: Option<Vec<u8>>,
}

impl InputGate {
    /// A gate that holds input until `open` when `hold` is set, and passes it straight on otherwise.
    pub fn new(hold: bool) -> Self {
        Self {
            held: hold.then(Vec::new),
        }
    }

    /// The gate for a pty on this platform: closed on Windows, where the pty is a ConPTY.
    pub fn for_pty() -> Self {
        Self::new(cfg!(windows))
    }

    /// `bytes` to write now, or `None` while the gate holds them.
    pub fn pass(&mut self, bytes: Vec<u8>) -> Option<Vec<u8>> {
        match &mut self.held {
            Some(held) => {
                held.extend_from_slice(&bytes);
                None
            }
            None => Some(bytes),
        }
    }

    /// Opens the gate, returning the input it held (if any) to write now.
    pub fn open(&mut self) -> Option<Vec<u8>> {
        self.held.take().filter(|held| !held.is_empty())
    }
}
