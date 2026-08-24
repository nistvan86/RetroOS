//! Polling driver for the kernel console during EarlyBoot and KernelReady.

use super::protocol::{ConsoleControl, ConsoleProtocolEvent};
use super::session::{ConsoleSession, ConsoleSettings, InputEvent, OutputAttachment};

pub use super::kernel::{EarlyConsole, EarlyConsoleAction, KernelConsole, KernelConsoleAction, KernelConsolePhase};

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
        crate::kernel::serial_log::write_session_byte(byte);
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
    let mut endpoint = KernelConsole::new(KernelConsolePhase::EarlyBoot);
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
        if let Some(event) = crate::kernel::serial_console::try_read_event() {
            let input = match event {
                ConsoleProtocolEvent::Control(ConsoleControl::Reboot) => {
                    crate::println!("serial control: reboot requested");
                    crate::kernel::drivers::hda::emergency_quiesce();
                    machine.reboot()
                }
                ConsoleProtocolEvent::Control(ConsoleControl::Panic) => {
                    panic!("serial control: panic requested");
                }
                ConsoleProtocolEvent::Input(InputEvent::Byte(byte)) => Some(InputEvent::Byte(byte)),
                ConsoleProtocolEvent::Input(InputEvent::Scancode(scancode)) => {
                    if crate::kernel::keyboard::update_key_state(scancode) {
                        let byte = crate::kernel::keyboard::scancode_to_ascii(scancode);
                        (byte != 0).then_some(InputEvent::Byte(byte))
                    } else {
                        None
                    }
                }
            };
            if let Some(input) = input {
                session.input(input);
                if let Some(action) = session.endpoint_mut().take_action() {
                    return match action {
                        EarlyConsoleAction::Boot => {
                            boot.clear_launch_cmdline();
                            action
                        }
                        EarlyConsoleAction::Exec => {
                            if let Some(command) = session.endpoint_mut().exec_command() {
                                boot.set_cmdline(command);
                                boot.set_post_exec(session.endpoint_mut().post_exec());
                                action
                            } else {
                                session.write_bytes(b"exec command was lost\r\n");
                                continue;
                            }
                        }
                        EarlyConsoleAction::Reboot => machine.reboot(),
                        EarlyConsoleAction::Panic => panic!("early console panic requested"),
                    };
                }
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
                        EarlyConsoleAction::Boot => {
                            boot.clear_launch_cmdline();
                            action
                        }
                        EarlyConsoleAction::Exec => {
                            if let Some(command) = session.endpoint_mut().exec_command() {
                                boot.set_cmdline(command);
                                boot.set_post_exec(session.endpoint_mut().post_exec());
                                action
                            } else {
                                session.write_bytes(b"exec command was lost\r\n");
                                continue;
                            }
                        }
                        EarlyConsoleAction::Reboot => machine.reboot(),
                        EarlyConsoleAction::Panic => panic!("early console panic requested"),
                    };
                }
            }
        }
        core::hint::spin_loop();
    }
}
