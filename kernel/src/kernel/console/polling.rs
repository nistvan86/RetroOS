//! Polling driver for the kernel console during EarlyBoot and KernelReady.

use super::coordinator::{ConsoleControl, ConsoleCoordinator, CoordinatorEvent, KernelConsoleInputContext};
use super::kernel::{KernelConsoleAction, KernelConsolePhase};
use super::session::{ConsoleSession, ConsoleSettings, InputEvent, OutputAttachment};

// Temporary source compatibility while callers migrate from the old module.
pub use super::kernel::{EarlyConsole, EarlyConsoleAction, KernelConsole};

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
        super::coordinator::write_output(
            super::session::OutputOrigin::EndpointRendered,
            &[byte],
        );
    }
}

/// Run the polling kernel-console driver for one boot phase.
///
/// Input is sourced from the backend's existing local-input path and from the
/// optional serial console. Both are immediately delivered to the coordinator's
/// KernelConsole endpoint; no console queue is introduced.
pub fn run<A: crate::Arch>(
    machine: &mut A,
    screen: &mut lib::term::Term,
    boot: &mut crate::BootConfig,
    coordinator: &mut ConsoleCoordinator,
    phase: KernelConsolePhase,
    poll_input: fn() -> Option<crate::Irq>,
    sync_cursor: fn(usize, usize),
) -> EarlyConsoleAction {
    coordinator.attach_kernel(phase);
    let serial_attached = coordinator.serial_attached();
    let mut video = VideoOutput { screen, sync_cursor };
    let mut serial = SerialOutput;
    {
        let endpoint = coordinator.kernel_console_mut().expect("kernel console attached");
        let mut session = ConsoleSession::new(
            endpoint,
            &mut video,
            serial_attached.then_some(&mut serial),
            ConsoleSettings::default(),
        );
        session.write_bytes(match phase {
            KernelConsolePhase::EarlyBoot => b"RetroOS early console\r\n",
            KernelConsolePhase::KernelReady => b"RetroOS kernel console\r\n",
        });
        session.write_bytes(b"type help for commands\r\n");
        session.write_bytes(match phase {
            KernelConsolePhase::EarlyBoot => b"early> ",
            KernelConsolePhase::KernelReady => b"kernel> ",
        });
    }

    loop {
        let serial_input = match coordinator.poll() {
            Some(CoordinatorEvent::Control(ConsoleControl::Reboot)) => {
                crate::println!("serial control: reboot requested");
                crate::kernel::drivers::hda::emergency_quiesce();
                machine.reboot()
            }
            Some(CoordinatorEvent::Control(ConsoleControl::Panic)) => {
                panic!("serial control: panic requested");
            }
            Some(CoordinatorEvent::Input(InputEvent::Byte(byte))) => Some(InputEvent::Byte(byte)),
            Some(CoordinatorEvent::Input(InputEvent::Scancode(scancode))) => {
                if crate::kernel::keyboard::update_key_state(scancode) {
                    let byte = crate::kernel::keyboard::scancode_to_ascii(scancode);
                    (byte != 0).then_some(InputEvent::Byte(byte))
                } else {
                    None
                }
            }
            None => None,
        };

        let local_input = match poll_input() {
            Some(crate::Irq::Key(scancode))
                if crate::kernel::keyboard::update_key_state(scancode) => {
                let byte = crate::kernel::keyboard::scancode_to_ascii(scancode);
                (byte != 0).then_some(InputEvent::Byte(byte))
            }
            _ => None,
        };

        for input in [serial_input, local_input].into_iter().flatten() {
            let mut command = [0; 256];
            let mut command_len = 0;
            let mut post_exec = None;
            let action = {
                let delivery = coordinator.deliver_kernel(
                    KernelConsoleInputContext {
                        video: &mut video,
                        serial: serial_attached.then_some(&mut serial),
                    },
                    input,
                );
                let action = delivery.action;
                if action == Some(KernelConsoleAction::Exec) {
                    if let Some(path) = delivery.exec_path() {
                        command_len = path.len();
                        command[..command_len].copy_from_slice(path);
                        post_exec = delivery.post_exec;
                    }
                }
                action
            };

            let Some(action) = action else { continue };
            let result = match action {
                KernelConsoleAction::Boot => {
                    boot.clear_launch_cmdline();
                    EarlyConsoleAction::Boot
                }
                KernelConsoleAction::Exec if command_len != 0 => {
                    boot.set_cmdline(&command[..command_len]);
                    boot.set_post_exec(post_exec);
                    EarlyConsoleAction::Exec
                }
                KernelConsoleAction::Exec => continue,
                KernelConsoleAction::Reboot => machine.reboot(),
                KernelConsoleAction::Panic => panic!("kernel console panic requested"),
            };
            coordinator.detach();
            return result;
        }
        core::hint::spin_loop();
    }
}
