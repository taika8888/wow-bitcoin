use std::time::Duration;

use crate::market::Quote;
use crate::time::Timestamp;

#[derive(Clone, Copy, Debug, PartialEq)]
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
        started_at: Timestamp,
        now: Timestamp,
        refresh_interval: f64,
    ) -> FeedStatus {
        let threshold = refresh_interval * Self::MISSED_REFRESHES_BEFORE_STALE;
        match last_good {
            None if now.seconds_since(started_at) > threshold => FeedStatus::Offline,
            None => FeedStatus::Loading,
            Some(quote) if now.seconds_since(quote.fetched_at) > threshold => FeedStatus::Stale(*quote),
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

    pub fn quote(&self) -> Option<&Quote> {
        match self {
            FeedStatus::Live(quote) | FeedStatus::Stale(quote) => Some(quote),
            FeedStatus::Loading | FeedStatus::Offline => None,
        }
    }
}

/// When the next quote request starts. Each wait runs from the previous request's start, so a slow or
/// timed-out request does not push the cadence back.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FeedSchedule {
    last_start: Option<Timestamp>,
    failures: u32,
}

impl FeedSchedule {
    pub const BACKOFF_CAP_SECONDS: f64 = 60.0;

    pub fn new() -> Self {
        Self::default()
    }

    /// Capped at the interval: with the cadence anchored on starts, a good quote, a failure and a good retry
    /// then land within three intervals, so one failure never reaches the stale threshold.
    pub fn request_timeout(refresh_seconds: u32) -> Duration {
        Duration::from_secs(u64::from(refresh_seconds.min(10)))
    }

    pub fn last_start(&self) -> Option<Timestamp> {
        self.last_start
    }

    pub fn failures(&self) -> u32 {
        self.failures
    }

    pub fn started(&mut self, at: Timestamp) {
        self.last_start = Some(at);
    }

    pub fn finished(&mut self, ok: bool) {
        self.failures = if ok { 0 } else { self.failures + 1 };
    }

    /// Failures from before a sleep say nothing about the network after it.
    pub fn woke(&mut self) {
        self.failures = 0;
    }

    /// None when no request has started yet, so the first one starts at once.
    pub fn next_start(&self, refresh_seconds: u32) -> Option<Timestamp> {
        self.last_start.map(|start| start.plus(Self::delay(refresh_seconds, self.failures)))
    }

    pub fn is_due(&self, refresh_seconds: u32, now: Timestamp) -> bool {
        self.next_start(refresh_seconds).is_none_or(|start| now >= start)
    }

    /// The first retry keeps the normal cadence; backoff starts at the second failure in a row.
    pub fn delay(refresh_seconds: u32, failures: u32) -> f64 {
        let base = f64::from(refresh_seconds);
        if failures <= 1 {
            return base;
        }
        let doubled = base * 2f64.powi((failures - 1).min(6) as i32);
        base.max(doubled.min(Self::BACKOFF_CAP_SECONDS))
    }
}
