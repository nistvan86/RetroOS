//! Serial-console ownership above the polling UART and ambient log sink.
//!
//! Stage 4 only adds the early-session RX handoff. TX remains on the existing
//! serial_log path so there is one compatible serial egress until runtime
//! session output ownership is implemented.

use core::sync::atomic::{AtomicU8, Ordering};

use arch_abi::ComPort;
use crate::kernel::console_protocol::{ConsoleProtocolDecoder, ConsoleProtocolEvent};
use crate::kernel::drivers::uart16550::Uart16550;

const DISABLED: u8 = 0;
const POLLING_LOG: u8 = 1;
const EARLY_SESSION: u8 = 2;
const PERSONALITY_SESSION: u8 = 3;
const FAILED: u8 = 4;

static STATE: AtomicU8 = AtomicU8::new(DISABLED);
static PORT: AtomicU8 = AtomicU8::new(0);
static mut DECODER: ConsoleProtocolDecoder = ConsoleProtocolDecoder::new();

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
    unsafe { DECODER = ConsoleProtocolDecoder::new(); }
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

/// Attach the configured serial input/output to a personality session.
///
/// A session must first detach to `PollingLog`; direct session-to-session
/// replacement is intentionally rejected.
pub fn attach_personality() -> bool {
    if PORT.load(Ordering::Acquire) == 0 {
        return false;
    }
    STATE.compare_exchange(
        POLLING_LOG,
        PERSONALITY_SESSION,
        Ordering::AcqRel,
        Ordering::Acquire,
    ).is_ok()
}

/// Return to ambient kernel logging while no interactive session owns serial.
pub fn detach_to_logging() {
    let state = STATE.load(Ordering::Acquire);
    if state == EARLY_SESSION || state == PERSONALITY_SESSION {
        STATE.store(POLLING_LOG, Ordering::Release);
    }
}

/// Poll one decoded console event from the configured serial console.
///
/// Protocol controls are recognized in every live state, including
/// `PollingLog`, so reboot does not depend on an attached personality.
pub fn try_read_event(now_ns: u64) -> Option<ConsoleProtocolEvent> {
    let state = STATE.load(Ordering::Acquire);
    if state == DISABLED || state == FAILED {
        return None;
    }
    let port = decode(PORT.load(Ordering::Acquire))?;
    let decoder = &raw mut DECODER;
    unsafe { (*decoder).expire(now_ns); }
    for _ in 0..64 {
        let byte = Uart16550::new(port).try_read_byte()?;
        let event = unsafe { (*decoder).feed_at(byte, now_ns) };
        if event.is_some() {
            return event;
        }
    }
    None
}

/// Compatibility helper for the early-console byte path.
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
        PERSONALITY_SESSION => SerialConsoleState::PersonalitySession,
        FAILED => SerialConsoleState::Failed,
        _ => SerialConsoleState::Disabled,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        attach_early, attach_personality, detach_to_logging, SerialConsoleState, STATE, DISABLED,
        POLLING_LOG,
    };
    use core::sync::atomic::Ordering;

    #[test]
    fn starts_disabled() {
        STATE.store(DISABLED, Ordering::Release);
        assert_eq!(super::state(), SerialConsoleState::Disabled);
    }

    #[test]
    fn session_handoffs_return_to_logging_before_replacement() {
        super::PORT.store(1, Ordering::Release);
        STATE.store(POLLING_LOG, Ordering::Release);
        assert!(attach_early());
        assert_eq!(super::state(), SerialConsoleState::EarlySession);
        assert!(!attach_personality());
        detach_to_logging();
        assert_eq!(super::state(), SerialConsoleState::PollingLog);
        assert!(attach_personality());
        assert_eq!(super::state(), SerialConsoleState::PersonalitySession);
        detach_to_logging();
        assert_eq!(super::state(), SerialConsoleState::PollingLog);
    }
}
