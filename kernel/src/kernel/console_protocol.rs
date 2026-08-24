//! Transport-neutral console escape protocol.
//!
//! Raw terminal bytes remain the default. The configured console may also carry
//! bounded control frames, which are decoded before any personality sees them:
//!
//!     DLE STX <command and payload> DLE ETX
//!
//! DLE is 0x10, STX is 0x02, and ETX is 0x03. DLE DLE represents a literal DLE
//! inside a frame. Key events contain one action byte (0 = down, 1 = up) and
//! one Set-1 scancode byte. Extended prefixes are sent as their own key event,
//! preserving the existing InputEvent::Scancode representation.

use super::console_session::InputEvent;

const DLE: u8 = 0x10;
const STX: u8 = 0x02;
const ETX: u8 = 0x03;
const REBOOT: u8 = 0x01;
const KEY_EVENT: u8 = 0x02;
const MAX_FRAME: usize = 8;
const FRAME_TIMEOUT_EPOCHS: u64 = 1_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConsoleControl {
    Reboot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConsoleProtocolEvent {
    Input(InputEvent),
    Control(ConsoleControl),
}

#[derive(Clone, Copy)]
enum State {
    Raw,
    RawDle,
    Frame { len: usize },
    FrameDle { len: usize },
}

pub struct ConsoleProtocolDecoder {
    state: State,
    frame: [u8; MAX_FRAME],
    last_activity_epoch: Option<u64>,
}

impl ConsoleProtocolDecoder {
    pub const fn new() -> Self {
        Self { state: State::Raw, frame: [0; MAX_FRAME], last_activity_epoch: None }
    }

    pub fn feed(&mut self, byte: u8) -> Option<ConsoleProtocolEvent> {
        self.feed_at(byte, 0)
    }

    /// Feed a byte with a monotonic poll epoch. A stale partial frame is
    /// discarded before this byte is interpreted as new input.
    pub fn feed_at(&mut self, byte: u8, epoch: u64) -> Option<ConsoleProtocolEvent> {
        self.expire(epoch);
        let event = match self.state {
            State::Raw => {
                if byte == DLE {
                    self.state = State::RawDle;
                    None
                } else {
                    Some(ConsoleProtocolEvent::Input(InputEvent::Byte(byte)))
                }
            }
            State::RawDle => match byte {
                DLE => {
                    self.state = State::Raw;
                    Some(ConsoleProtocolEvent::Input(InputEvent::Byte(DLE)))
                }
                STX => {
                    self.state = State::Frame { len: 0 };
                    None
                }
                _ => {
                    self.state = State::Raw;
                    None
                }
            },
            State::Frame { len } => {
                if byte == DLE {
                    self.state = State::FrameDle { len };
                } else if len < MAX_FRAME {
                    self.frame[len] = byte;
                    self.state = State::Frame { len: len + 1 };
                } else {
                    self.state = State::Raw;
                }
                None
            }
            State::FrameDle { len } => match byte {
                DLE if len < MAX_FRAME => {
                    self.frame[len] = DLE;
                    self.state = State::Frame { len: len + 1 };
                    None
                }
                ETX => {
                    self.state = State::Raw;
                    self.decode_frame(len)
                }
                _ => {
                    self.state = State::Raw;
                    None
                }
            },
        };
        self.last_activity_epoch = if matches!(self.state, State::Raw) {
            None
        } else {
            Some(epoch)
        };
        event
    }

    /// Discard a partial frame after a bounded idle interval so a dropped
    /// transport cannot permanently capture subsequent terminal bytes.
    pub fn expire(&mut self, epoch: u64) {
        if !matches!(self.state, State::Raw)
            && self.last_activity_epoch.is_some_and(|last| {
                epoch.saturating_sub(last) >= FRAME_TIMEOUT_EPOCHS
            })
        {
            self.state = State::Raw;
            self.last_activity_epoch = None;
        }
    }

    fn decode_frame(&self, len: usize) -> Option<ConsoleProtocolEvent> {
        match self.frame.get(..len)? {
            [REBOOT] => Some(ConsoleProtocolEvent::Control(ConsoleControl::Reboot)),
            [KEY_EVENT, action, scancode] if *action <= 1 => {
                let code = if *action == 0 { *scancode } else { *scancode | 0x80 };
                Some(ConsoleProtocolEvent::Input(InputEvent::Scancode(code)))
            }
            _ => None,
        }
    }
}

impl Default for ConsoleProtocolDecoder {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::{ConsoleControl, ConsoleProtocolDecoder, ConsoleProtocolEvent, DLE, ETX, FRAME_TIMEOUT_EPOCHS, KEY_EVENT, REBOOT, STX};
    use crate::kernel::console_session::InputEvent;

    fn frame(payload: &[u8]) -> alloc::vec::Vec<u8> {
        let mut bytes = alloc::vec![DLE, STX];
        for &byte in payload {
            if byte == DLE { bytes.push(DLE); }
            bytes.push(byte);
        }
        bytes.extend_from_slice(&[DLE, ETX]);
        bytes
    }

    fn feed(decoder: &mut ConsoleProtocolDecoder, bytes: &[u8]) -> alloc::vec::Vec<ConsoleProtocolEvent> {
        bytes.iter().filter_map(|&byte| decoder.feed(byte)).collect()
    }

    #[test]
    fn raw_bytes_pass_through() {
        let mut decoder = ConsoleProtocolDecoder::new();
        assert_eq!(feed(&mut decoder, b"help\r"), alloc::vec![
            ConsoleProtocolEvent::Input(InputEvent::Byte(b'h')),
            ConsoleProtocolEvent::Input(InputEvent::Byte(b'e')),
            ConsoleProtocolEvent::Input(InputEvent::Byte(b'l')),
            ConsoleProtocolEvent::Input(InputEvent::Byte(b'p')),
            ConsoleProtocolEvent::Input(InputEvent::Byte(b'\r')),
        ]);
    }

    #[test]
    fn reboot_is_a_control_event() {
        let mut decoder = ConsoleProtocolDecoder::new();
        assert_eq!(feed(&mut decoder, &frame(&[REBOOT])), alloc::vec![
            ConsoleProtocolEvent::Control(ConsoleControl::Reboot),
        ]);
    }

    #[test]
    fn key_frames_emit_set1_make_and_break_events() {
        let mut decoder = ConsoleProtocolDecoder::new();
        let mut bytes = frame(&[KEY_EVENT, 0, 0x1D]);
        bytes.extend_from_slice(&frame(&[KEY_EVENT, 1, 0x1D]));
        assert_eq!(feed(&mut decoder, &bytes), alloc::vec![
            ConsoleProtocolEvent::Input(InputEvent::Scancode(0x1D)),
            ConsoleProtocolEvent::Input(InputEvent::Scancode(0x9D)),
        ]);
    }

    #[test]
    fn escaped_dle_is_raw_data() {
        let mut decoder = ConsoleProtocolDecoder::new();
        assert_eq!(feed(&mut decoder, &[DLE, DLE]), alloc::vec![
            ConsoleProtocolEvent::Input(InputEvent::Byte(DLE)),
        ]);
    }

    #[test]
    fn stale_partial_frame_returns_to_raw_mode() {
        let mut decoder = ConsoleProtocolDecoder::new();
        assert_eq!(decoder.feed_at(DLE, 1), None);
        assert_eq!(decoder.feed_at(STX, 2), None);
        decoder.expire(FRAME_TIMEOUT_EPOCHS + 2);
        assert_eq!(decoder.feed_at(b'x', FRAME_TIMEOUT_EPOCHS + 3), Some(
            ConsoleProtocolEvent::Input(InputEvent::Byte(b'x')),
        ));
    }
}
