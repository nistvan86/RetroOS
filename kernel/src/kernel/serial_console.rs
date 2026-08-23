//! Serial-console ownership above the polling UART and ambient log sink.
//!
//! Stage 4 only adds the early-session RX handoff. TX remains on the existing
//! serial_log path so there is one compatible serial egress until runtime
//! session output ownership is implemented.

use core::sync::atomic::{AtomicU8, Ordering};

use arch_abi::ComPort;
use crate::kernel::drivers::uart16550::Uart16550;

const DISABLED: u8 = 0;
const POLLING_LOG: u8 = 1;
const EARLY_SESSION: u8 = 2;
const FAILED: u8 = 4;

static STATE: AtomicU8 = AtomicU8::new(DISABLED);
static PORT: AtomicU8 = AtomicU8::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SerialConsoleState {
    Disabled,
    PollingLog,
    EarlySession,
    PersonalitySession,
    Failed,
}

fn encode(port: ComPort) -> u8 {
    match port {
        ComPort::Com1 => 1,
        ComPort::Com2 => 2,
    }
}

fn decode(port: u8) -> Option<ComPort> {
    match port {
        1 => Some(ComPort::Com1),
        2 => Some(ComPort::Com2),
        _ => None,
    }
}

/// Initialize the existing ambient-log UART owner and enter fallback logging.
pub fn init_log(port: ComPort) -> bool {
    if !crate::kernel::serial_log::init(port) {
        STATE.store(FAILED, Ordering::Release);
        return false;
    }
    PORT.store(encode(port), Ordering::Release);
    STATE.store(POLLING_LOG, Ordering::Release);
    true
}

/// Attach the configured serial input to the early console.
pub fn attach_early() -> bool {
    if PORT.load(Ordering::Acquire) == 0 {
        return false;
    }
    STATE.compare_exchange(
        POLLING_LOG,
        EARLY_SESSION,
        Ordering::AcqRel,
        Ordering::Acquire,
    ).is_ok()
}

/// Return to ambient kernel logging while no interactive session owns serial.
pub fn detach_to_logging() {
    if STATE.load(Ordering::Acquire) == EARLY_SESSION {
        STATE.store(POLLING_LOG, Ordering::Release);
    }
}

/// Poll one byte only while the early session owns serial input.
pub fn try_read_byte() -> Option<u8> {
    if STATE.load(Ordering::Acquire) != EARLY_SESSION {
        return None;
    }
    let port = decode(PORT.load(Ordering::Acquire))?;
    Uart16550::new(port).try_read_byte()
}

pub fn state() -> SerialConsoleState {
    match STATE.load(Ordering::Acquire) {
        POLLING_LOG => SerialConsoleState::PollingLog,
        EARLY_SESSION => SerialConsoleState::EarlySession,
        FAILED => SerialConsoleState::Failed,
        _ => SerialConsoleState::Disabled,
    }
}

#[cfg(test)]
mod tests {
    use super::{SerialConsoleState, STATE, DISABLED};

    #[test]
    fn starts_disabled() {
        STATE.store(DISABLED, core::sync::atomic::Ordering::Release);
        assert_eq!(super::state(), SerialConsoleState::Disabled);
    }
}
