//! Nanosecond timestamps and generic history markers for deterministic audio.

/// Monotonic logical audio time, expressed in nanoseconds.
///
/// The active architecture supplies the value. This type does not advance
/// itself and does not represent a sink or DMA cursor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct AudioTime(u64);

impl AudioTime {
    pub const ZERO: Self = Self(0);

    pub const fn from_nanos(nanos: u64) -> Self {
        Self(nanos)
    }

    pub const fn as_nanos(self) -> u64 {
        self.0
    }

    pub const fn saturating_duration_since(self, earlier: Self) -> u64 {
        self.0.saturating_sub(earlier.0)
    }
}

/// A device operation together with the logical time at which the guest
/// performed it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimedEvent<T> {
    pub at: AudioTime,
    pub event: T,
}

/// Whether a source should produce PCM or only advance its internal state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderMode {
    ProducePcm,
    AdvanceOnly,
}

/// Convert nanoseconds to a sample-frame position without floating point.
pub const fn audio_time_to_frame(time: AudioTime, sample_rate: u32) -> u64 {
    ((time.as_nanos() as u128 * sample_rate as u128) / 1_000_000_000) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_is_ordered_and_frame_conversion_is_integer() {
        let t = AudioTime::from_nanos(1_000_000_000);
        assert!(t > AudioTime::ZERO);
        assert_eq!(audio_time_to_frame(t, 48_000), 48_000);
        assert_eq!(audio_time_to_frame(AudioTime::from_nanos(500_000), 48_000), 24);
    }

    #[test]
    fn duration_does_not_underflow() {
        assert_eq!(
            AudioTime::from_nanos(10).saturating_duration_since(AudioTime::from_nanos(20)),
            0
        );
    }
}
