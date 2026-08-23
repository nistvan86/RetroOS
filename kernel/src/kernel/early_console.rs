//! Allocation-free command endpoint for the kernel-owned early console.

use super::console_session::{
    ConsoleEndpoint, ConsoleSession, ConsoleSettings, EchoSink, InputDisposition, InputEvent,
    OutputAttachment,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EarlyConsoleAction {
    Continue,
    Exec,
    Reboot,
}

pub struct EarlyConsole {
    line: [u8; 256],
    len: usize,
    overflowed: bool,
    exec: [u8; 256],
    exec_len: usize,
    pending_action: Option<EarlyConsoleAction>,
}

impl EarlyConsole {
    pub const fn new() -> Self {
        Self {
            line: [0; 256],
            len: 0,
            overflowed: false,
            exec: [0; 256],
            exec_len: 0,
            pending_action: None,
        }
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
                self.overflowed = false;
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
            0x20..=0x7e => {
                self.overflowed = true;
                None
            }
            _ => None,
        }
    }

    fn command(&mut self) -> Option<EarlyConsoleAction> {
        if self.overflowed {
            return None;
        }
        let command = &self.line[..self.len];
        if command == b"resume" {
            return Some(EarlyConsoleAction::Continue);
        }
        if command == b"reboot" {
            return Some(EarlyConsoleAction::Reboot);
        }
        if command.len() > 5 && &command[..5] == b"exec " {
            let path = &command[5..];
            if !path.is_empty() && path.len() <= self.exec.len() {
                self.exec[..path.len()].copy_from_slice(path);
                self.exec_len = path.len();
                return Some(EarlyConsoleAction::Exec);
            }
        }
        None
    }

    fn write_command_result(&self, out: &mut dyn EchoSink) {
        if self.overflowed {
            write_bytes(out, b"command too long\r\n");
            return;
        }
        match &self.line[..self.len] {
            b"help" => write_bytes(out, b"commands: help info resume reboot exec <path> [args]\r\n"),
            b"info" => write_bytes(out, b"early console: paging active\r\n"),
            b"resume" | b"reboot" => {}
            command if command.starts_with(b"exec ") => {
                write_bytes(out, b"exec requires a non-empty path\r\n");
            }
            _ => write_bytes(out, b"unknown command\r\n"),
        }
    }

    pub fn exec_command(&self) -> Option<&[u8]> {
        (self.exec_len != 0).then_some(&self.exec[..self.exec_len])
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

struct VideoOutput<'a> {
    screen: &'a mut lib::term::Term,
    sync_cursor: fn(usize, usize),
}

impl OutputAttachment for VideoOutput<'_> {
    fn write_byte(&mut self, byte: u8) {
        self.screen.putchar(byte);
        crate::kernel::term::mark_dirty();
        let (column, row) = self.screen.cursor_pos();
        (self.sync_cursor)(column, row);
    }
}

struct SerialOutput;

impl OutputAttachment for SerialOutput {
    fn write_byte(&mut self, byte: u8) {
        crate::kernel::serial_log::write_byte(byte);
    }
}

/// Run the early command loop before normal personality startup.
///
/// Input is sourced from the backend's existing local-input path and from the
/// optional serial console. Both are immediately translated into the same
/// endpoint; no early-console queue is introduced.
pub fn run<A: crate::Arch>(
    machine: &mut A,
    screen: &mut lib::term::Term,
    boot: &mut crate::BootConfig,
    poll_input: fn() -> Option<crate::Irq>,
    sync_cursor: fn(usize, usize),
) -> EarlyConsoleAction {
    let serial_attached = crate::kernel::serial_console::attach_early();
    let mut endpoint = EarlyConsole::new();
    let mut video = VideoOutput { screen, sync_cursor };
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
                    EarlyConsoleAction::Continue => {
                        boot.clear_launch_cmdline();
                        action
                    }
                    EarlyConsoleAction::Exec => {
                        if let Some(command) = session.endpoint_mut().exec_command() {
                            boot.set_cmdline(command);
                            action
                        } else {
                            session.write_bytes(b"exec command was lost\r\n");
                            continue;
                        }
                    }
                    EarlyConsoleAction::Reboot => machine.reboot(),
                };
            }
        }
        if let Some(crate::Irq::Key(scancode)) = poll_input()
            && crate::kernel::keyboard::update_key_state(scancode)
        {
            let byte = crate::kernel::keyboard::scancode_to_ascii(scancode);
            if byte != 0 {
                session.input(InputEvent::Byte(byte));
                if let Some(action) = session.endpoint_mut().take_action() {
                    return match action {
                        EarlyConsoleAction::Continue => {
                            boot.clear_launch_cmdline();
                            action
                        }
                        EarlyConsoleAction::Exec => {
                            if let Some(command) = session.endpoint_mut().exec_command() {
                                boot.set_cmdline(command);
                                action
                            } else {
                                session.write_bytes(b"exec command was lost\r\n");
                                continue;
                            }
                        }
                        EarlyConsoleAction::Reboot => machine.reboot(),
                    };
                }
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
        assert_eq!(output, b"help\r\ncommands: help info resume reboot exec <path> [args]\r\nearly> ");
    }

    #[test]
    fn backspace_edits_the_line() {
        let mut console = EarlyConsole::new();
        let (output, _) = send(&mut console, b"helo\x08p\r");
        assert!(output.starts_with(b"helo\x08 \x08p\r\n"));
    }

    #[test]
    fn exec_preserves_path_and_arguments() {
        let mut console = EarlyConsole::new();
        let (_, action) = send(&mut console, b"exec TESTS/X.COM arg\r");
        assert_eq!(action, Some(EarlyConsoleAction::Exec));
        assert_eq!(console.exec_command(), Some(&b"TESTS/X.COM arg"[..]));
    }

    #[test]
    fn exec_without_path_is_rejected() {
        let mut console = EarlyConsole::new();
        let (output, action) = send(&mut console, b"exec \r");
        assert_eq!(action, None);
        assert!(output.windows(b"exec requires a non-empty path".len())
            .any(|window| window == b"exec requires a non-empty path"));
    }

    #[test]
    fn overlong_command_is_rejected() {
        let mut console = EarlyConsole::new();
        let mut input = [b'a'; 258];
        input[257] = b'\r';
        let (output, action) = send(&mut console, &input);
        assert_eq!(action, None);
        assert!(output.windows(b"command too long".len())
            .any(|window| window == b"command too long"));
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
