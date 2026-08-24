//! Serial-console ownership above the polling UART and ambient log sink.
//!
//! This module owns the coupled serial attachment valve and protocol RX state;
//! endpoint selection is coordinated by the parent console coordinator.

use core::sync::atomic::{AtomicU8, AtomicU32, Ordering};

use arch_abi::ComPort;
use super::protocol::{ConsoleProtocolDecoder, ConsoleProtocolEvent};
use crate::kernel::drivers::uart16550::Uart16550;

const DISABLED: u8 = 0;
const POLLING_LOG: u8 = 1;
const ATTACHED_SESSION: u8 = 2;
const FAILED: u8 = 3;

static STATE: AtomicU8 = AtomicU8::new(DISABLED);
static PORT: AtomicU8 = AtomicU8::new(0);
static POLL_EPOCH: AtomicU32 = AtomicU32::new(0);
static mut DECODER: ConsoleProtocolDecoder = ConsoleProtocolDecoder::new();

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SerialConsoleState {
    Disabled,
    PollingLog,
    AttachedSession,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SerialTxSource {
    AmbientLog,
    AttachedSession,
    Emergency,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SerialTxRoute {
    Drop,
    Ambient,
    Session,
    Emergency,
}

/// Pure ownership valve used by both TX paths and unit tests.
pub fn tx_route(state: SerialConsoleState, source: SerialTxSource) -> SerialTxRoute {
    match source {
        SerialTxSource::Emergency => SerialTxRoute::Emergency,
        SerialTxSource::AmbientLog if matches!(state, SerialConsoleState::Disabled | SerialConsoleState::PollingLog) => SerialTxRoute::Ambient,
        SerialTxSource::AttachedSession if matches!(state, SerialConsoleState::AttachedSession) => SerialTxRoute::Session,
        _ => SerialTxRoute::Drop,
    }
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
    if !crate::kernel::console::serial_log::init(port) {
        STATE.store(FAILED, Ordering::Release);
        return false;
    }
    PORT.store(encode(port), Ordering::Release);
    unsafe { DECODER = ConsoleProtocolDecoder::new(); }
    STATE.store(POLLING_LOG, Ordering::Release);
    true
}

/// Attach any interactive console endpoint to the configured serial port.
///
/// A target must first detach to `PollingLog`; direct replacement is rejected.
pub fn attach_session() -> bool {
    if PORT.load(Ordering::Acquire) == 0 {
        return false;
    }
    STATE.compare_exchange(
        POLLING_LOG,
        ATTACHED_SESSION,
        Ordering::AcqRel,
        Ordering::Acquire,
    ).is_ok()
}

/// Temporary compatibility wrapper for callers not yet migrated.
pub fn attach_early() -> bool { attach_session() }

/// Temporary compatibility wrapper for callers not yet migrated.
pub fn attach_personality() -> bool { attach_session() }

/// Return to ambient kernel logging while no interactive session owns serial.
pub fn detach_to_logging() {
    let state = STATE.load(Ordering::Acquire);
    if state == ATTACHED_SESSION {
        STATE.store(POLLING_LOG, Ordering::Release);
    }
}

/// Ambient logs own serial TX only while no interactive session is attached.
pub fn ambient_tx_allowed() -> bool {
    tx_route(state(), SerialTxSource::AmbientLog) == SerialTxRoute::Ambient
}

/// An attached session owns serial TX only while it also owns serial RX.
pub fn session_tx_allowed() -> bool {
    tx_route(state(), SerialTxSource::AttachedSession) == SerialTxRoute::Session
}

/// Ordinary terminal input is enabled only for an attached endpoint. Protocol
/// control frames remain available independently in `PollingLog`.
pub fn ordinary_rx_allowed() -> bool {
    matches!(state(), SerialConsoleState::AttachedSession)
}

/// Poll one decoded console event from the configured serial console.
///
/// Protocol controls are recognized in every live state, including
/// `PollingLog`, so reboot does not depend on an attached personality.
pub fn try_read_event() -> Option<ConsoleProtocolEvent> {
    let state = STATE.load(Ordering::Acquire);
    if state == DISABLED || state == FAILED {
        return None;
    }
    let port = decode(PORT.load(Ordering::Acquire))?;
    let epoch = u64::from(POLL_EPOCH.fetch_add(1, Ordering::Relaxed));
    let decoder = &raw mut DECODER;
    unsafe { (*decoder).expire(epoch); }
    for _ in 0..64 {
        let byte = Uart16550::new(port).try_read_byte()?;
        let event = unsafe { (*decoder).feed_at(byte, epoch) };
        if event.is_some() {
            return event;
        }
    }
    None
}

/// Compatibility helper for the early-console byte path.
pub fn try_read_byte() -> Option<u8> {
    if STATE.load(Ordering::Acquire) != ATTACHED_SESSION {
        return None;
    }
    let port = decode(PORT.load(Ordering::Acquire))?;
    Uart16550::new(port).try_read_byte()
}

pub fn state() -> SerialConsoleState {
    match STATE.load(Ordering::Acquire) {
        POLLING_LOG => SerialConsoleState::PollingLog,
        ATTACHED_SESSION => SerialConsoleState::AttachedSession,
        FAILED => SerialConsoleState::Failed,
        _ => SerialConsoleState::Disabled,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        attach_session, detach_to_logging, SerialConsoleState, STATE, DISABLED,
        POLLING_LOG,
    };
    use core::sync::atomic::Ordering;

    #[test]
    fn starts_disabled() {
        STATE.store(DISABLED, Ordering::Release);
        assert_eq!(super::state(), SerialConsoleState::Disabled);
    }

    #[test]
    fn tx_route_exhaustively_matches_shared_ownership() {
        use super::{SerialConsoleState as S, SerialTxRoute as R, SerialTxSource as T};
        for state in [S::Disabled, S::PollingLog, S::AttachedSession, S::Failed] {
            assert_eq!(super::tx_route(state, T::Emergency), R::Emergency);
        }
        assert_eq!(super::tx_route(S::PollingLog, T::AmbientLog), R::Ambient);
        assert_eq!(super::tx_route(S::AttachedSession, T::AmbientLog), R::Drop);
        assert_eq!(super::tx_route(S::PollingLog, T::AttachedSession), R::Drop);
        assert_eq!(super::tx_route(S::AttachedSession, T::AttachedSession), R::Session);
    }

    #[test]
    fn tx_valve_follows_the_shared_attachment_state() {
        super::PORT.store(1, Ordering::Release);
        STATE.store(POLLING_LOG, Ordering::Release);
        assert!(super::ambient_tx_allowed());
        assert!(!super::session_tx_allowed());
        assert!(!super::ordinary_rx_allowed());
        assert!(attach_session());
        assert!(!super::ambient_tx_allowed());
        assert!(super::session_tx_allowed());
        assert!(super::ordinary_rx_allowed());
        detach_to_logging();
        assert!(super::ambient_tx_allowed());
        assert!(!super::session_tx_allowed());
        assert!(!super::ordinary_rx_allowed());
    }

    #[test]
    fn session_handoffs_return_to_logging_before_replacement() {
        super::PORT.store(1, Ordering::Release);
        STATE.store(POLLING_LOG, Ordering::Release);
        assert!(attach_session());
        assert_eq!(super::state(), SerialConsoleState::AttachedSession);
        assert!(!attach_session());
        detach_to_logging();
        assert_eq!(super::state(), SerialConsoleState::PollingLog);
        assert!(attach_session());
        assert_eq!(super::state(), SerialConsoleState::AttachedSession);
        detach_to_logging();
        assert_eq!(super::state(), SerialConsoleState::PollingLog);
    }
}
