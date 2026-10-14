//! Retry policy for trusted host receipt time; never a caller-supplied audit time.
use fleetiq_domain::UnixTimeMillis;

/// Internal policy keeps existing exact commands distinct from redelivered HTTP commands.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RecordedTime {
    Exact(UnixTimeMillis),
    FirstReceipt(UnixTimeMillis),
}
impl RecordedTime {
    pub(crate) const fn value(self) -> UnixTimeMillis {
        match self {
            Self::Exact(time) | Self::FirstReceipt(time) => time,
        }
    }
    pub(crate) fn accepts(self, stored: UnixTimeMillis) -> bool {
        match self {
            Self::Exact(time) => time == stored,
            Self::FirstReceipt(_) => true,
        }
    }
}
