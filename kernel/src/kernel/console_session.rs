//! Lightweight console-session mediation.
//!
//! A session routes input to an existing personality endpoint and fans output
//! out to attached sinks. It deliberately owns no input queue, line discipline,
//! terminal grid, or hardware device.

pub type AttachmentId = u8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConsoleEndpointId {
    Early,
    TtyLike(u8),
    Dos(usize),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputPolicy {
    Exclusive(AttachmentId),
    Shared,
    LocalPriority,
    SerialPriority,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputOrigin {
    AmbientLog,
    Terminal,
    DosConsole,
    EarlyConsole,
}

pub trait InputAdapter {
    fn deliver(&mut self, input: InputEvent) -> InputDisposition;
}

pub trait OutputObserver {
    fn output(&mut self, origin: OutputOrigin, byte: u8);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputEvent {
    Byte(u8),
    Scancode(u8),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputDisposition {
    Consumed,
    Ignored,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConsoleSettings {
    /// Allow endpoint-requested input echo to reach session output.
    pub echo: bool,
}

impl Default for ConsoleSettings {
    fn default() -> Self {
        Self { echo: true }
    }
}

pub trait EchoSink {
    fn write_byte(&mut self, byte: u8);
}

pub trait ConsoleEndpoint {
    fn input(&mut self, event: InputEvent, echo: &mut dyn EchoSink) -> InputDisposition;
}

pub trait OutputAttachment {
    fn write_byte(&mut self, byte: u8);
}

struct NullEcho;

impl EchoSink for NullEcho {
    fn write_byte(&mut self, _byte: u8) {}
}

struct OutputFanout<'a, V: OutputAttachment, S: OutputAttachment> {
    video: &'a mut V,
    serial: Option<&'a mut S>,
}

impl<V: OutputAttachment, S: OutputAttachment> EchoSink for OutputFanout<'_, V, S> {
    fn write_byte(&mut self, byte: u8) {
        self.video.write_byte(byte);
        if let Some(serial) = self.serial.as_deref_mut() {
            serial.write_byte(byte);
        }
    }
}

/// A borrowed mediator between one endpoint and optional output attachments.
///
/// The endpoint remains responsible for its canonical input representation. For
/// example, a TTY-like endpoint can write directly to `kpipe`, while DOS can
/// inject into its BDA-compatible keyboard path.
pub struct ConsoleSession<'a, E, V, S>
where
    E: ConsoleEndpoint,
    V: OutputAttachment,
    S: OutputAttachment,
{
    endpoint: &'a mut E,
    video: &'a mut V,
    serial: Option<&'a mut S>,
    settings: ConsoleSettings,
}

impl<'a, E, V, S> ConsoleSession<'a, E, V, S>
where
    E: ConsoleEndpoint,
    V: OutputAttachment,
    S: OutputAttachment,
{
    pub fn new(
        endpoint: &'a mut E,
        video: &'a mut V,
        serial: Option<&'a mut S>,
        settings: ConsoleSettings,
    ) -> Self {
        Self { endpoint, video, serial, settings }
    }

    pub fn input(&mut self, event: InputEvent) -> InputDisposition {
        if self.settings.echo {
            let mut output = OutputFanout {
                video: self.video,
                serial: self.serial.as_deref_mut(),
            };
            self.endpoint.input(event, &mut output)
        } else {
            let mut echo = NullEcho;
            self.endpoint.input(event, &mut echo)
        }
    }

    pub fn write_byte(&mut self, byte: u8) {
        self.video.write_byte(byte);
        if let Some(serial) = self.serial.as_deref_mut() {
            serial.write_byte(byte);
        }
    }

    pub fn write_bytes(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.write_byte(byte);
        }
    }

    pub fn endpoint_mut(&mut self) -> &mut E {
        self.endpoint
    }

    pub fn settings(&self) -> ConsoleSettings {
        self.settings
    }

    pub fn set_echo(&mut self, echo: bool) {
        self.settings.echo = echo;
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;
    use super::{ConsoleEndpoint, ConsoleSession, ConsoleSettings, EchoSink, InputDisposition, InputEvent, OutputAttachment};

    struct Endpoint;

    impl ConsoleEndpoint for Endpoint {
        fn input(&mut self, event: InputEvent, echo: &mut dyn EchoSink) -> InputDisposition {
            match event {
                InputEvent::Byte(byte) => {
                    echo.write_byte(byte);
                    InputDisposition::Consumed
                }
                InputEvent::Scancode(_) => InputDisposition::Ignored,
            }
        }
    }

    #[derive(Default)]
    struct Sink(Vec<u8>);

    impl OutputAttachment for Sink {
        fn write_byte(&mut self, byte: u8) {
            self.0.push(byte);
        }
    }

    #[test]
    fn echoes_to_video_and_serial_without_session_buffering() {
        let mut endpoint = Endpoint;
        let mut video = Sink::default();
        let mut serial = Sink::default();
        let mut session = ConsoleSession::new(
            &mut endpoint,
            &mut video,
            Some(&mut serial),
            ConsoleSettings::default(),
        );

        assert_eq!(session.input(InputEvent::Byte(b'x')), InputDisposition::Consumed);
        assert_eq!(video.0, b"x");
        assert_eq!(serial.0, b"x");
    }

    #[test]
    fn disabled_echo_still_delivers_input_without_output() {
        let mut endpoint = Endpoint;
        let mut video = Sink::default();
        let mut serial = Sink::default();
        let mut session = ConsoleSession::new(
            &mut endpoint,
            &mut video,
            Some(&mut serial),
            ConsoleSettings { echo: false },
        );

        assert_eq!(session.input(InputEvent::Byte(b'x')), InputDisposition::Consumed);
        assert!(video.0.is_empty());
        assert!(serial.0.is_empty());
    }

    #[test]
    fn output_is_mirrored_without_input() {
        let mut endpoint = Endpoint;
        let mut video = Sink::default();
        let mut serial = Sink::default();
        let mut session = ConsoleSession::new(
            &mut endpoint,
            &mut video,
            Some(&mut serial),
            ConsoleSettings::default(),
        );

        session.write_bytes(b"ok");
        assert_eq!(video.0, b"ok");
        assert_eq!(serial.0, b"ok");
    }
}
