/// A moment, in seconds since the Unix epoch. The core never reads a clock: every time it uses is one the shell passed in.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Timestamp(f64);

impl Timestamp {
    pub const fn from_unix_seconds(seconds: f64) -> Self {
        Self(seconds)
    }

    pub const fn unix_seconds(self) -> f64 {
        self.0
    }

    pub fn plus(self, seconds: f64) -> Self {
        Self(self.0 + seconds)
    }

    pub fn seconds_since(self, earlier: Timestamp) -> f64 {
        self.0 - earlier.0
    }

    pub fn max(self, other: Timestamp) -> Self {
        if other > self { other } else { self }
    }
}
