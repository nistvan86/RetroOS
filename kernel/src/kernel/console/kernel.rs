//! Reusable allocation-free kernel command console endpoint.

use super::session::{ConsoleEndpoint, EchoSink, InputDisposition, InputEvent};


#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KernelConsoleAction {
    Boot,
    Exec,
    Reboot,
    Panic,
}

pub struct KernelConsole {

    line: [u8; 256],
    len: usize,
    overflowed: bool,
    exec: [u8; 256],
    exec_len: usize,
    pending_action: Option<KernelConsoleAction>,
    post_exec: Option<arch_abi::PostExecAction>,
}

impl KernelConsole {
    pub const fn new() -> Self {
        Self {
            line: [0; 256],
            len: 0,
            overflowed: false,
            exec: [0; 256],
            exec_len: 0,
            pending_action: None,
            post_exec: Some(arch_abi::PostExecAction::ReturnToKernelConsole),
        }
    }


    pub fn prompt(&self, out: &mut dyn EchoSink) {
        write_bytes(out, b"bootmon> ");
    }

    pub fn accept(&mut self, byte: u8, out: &mut dyn EchoSink) -> Option<KernelConsoleAction> {
        match byte {
            b'\r' | b'\n' => {
                out.write_byte(b'\r');
                out.write_byte(b'\n');
                let action = self.command(out);
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

    fn command(&mut self, _out: &mut dyn EchoSink) -> Option<KernelConsoleAction> {
        if self.overflowed {
            return None;
        }
        let command = &self.line[..self.len];
        if command == b"boot" {
            return Some(KernelConsoleAction::Boot);
        }
        if command == b"reboot" {
            return Some(KernelConsoleAction::Reboot);
        }
        if command == b"panic" {
            return Some(KernelConsoleAction::Panic);
        }
        if command.starts_with(b"exec ") {
            let mut path = &command[5..];
            let mut post_exec = arch_abi::PostExecAction::ReturnToKernelConsole;
            for (option, action) in [
                (b"--and-halt " as &[u8], arch_abi::PostExecAction::Shutdown),
                (b"--and-reboot " as &[u8], arch_abi::PostExecAction::Reboot),
            ] {
                if path.starts_with(option) {
                    path = &path[option.len()..];
                    post_exec = action;
                    break;
                }
            }
            if !path.is_empty() && path.len() <= self.exec.len() {
                self.exec[..path.len()].copy_from_slice(path);
                self.exec_len = path.len();
                self.post_exec = Some(post_exec);
                return Some(KernelConsoleAction::Exec);
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
            b"help" => write_bytes(out, b"commands: help info boot reboot panic exec [--and-halt|--and-reboot] <path> [args]\r\n"),
            b"info" => write_bytes(out, b"boot monitor: paging active\r\n"),
            b"boot" | b"reboot" | b"panic" => {}
            command if command.starts_with(b"exec ") => {
                write_bytes(out, b"exec requires a non-empty path\r\n");
            }
            _ => write_bytes(out, b"unknown command\r\n"),
        }
    }

    pub fn exec_command(&self) -> Option<&[u8]> {
        (self.exec_len != 0).then_some(&self.exec[..self.exec_len])
    }

    pub fn post_exec(&self) -> Option<arch_abi::PostExecAction> { self.post_exec }

    pub fn take_action(&mut self) -> Option<KernelConsoleAction> {
        self.pending_action.take()
    }
}

impl Default for KernelConsole {
    fn default() -> Self { Self::new() }
}

impl ConsoleEndpoint for KernelConsole {
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

// Temporary source compatibility while callers migrate from the old names.
pub type EarlyConsole = KernelConsole;
pub type EarlyConsoleAction = KernelConsoleAction;

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;
    use super::{KernelConsole, KernelConsoleAction};
    use crate::kernel::console::session::EchoSink;

    #[derive(Default)]
    struct Output(Vec<u8>);

    impl EchoSink for Output {
        fn write_byte(&mut self, byte: u8) { self.0.push(byte); }
    }

    fn send(console: &mut KernelConsole, input: &[u8]) -> (Vec<u8>, Option<KernelConsoleAction>) {
        let mut output = Output::default();
        let mut action = None;
        for &byte in input {
            action = console.accept(byte, &mut output);
        }
        (output.0, action)
    }

    #[test]
    fn early_boot_boot_is_a_command_action() {
        let mut console = KernelConsole::new();
        let (output, action) = send(&mut console, b"boot\r");
        assert_eq!(action, Some(KernelConsoleAction::Boot));
        assert!(output.starts_with(b"boot\r\n"));
    }

    #[test]
    fn echoes_line_and_help_response() {
        let mut console = KernelConsole::new();
        let (output, _) = send(&mut console, b"help\r");
        assert_eq!(output, b"help\r\ncommands: help info boot reboot panic exec [--and-halt|--and-reboot] <path> [args]\r\nbootmon> ");
    }

    #[test]
    fn backspace_edits_the_line() {
        let mut console = KernelConsole::new();
        let (output, _) = send(&mut console, b"helo\x08p\r");
        assert!(output.starts_with(b"helo\x08 \x08p\r\n"));
    }

    #[test]
    fn exec_preserves_path_and_arguments() {
        let mut console = KernelConsole::new();
        let (_, action) = send(&mut console, b"exec TESTS/X.COM arg\r");
        assert_eq!(action, Some(KernelConsoleAction::Exec));
        assert_eq!(console.exec_command(), Some(&b"TESTS/X.COM arg"[..]));
    }

    #[test]
    fn exec_post_action_options_are_explicit() {
        let mut console = KernelConsole::new();
        let (_, action) = send(&mut console, b"exec /host/STUB.COM arg\r");
        assert_eq!(action, Some(KernelConsoleAction::Exec));
        assert_eq!(console.exec_command(), Some(&b"/host/STUB.COM arg"[..]));
        assert_eq!(
            console.post_exec(),
            Some(arch_abi::PostExecAction::ReturnToKernelConsole)
        );

        let (_, action) = send(&mut console, b"exec --and-halt /host/STUB.COM\r");
        assert_eq!(action, Some(KernelConsoleAction::Exec));
        assert_eq!(console.post_exec(), Some(arch_abi::PostExecAction::Shutdown));

        let (_, action) = send(&mut console, b"exec --and-reboot /host/STUB.COM\r");
        assert_eq!(action, Some(KernelConsoleAction::Exec));
        assert_eq!(console.post_exec(), Some(arch_abi::PostExecAction::Reboot));
    }

    #[test]
    fn exec_without_path_is_rejected() {
        let mut console = KernelConsole::new();
        let (output, action) = send(&mut console, b"exec \r");
        assert_eq!(action, None);
        assert!(output.windows(b"exec requires a non-empty path".len())
            .any(|window| window == b"exec requires a non-empty path"));
    }

    #[test]
    fn overlong_command_is_rejected() {
        let mut console = KernelConsole::new();
        let mut input = [b'a'; 258];
        input[257] = b'\r';
        let (output, action) = send(&mut console, &input);
        assert_eq!(action, None);
        assert!(output.windows(b"command too long".len())
            .any(|window| window == b"command too long"));
    }

    #[test]
    fn info_reports_the_boot_monitor() {
        let mut console = KernelConsole::new();
        let (output, _) = send(&mut console, b"info\r");
        assert!(output.windows(b"boot monitor: paging active".len())
            .any(|window| window == b"boot monitor: paging active"));
    }
}
