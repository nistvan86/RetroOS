//! Phase-independent ownership and routing for the single active console.

use super::kernel::{KernelConsole, KernelConsoleAction};
use super::protocol::{ConsoleControl as ProtocolControl, ConsoleProtocolEvent};
use super::serial;
use super::session::{ConsoleSession, ConsoleSettings, InputEvent, InputDisposition, OutputAttachment, OutputOrigin};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConsoleTarget {
    Detached,
    Kernel,
    Personality,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConsoleControl {
    Reboot,
    Panic,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoordinatorEvent {
    Control(ConsoleControl),
    Input(InputEvent),
}

pub struct ConsoleCoordinator {
    target: ConsoleTarget,
    serial_attached: bool,
    kernel_console: KernelConsole,
}

pub struct KernelConsoleInputContext<'a, V: OutputAttachment, S: OutputAttachment> {
    pub video: &'a mut V,
    pub serial: Option<&'a mut S>,
}

pub struct KernelConsoleDelivery {
    pub disposition: InputDisposition,
    pub action: Option<KernelConsoleAction>,
    exec: [u8; 256],
    exec_len: usize,
    pub post_exec: Option<arch_abi::PostExecAction>,
}

impl KernelConsoleDelivery {
    pub fn exec_path(&self) -> Option<&[u8]> {
        (self.exec_len != 0).then_some(&self.exec[..self.exec_len])
    }
}

impl ConsoleCoordinator {
    pub fn new() -> Self {
        Self {
            target: ConsoleTarget::Detached,
            serial_attached: false,
            kernel_console: KernelConsole::new(),
        }
    }

    pub fn target(&self) -> ConsoleTarget { self.target }

    pub fn attach_kernel(&mut self) -> bool {
        self.detach();
        self.kernel_console = KernelConsole::new();
        self.serial_attached = serial::attach_session();
        self.target = ConsoleTarget::Kernel;
        true
    }

    pub fn attach_personality(&mut self) -> bool {
        self.detach();
        self.serial_attached = serial::attach_session();
        self.target = ConsoleTarget::Personality;
        true
    }

    pub fn detach(&mut self) {
        if self.target != ConsoleTarget::Detached {
            serial::detach_to_logging();
        }
        self.serial_attached = false;
        self.target = ConsoleTarget::Detached;
    }

    pub fn serial_attached(&self) -> bool { self.serial_attached }

    pub fn kernel_console_mut(&mut self) -> Option<&mut KernelConsole> {
        matches!(self.target, ConsoleTarget::Kernel).then_some(&mut self.kernel_console)
    }

    /// Poll one serial event. Controls remain available in `PollingLog`, while
    /// ordinary input is admitted only when this coordinator owns a target.
    pub fn poll(&mut self) -> Option<CoordinatorEvent> {
        let event = serial::try_read_event()?;
        map_protocol_event(event, serial::ordinary_rx_allowed())
    }

    pub fn deliver_kernel<V: OutputAttachment, S: OutputAttachment>(
        &mut self,
        context: KernelConsoleInputContext<'_, V, S>,
        input: InputEvent,
    ) -> KernelConsoleDelivery {
        let mut result = KernelConsoleDelivery {
            disposition: InputDisposition::Ignored,
            action: None,
            exec: [0; 256],
            exec_len: 0,
            post_exec: None,
        };
        if !matches!(self.target, ConsoleTarget::Kernel) {
            return result;
        }
        let Some(endpoint) = self.kernel_console_mut() else {
            return result;
        };
        let mut session = ConsoleSession::new(
            endpoint,
            context.video,
            context.serial,
            ConsoleSettings::default(),
        );
        result.disposition = session.input(input);
        result.action = session.endpoint_mut().take_action();
        if result.action == Some(KernelConsoleAction::Exec) {
            if let Some(path) = session.endpoint_mut().exec_command() {
                result.exec_len = path.len();
                result.exec[..result.exec_len].copy_from_slice(path);
                result.post_exec = session.endpoint_mut().post_exec();
            }
        }
        result
    }

    /// Deliver one runtime personality event using operation-scoped adapter
    /// borrows. The coordinator owns routing policy, while kpipe and DOS BIOS
    /// state remain authoritative in their existing owners.
    pub fn deliver_personality<A: crate::Arch>(
        &mut self,
        machine: &mut A,
        regs: &mut crate::Regs,
        kt: &mut crate::kernel::thread::KernelThread<A>,
        personality: &mut crate::kernel::thread::Personality<A>,
        input: InputEvent,
    ) -> InputDisposition {
        if self.target != ConsoleTarget::Personality {
            return InputDisposition::Ignored;
        }
        match personality {
            crate::kernel::thread::Personality::Linux(_)
            | crate::kernel::thread::Personality::Os2(_)
            | crate::kernel::thread::Personality::Windows(_) => {
                let Some(mut stream) = super::stream::StreamConsoleAdapter::from_fds(&kt.fds)
                else {
                    return InputDisposition::Ignored;
                };
                match input {
                    InputEvent::Scancode(scancode) => stream.deliver_scancode(scancode),
                    InputEvent::Byte(byte) => stream.deliver_byte(byte),
                }
            }
            crate::kernel::thread::Personality::Dos(dos) => {
                if kt.state == crate::kernel::thread::ThreadState::Blocked {
                    return deliver_blocked_dos(input);
                }
                let dos_ptr = &mut **dos as *mut crate::kernel::thread::DosState<A>;
                let mut adapter = super::dos::DosConsoleAdapter::new(unsafe { &mut *dos_ptr });
                match input {
                    InputEvent::Scancode(scancode) => {
                        adapter.deliver_scancode(machine, regs, scancode)
                    }
                    InputEvent::Byte(byte) => {
                        let mut scancodes = [0; 4];
                        let count = super::dos::ascii_to_scancodes(byte, &mut scancodes);
                        let mut disposition = InputDisposition::Ignored;
                        for &scancode in &scancodes[..count] {
                            disposition = adapter.deliver_scancode(machine, regs, scancode);
                        }
                        disposition
                    }
                }
            }
        }
    }
}

/// Central interactive-output policy for call stacks that cannot borrow the
/// coordinator value. Ambient and emergency logging remain outside this path.
pub fn output_local(origin: OutputOrigin) -> bool {
    matches!(origin, OutputOrigin::StreamConsole)
}

pub fn output_serial(origin: OutputOrigin) -> bool {
    matches!(origin, OutputOrigin::StreamConsole | OutputOrigin::EndpointRendered)
}

pub fn write_output(origin: OutputOrigin, bytes: &[u8]) {
    if output_local(origin) {
        for &byte in bytes {
            crate::term::putchar(byte);
        }
        crate::kernel::term::mark_dirty();
    }
    if output_serial(origin) {
        for &byte in bytes {
            crate::kernel::console::serial_log::write_session_byte(byte);
        }
    }
}

fn deliver_blocked_dos(input: InputEvent) -> InputDisposition {
    let byte = match input {
        InputEvent::Scancode(scancode) => {
            if !crate::kernel::keyboard::update_key_state(scancode) {
                return InputDisposition::Ignored;
            }
            crate::kernel::keyboard::scancode_to_ascii(scancode)
        }
        InputEvent::Byte(byte) => byte,
    };
    if byte == 0 {
        return InputDisposition::Ignored;
    }
    crate::term::putchar(byte);
    crate::kernel::term::mark_dirty();
    let cpipe = crate::kernel::thread::console_pipe();
    (crate::kernel::kpipe::write(cpipe, &[byte]) == 1)
        .then_some(InputDisposition::Consumed)
        .unwrap_or(InputDisposition::Ignored)
}

fn map_protocol_event(
    event: ConsoleProtocolEvent,
    ordinary_input_allowed: bool,
) -> Option<CoordinatorEvent> {
    match event {
        ConsoleProtocolEvent::Control(ProtocolControl::Reboot) => {
            Some(CoordinatorEvent::Control(ConsoleControl::Reboot))
        }
        ConsoleProtocolEvent::Control(ProtocolControl::Panic) => {
            Some(CoordinatorEvent::Control(ConsoleControl::Panic))
        }
        ConsoleProtocolEvent::Input(input) if ordinary_input_allowed => {
            Some(CoordinatorEvent::Input(input))
        }
        ConsoleProtocolEvent::Input(_) => None,
    }
}

impl Drop for ConsoleCoordinator {
    fn drop(&mut self) { self.detach(); }
}

#[cfg(test)]
mod tests {
    use super::{
        map_protocol_event, ConsoleControl, ConsoleCoordinator, ConsoleTarget, CoordinatorEvent,
        KernelConsoleAction, KernelConsoleInputContext,
    };

    use crate::kernel::console::protocol::{ConsoleControl as ProtocolControl, ConsoleProtocolEvent};
    use crate::kernel::console::session::{
        InputDisposition, InputEvent, OutputAttachment, OutputOrigin,
    };

    #[derive(Default)]
    struct Sink(alloc::vec::Vec<u8>);

    impl OutputAttachment for Sink {
        fn write_byte(&mut self, byte: u8) { self.0.push(byte); }
    }

    #[test]
    fn protocol_controls_are_admitted_without_an_endpoint() {
        assert_eq!(
            map_protocol_event(
                ConsoleProtocolEvent::Control(ProtocolControl::Reboot),
                false,
            ),
            Some(CoordinatorEvent::Control(ConsoleControl::Reboot)),
        );
        assert_eq!(
            map_protocol_event(
                ConsoleProtocolEvent::Input(InputEvent::Byte(b'x')),
                false,
            ),
            None,
        );
        assert_eq!(
            map_protocol_event(
                ConsoleProtocolEvent::Input(InputEvent::Byte(b'x')),
                true,
            ),
            Some(CoordinatorEvent::Input(InputEvent::Byte(b'x'))),
        );
    }

    #[test]
    fn output_origins_have_no_duplicate_local_route() {
        assert!(super::output_local(OutputOrigin::StreamConsole));
        assert!(super::output_serial(OutputOrigin::StreamConsole));
        assert!(!super::output_local(OutputOrigin::EndpointRendered));
        assert!(super::output_serial(OutputOrigin::EndpointRendered));
    }

    #[test]
    fn kernel_delivery_fans_out_echo() {
            let mut coordinator = ConsoleCoordinator::new();
            coordinator.attach_kernel();
            let mut video = Sink::default();
            let mut serial = Sink::default();
            let mut last = None;
            for &byte in b"help\r" {
                last = Some(coordinator.deliver_kernel(
                    KernelConsoleInputContext {
                        video: &mut video,
                        serial: Some(&mut serial),
                    },
                    InputEvent::Byte(byte),
                ));
            }
            assert_eq!(last.unwrap().disposition, InputDisposition::Consumed);
            assert_eq!(video.0, serial.0);
            assert!(video.0.windows(b"commands: help".len())
                .any(|window| window == b"commands: help"));
    }

    #[test]
    fn kernel_delivery_accepts_boot() {
        let mut early = ConsoleCoordinator::new();
        early.attach_kernel();
        let mut video = Sink::default();
        let mut last = None;
        for &byte in b"boot\r" {
            last = Some(early.deliver_kernel(
                KernelConsoleInputContext { video: &mut video, serial: None::<&mut Sink> },
                InputEvent::Byte(byte),
            ));
        }
        assert_eq!(last.unwrap().action, Some(KernelConsoleAction::Boot));

    }

    #[test]
    fn target_transitions_are_endpoint_agnostic() {
        let mut coordinator = ConsoleCoordinator::new();
        assert_eq!(coordinator.target(), ConsoleTarget::Detached);
        coordinator.attach_kernel();
        assert_eq!(coordinator.target(), ConsoleTarget::Kernel);
        coordinator.detach();
        assert_eq!(coordinator.target(), ConsoleTarget::Detached);
        coordinator.attach_personality();
        assert_eq!(coordinator.target(), ConsoleTarget::Personality);
        coordinator.detach();
        assert_eq!(coordinator.target(), ConsoleTarget::Detached);
    }
}
