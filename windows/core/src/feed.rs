use crate::market::{Quote, Time};

#[derive(Clone, Debug, PartialEq)]
pub enum FeedStatus {
    Loading,
    Live(Quote),
    Stale(Quote),
    Offline,
}

impl FeedStatus {
    pub const MISSED_REFRESHES_BEFORE_STALE: f64 = 3.0;

    /// `started_at` separates "still waiting for the first quote" from "never got one".
    pub fn derive(
        last_good: Option<&Quote>,
        started_at: Time,
        now: Time,
        refresh_interval: f64,
    ) -> FeedStatus {
        let threshold = refresh_interval * Self::MISSED_REFRESHES_BEFORE_STALE;
        match last_good {
            None if now.since(started_at) > threshold => FeedStatus::Offline,
            None => FeedStatus::Loading,
            Some(quote) if now.since(quote.fetched_at) > threshold => FeedStatus::Stale(*quote),
            Some(quote) => FeedStatus::Live(*quote),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            FeedStatus::Loading => "loading",
            FeedStatus::Live(_) => "live",
            FeedStatus::Stale(_) => "stale",
            FeedStatus::Offline => "offline",
        }
    }
}

/// When the next quote request starts. Each wait runs from the previous request's start, so a slow or timed-out
/// request does not push the cadence back.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FeedSchedule {
    last_start: Option<Time>,
    failures: u32,
}

impl FeedSchedule {
    /// Capped at the interval: with the cadence anchored on starts, a good quote, a failure and a good retry then
    /// land within three intervals, so one failure never reaches the stale threshold.
    pub fn request_timeout(refresh_seconds: u32) -> f64 {
        f64::min(10.0, refresh_seconds.into())
    }

    pub fn started(&mut self, at: Time) {
        self.last_start = Some(at);
    }

    pub fn finished(&mut self, ok: bool) {
        self.failures = if ok { 0 } else { self.failures + 1 };
    }

    /// Failures from before a sleep say nothing about the network after it.
    pub fn woke(&mut self) {
        self.failures = 0;
    }

    pub fn failures(&self) -> u32 {
        self.failures
    }

    pub fn next_start(&self, refresh_seconds: u32) -> Time {
        match self.last_start {
            None => Time::DISTANT_PAST,
            Some(start) => start.plus(Self::delay(refresh_seconds, self.failures)),
        }
    }

    /// The first retry keeps the normal cadence; backoff starts at the second failure in a row, capped at a minute.
    pub fn delay(refresh_seconds: u32, failures: u32) -> f64 {
        let base = f64::from(refresh_seconds);
        if failures <= 1 {
            return base;
        }
        let doubled = base * 2f64.powi((failures - 1).min(6) as i32);
        base.max(doubled.min(60.0))
    }
}
