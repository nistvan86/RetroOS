//! Allocation-free command endpoint for the kernel-owned early console.

use super::console_session::{
    ConsoleEndpoint, ConsoleSession, ConsoleSettings, EchoSink, InputDisposition, InputEvent,
    OutputAttachment,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EarlyConsoleAction {
    Continue,
    Reboot,
}

pub struct EarlyConsole {
    line: [u8; 64],
    len: usize,
    pending_action: Option<EarlyConsoleAction>,
}

impl EarlyConsole {
    pub const fn new() -> Self {
        Self { line: [0; 64], len: 0, pending_action: None }
    }

    pub fn prompt(&self, out: &mut dyn EchoSink) {
        write_bytes(out, b"early> ");
    }

    pub fn accept(&mut self, byte: u8, out: &mut dyn EchoSink) -> Option<EarlyConsoleAction> {
        match byte {
            b'\r' | b'\n' => {
                out.write_byte(b'\r');
                out.write_byte(b'\n');
                let action = self.command();
                if action.is_none() {
                    self.write_command_result(out);
                    self.prompt(out);
                }
                self.len = 0;
                action
            }
            0x08 | 0x7f if self.len != 0 => {
                self.len -= 1;
                write_bytes(out, b"\x08 \x08");
                None
            }
            0x20..=0x7e if self.len < self.line.len() => {
                self.line[self.len] = byte;
                self.len += 1;
                out.write_byte(byte);
                None
            }
            _ => None,
        }
    }

    fn command(&self) -> Option<EarlyConsoleAction> {
        let command = &self.line[..self.len];
        if command == b"resume" {
            return Some(EarlyConsoleAction::Continue);
        }
        if command == b"reboot" {
            return Some(EarlyConsoleAction::Reboot);
        }
        if command == b"help" {
            return None;
        }
        None
    }

    fn write_command_result(&self, out: &mut dyn EchoSink) {
        match &self.line[..self.len] {
            b"help" => write_bytes(out, b"commands: help info resume reboot\r\n"),
            b"info" => write_bytes(out, b"early console: paging active\r\n"),
            b"resume" | b"reboot" => {}
            _ => write_bytes(out, b"unknown command\r\n"),
        }
    }

    pub fn take_action(&mut self) -> Option<EarlyConsoleAction> {
        self.pending_action.take()
    }
}

impl Default for EarlyConsole {
    fn default() -> Self {
        Self::new()
    }
}

impl ConsoleEndpoint for EarlyConsole {
    fn input(&mut self, event: InputEvent, echo: &mut dyn EchoSink) -> InputDisposition {
        let InputEvent::Byte(byte) = event else {
            return InputDisposition::Ignored;
        };
        self.pending_action = self.accept(byte, echo);
        InputDisposition::Consumed
    }

}

fn write_bytes(out: &mut dyn EchoSink, bytes: &[u8]) {
    for &byte in bytes {
        out.write_byte(byte);
    }
}

struct VideoOutput<'a>(&'a mut lib::term::Term);

impl OutputAttachment for VideoOutput<'_> {
    fn write_byte(&mut self, byte: u8) {
        self.0.putchar(byte);
        crate::kernel::term::mark_dirty();
    }
}

struct SerialOutput;

impl OutputAttachment for SerialOutput {
    fn write_byte(&mut self, byte: u8) {
        crate::kernel::serial_log::write_byte(byte);
    }
}

/// Run the early command loop before ring 1 and normal startup.
pub fn run(screen: &mut lib::term::Term, reboot: fn() -> !) -> EarlyConsoleAction {
    let serial_attached = crate::kernel::serial_console::attach_early();
    let mut endpoint = EarlyConsole::new();
    let mut video = VideoOutput(screen);
    let mut serial = SerialOutput;
    let mut session = ConsoleSession::new(
        &mut endpoint,
        &mut video,
        serial_attached.then_some(&mut serial),
        ConsoleSettings::default(),
    );
    session.write_bytes(b"RetroOS early console\r\n");
    session.write_bytes(b"type help for commands\r\n");
    session.write_bytes(b"early> ");

    loop {
        if let Some(byte) = crate::kernel::serial_console::try_read_byte() {
            session.input(InputEvent::Byte(byte));
            if let Some(action) = session.endpoint_mut().take_action() {
                return match action {
                    EarlyConsoleAction::Continue => action,
                    EarlyConsoleAction::Reboot => reboot(),
                };
            }
        }
        core::hint::spin_loop();
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;
    use super::{EarlyConsole, EarlyConsoleAction};
    use crate::kernel::console_session::EchoSink;

    #[derive(Default)]
    struct Output(Vec<u8>);

    impl EchoSink for Output {
        fn write_byte(&mut self, byte: u8) {
            self.0.push(byte);
        }
    }

    fn send(console: &mut EarlyConsole, input: &[u8]) -> (Vec<u8>, Option<EarlyConsoleAction>) {
        let mut output = Output::default();
        let mut action = None;
        for &byte in input {
            action = console.accept(byte, &mut output);
        }
        (output.0, action)
    }

    #[test]
    fn echoes_line_and_help_response() {
        let mut console = EarlyConsole::new();
        let (output, _) = send(&mut console, b"help\r");
        assert_eq!(output, b"help\r\ncommands: help info resume reboot\r\nearly> ");
    }

    #[test]
    fn backspace_edits_the_line() {
        let mut console = EarlyConsole::new();
        let (output, _) = send(&mut console, b"helo\x08p\r");
        assert!(output.starts_with(b"helo\x08 \x08p\r\n"));
    }

    #[test]
    fn resume_is_a_command_action() {
        let mut console = EarlyConsole::new();
        let mut output = Output::default();
        let mut action = None;
        for &byte in b"resume\r" {
            action = console.accept(byte, &mut output);
        }
        assert_eq!(action, Some(EarlyConsoleAction::Continue));
        assert!(output.0.starts_with(b"resume\r\n"));
    }
}
