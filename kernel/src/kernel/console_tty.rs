//! Compatibility adapter for the existing pipe-backed, text-terminal paths.
//!
//! This is intentionally not a new line discipline. It keeps `kpipe` as the
//! canonical input buffer and `kernel::term` as the canonical output state while
//! giving transports and personalities one shared call surface.

use crate::kernel::{keyboard, kpipe, term};
use crate::kernel::thread::{FdKind, MAX_FDS};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TtyConsoleAdapter {
    input_pipe: u8,
}

impl TtyConsoleAdapter {
    pub fn new(input_pipe: u8) -> Self {
        Self { input_pipe }
    }

    pub fn from_fds(fds: &[FdKind; MAX_FDS]) -> Option<Self> {
        match fds[0] {
            FdKind::PipeRead(pipe) => Some(Self::new(pipe)),
            _ => None,
        }
    }

    /// Convert a local keyboard scancode using the existing shared keyboard
    /// state and deliver the resulting byte directly to the existing kpipe.
    pub fn deliver_scancode(&mut self, scancode: u8) {
        if !keyboard::update_key_state(scancode) {
            return;
        }
        let byte = keyboard::scancode_to_ascii(scancode);
        if byte != 0 {
            self.deliver_byte(byte);
        }
    }

    /// Deliver already-translated transport input without adding a queue.
    pub fn deliver_byte(&mut self, byte: u8) {
        let _ = kpipe::write(self.input_pipe, &[byte]);
    }
}

/// Preserve the existing ConsoleOut behavior while giving all TTY-like
/// personalities one output adapter call site.
pub fn write_console_bytes(bytes: &[u8]) {
    for &byte in bytes {
        term::putchar(byte);
    }
    term::mark_dirty();
}

#[cfg(test)]
mod tests {
    use super::TtyConsoleAdapter;

    #[test]
    fn constructs_for_a_pipe_endpoint() {
        let adapter = TtyConsoleAdapter::new(7);
        assert_eq!(adapter, TtyConsoleAdapter::new(7));
    }
}
