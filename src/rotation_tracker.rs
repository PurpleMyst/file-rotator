use std::time::{Duration, Instant};

#[allow(dead_code)] // idk why this is needed
#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub(super) enum RotationTracker {
    Lines {
        period: usize,
        written: usize,
    },

    Bytes {
        period: usize,
        written: usize,
    },

    Interval {
        period: Duration,
        next_rotation: Instant,
    },

    Manual,
}

fn calc_next_rotation(period: Duration) -> Instant {
    Instant::now() + period
}

impl RotationTracker {
    /// Notify the tracker that we have written some amount of data
    pub(super) fn wrote(&mut self, buf: &[u8]) {
        match self {
            RotationTracker::Lines { written, .. } => {
                *written = written.saturating_add(buf.iter().filter(|&&c| c == b'\n').count())
            }

            RotationTracker::Bytes { written, .. } => *written = written.saturating_add(buf.len()),

            RotationTracker::Interval { .. } | RotationTracker::Manual => {}
        }
    }

    /// Ask the tracker if we should rotate before writing any more data
    pub(super) fn should_rotate(&self) -> bool {
        match self {
            RotationTracker::Lines { period, written }
            | RotationTracker::Bytes { period, written } => written >= period,

            RotationTracker::Interval { next_rotation, .. } => Instant::now()
                .checked_duration_since(*next_rotation)
                .is_some(),

            RotationTracker::Manual => false,
        }
    }

    /// Notify the tracker that we have rotated and so internal counters should be reset
    pub(super) fn reset(&mut self) {
        match self {
            RotationTracker::Lines { written, .. } | RotationTracker::Bytes { written, .. } => {
                *written = 0
            }

            RotationTracker::Interval {
                next_rotation,
                period,
            } => *next_rotation = calc_next_rotation(*period),

            RotationTracker::Manual => {}
        }
    }
}

impl From<super::RotationPeriod> for RotationTracker {
    fn from(rotate_every: super::RotationPeriod) -> Self {
        match rotate_every {
            super::RotationPeriod::Lines(period) => Self::Lines { period, written: 0 },
            super::RotationPeriod::Bytes(period) => Self::Bytes { period, written: 0 },
            super::RotationPeriod::Interval(period) => Self::Interval {
                next_rotation: calc_next_rotation(period),
                period,
            },
            super::RotationPeriod::Manual => Self::Manual,
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::super::RotationPeriod;
    use super::RotationTracker;

    proptest! {
        #[test]
        fn test_bytes(period in 0..=4096_usize) {
            let buf = vec![0; period];

            let mut tracker = RotationTracker::from(RotationPeriod::Bytes(period));

            if period == 0 {
                prop_assert!(tracker.should_rotate());
                return Ok(());
            }

            prop_assert!(!tracker.should_rotate());
            for chunk in buf[..period - 1].chunks(period.saturating_add(9) / 10) {
                tracker.wrote(chunk);
                prop_assert!(!tracker.should_rotate());
            }

            tracker.wrote(&buf[period - 1..]);
            prop_assert!(tracker.should_rotate());
        }

        // yes this is just the previous test changed to '\n', fight me irl
        #[test]
        fn test_lines(period in 0..=4096_usize) {
            let buf = vec![b'\n'; period];

            let mut tracker = RotationTracker::from(RotationPeriod::Lines(period));

            if period == 0 {
                prop_assert!(tracker.should_rotate());
                return Ok(());
            }

            prop_assert!(!tracker.should_rotate());
            for chunk in buf[..period - 1].chunks(period.saturating_add(9) / 10) {
                tracker.wrote(chunk);
                prop_assert!(!tracker.should_rotate());
            }

            tracker.wrote(&buf[period - 1..]);
            prop_assert!(tracker.should_rotate());
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            cases: 3,
            timeout: 5 * 1000,
            ..ProptestConfig::default()
        })]

        // #[test]
        fn test_interval(period in 1..=3u64) {
            let period = std::time::Duration::from_secs(period);
            let tracker = RotationTracker::from(RotationPeriod::Interval(period));

            prop_assert!(!tracker.should_rotate());
            std::thread::sleep(period);
            prop_assert!(tracker.should_rotate());
        }
    }

    #[test]
    fn test_manual() {
        let mut tracker = RotationTracker::from(RotationPeriod::Manual);
        assert!(!tracker.should_rotate());
        tracker.wrote(b"hello, world");
        assert!(!tracker.should_rotate());
        tracker.reset();
        assert!(!tracker.should_rotate());
    }
}
