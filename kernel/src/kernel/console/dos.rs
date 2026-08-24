//! DOS console adapter.
//!
//! This adapter deliberately preserves DOS's existing hardware-shaped input
//! path. Transport events become Set-1 scancodes, then `DosState::process_key`
//! queues them through the virtual keyboard, BIOS INT 09h, and the BDA ring.

use crate::kernel::thread;
use crate::Regs;
use super::session::InputDisposition;

pub struct DosConsoleAdapter<'a, A: crate::Arch> {
    dos: &'a mut thread::DosState<A>,
}

impl<'a, A: crate::Arch> DosConsoleAdapter<'a, A> {
    pub fn new(dos: &'a mut thread::DosState<A>) -> Self {
        Self { dos }
    }

    pub fn deliver_scancode(&mut self, machine: &mut A, regs: &mut Regs, scancode: u8) -> InputDisposition {
        self.dos.process_key(machine, regs, scancode);
        InputDisposition::Consumed
    }
}

/// Send DOS text output through the attached-session route. DOS's BIOS/VGA
/// rendering and ambient debug mirror remain owned by their existing paths.
pub fn write_attached_byte(byte: u8) {
    crate::kernel::serial_log::write_session_byte(byte);
}

/// Translate one terminal byte into a short Set-1 make/break sequence.
/// Returns zero for bytes without a faithful basic keyboard representation.
/// The caller owns the sequence storage so this helper allocates nothing and
/// does not introduce a DOS input queue.
pub fn ascii_to_scancodes(byte: u8, out: &mut [u8; 4]) -> usize {
    ascii_to_scancodes_explicit(byte, out)
}

fn ascii_to_scancodes_explicit(byte: u8, out: &mut [u8; 4]) -> usize {
    let (scan, shift) = match byte {
        b'a' | b'A' => (0x1E, byte == b'A'),
        b'b' | b'B' => (0x30, byte == b'B'),
        b'c' | b'C' => (0x2E, byte == b'C'),
        b'd' | b'D' => (0x20, byte == b'D'),
        b'e' | b'E' => (0x12, byte == b'E'),
        b'f' | b'F' => (0x21, byte == b'F'),
        b'g' | b'G' => (0x22, byte == b'G'),
        b'h' | b'H' => (0x23, byte == b'H'),
        b'i' | b'I' => (0x17, byte == b'I'),
        b'j' | b'J' => (0x24, byte == b'J'),
        b'k' | b'K' => (0x25, byte == b'K'),
        b'l' | b'L' => (0x26, byte == b'L'),
        b'm' | b'M' => (0x32, byte == b'M'),
        b'n' | b'N' => (0x31, byte == b'N'),
        b'o' | b'O' => (0x18, byte == b'O'),
        b'p' | b'P' => (0x19, byte == b'P'),
        b'q' | b'Q' => (0x10, byte == b'Q'),
        b'r' | b'R' => (0x13, byte == b'R'),
        b's' | b'S' => (0x1F, byte == b'S'),
        b't' | b'T' => (0x14, byte == b'T'),
        b'u' | b'U' => (0x16, byte == b'U'),
        b'v' | b'V' => (0x2F, byte == b'V'),
        b'w' | b'W' => (0x11, byte == b'W'),
        b'x' | b'X' => (0x2D, byte == b'X'),
        b'y' | b'Y' => (0x15, byte == b'Y'),
        b'z' | b'Z' => (0x2C, byte == b'Z'),
        b'1' | b'!' => (0x02, byte == b'!'),
        b'2' | b'@' => (0x03, byte == b'@'),
        b'3' | b'#' => (0x04, byte == b'#'),
        b'4' | b'$' => (0x05, byte == b'$'),
        b'5' | b'%' => (0x06, byte == b'%'),
        b'6' | b'^' => (0x07, byte == b'^'),
        b'7' | b'&' => (0x08, byte == b'&'),
        b'8' | b'*' => (0x09, byte == b'*'),
        b'9' | b'(' => (0x0A, byte == b'('),
        b'0' | b')' => (0x0B, byte == b')'),
        b' ' => (0x39, false),
        b'\r' | b'\n' => (0x1C, false),
        0x08 | 0x7F => (0x0E, false),
        0x1B => (0x01, false),
        0x09 => (0x0F, false),
        _ => return 0,
    };

    let mut n = 0;
    if shift {
        out[n] = 0x2A;
        n += 1;
    }
    out[n] = scan;
    n += 1;
    out[n] = scan | 0x80;
    n += 1;
    if shift {
        out[n] = 0xAA;
        n += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::ascii_to_scancodes;

    #[test]
    fn translates_basic_ascii_to_make_break() {
        let mut events = [0; 4];
        assert_eq!(ascii_to_scancodes(b'q', &mut events), 2);
        assert_eq!(&events[..2], &[0x10, 0x90]);
    }

    #[test]
    fn translates_uppercase_with_shift() {
        let mut events = [0; 4];
        assert_eq!(ascii_to_scancodes(b'Q', &mut events), 4);
        assert_eq!(&events, &[0x2A, 0x10, 0x90, 0xAA]);
    }

    #[test]
    fn translates_controls() {
        let mut events = [0; 4];
        assert_eq!(ascii_to_scancodes(b'\r', &mut events), 2);
        assert_eq!(&events[..2], &[0x1C, 0x9C]);
        assert_eq!(ascii_to_scancodes(0, &mut events), 0);
    }
}
