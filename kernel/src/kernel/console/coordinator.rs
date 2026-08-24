//! Phase-independent ownership and routing for the single active console.

use super::kernel::{KernelConsole, KernelConsolePhase};
use super::protocol::{ConsoleControl as ProtocolControl, ConsoleProtocolEvent};
use super::serial;
use super::session::{InputEvent, InputDisposition};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConsoleTarget {
    Detached,
    Kernel(KernelConsolePhase),
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

impl ConsoleCoordinator {
    pub fn new() -> Self {
        Self {
            target: ConsoleTarget::Detached,
            serial_attached: false,
            kernel_console: KernelConsole::new(KernelConsolePhase::EarlyBoot),
        }
    }

    pub fn target(&self) -> ConsoleTarget { self.target }

    pub fn attach_kernel(&mut self, phase: KernelConsolePhase) -> bool {
        self.detach();
        self.kernel_console = KernelConsole::new(phase);
        self.serial_attached = serial::attach_session();
        self.target = ConsoleTarget::Kernel(phase);
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
        matches!(self.target, ConsoleTarget::Kernel(_)).then_some(&mut self.kernel_console)
    }

    /// Poll one serial event. Controls remain available in `PollingLog`, while
    /// ordinary input is admitted only when this coordinator owns a target.
    pub fn poll(&mut self) -> Option<CoordinatorEvent> {
        let event = serial::try_read_event()?;
        map_protocol_event(event, serial::ordinary_rx_allowed())
    }

    pub fn deliver_kernel(&mut self, _input: InputEvent) -> InputDisposition {
        if matches!(self.target, ConsoleTarget::Kernel(_)) {
            InputDisposition::Consumed
        } else {
            InputDisposition::Ignored
        }
    }
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
    use super::{map_protocol_event, ConsoleControl, ConsoleCoordinator, ConsoleTarget, CoordinatorEvent};
    use crate::kernel::console::kernel::KernelConsolePhase;
    use crate::kernel::console::protocol::{ConsoleControl as ProtocolControl, ConsoleProtocolEvent};
    use crate::kernel::console::session::InputEvent;

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
    fn target_transitions_are_endpoint_agnostic() {
        let mut coordinator = ConsoleCoordinator::new();
        assert_eq!(coordinator.target(), ConsoleTarget::Detached);
        coordinator.attach_kernel(KernelConsolePhase::EarlyBoot);
        assert_eq!(coordinator.target(), ConsoleTarget::Kernel(KernelConsolePhase::EarlyBoot));
        coordinator.detach();
        assert_eq!(coordinator.target(), ConsoleTarget::Detached);
        coordinator.attach_personality();
        assert_eq!(coordinator.target(), ConsoleTarget::Personality);
        coordinator.detach();
        assert_eq!(coordinator.target(), ConsoleTarget::Detached);
    }
}
