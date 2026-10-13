//! In-process metrics: order and trade counts, submit and WAL latency.
//!
//! Two views of the same numbers:
//! - The last 60 seconds in one-second buckets, with raw latency samples so p50 and p99
//!   are exact, for the load dashboard (`GET /v1/stats`).
//! - All-time counters and fixed-bucket histograms in the Prometheus text format, for
//!   scraping (`GET /metrics`, admin listener only).

use serde::Serialize;
use std::collections::VecDeque;
use std::fmt::Write;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use utoipa::ToSchema;

/// Seconds of history kept for the dashboard.
pub const WINDOW_SECS: u64 = 60;
/// Latency samples kept per second; beyond this a second's percentiles use a sample.
const MAX_SAMPLES: usize = 20_000;
/// Prometheus histogram bucket bounds, in seconds.
const BOUNDS: [f64; 12] = [
    0.000_025, 0.000_05, 0.000_1, 0.000_25, 0.000_5, 0.001, 0.002_5, 0.005, 0.01, 0.025, 0.05, 0.1,
];

#[derive(Default)]
struct Second {
    unix: u64,
    orders: u64,
    rejected: u64,
    trades: u64,
    /// Submit latencies in microseconds.
    samples: Vec<u32>,
}

#[derive(Default, Clone)]
struct Histogram {
    counts: [u64; BOUNDS.len()],
    count: u64,
    sum: f64,
}

impl Histogram {
    fn observe(&mut self, d: Duration) {
        let secs = d.as_secs_f64();
        for (bound, count) in BOUNDS.iter().zip(self.counts.iter_mut()) {
            if secs <= *bound {
                *count += 1;
            }
        }
        self.count += 1;
        self.sum += secs;
    }

    fn render(&self, out: &mut String, name: &str, help: &str) {
        let _ = writeln!(out, "# HELP {name} {help}");
        let _ = writeln!(out, "# TYPE {name} histogram");
        for (bound, count) in BOUNDS.iter().zip(self.counts.iter()) {
            let _ = writeln!(out, "{name}_bucket{{le=\"{bound}\"}} {count}");
        }
        let _ = writeln!(out, "{name}_bucket{{le=\"+Inf\"}} {}", self.count);
        let _ = writeln!(out, "{name}_sum {}", self.sum);
        let _ = writeln!(out, "{name}_count {}", self.count);
    }
}

#[derive(Default)]
struct Inner {
    seconds: VecDeque<Second>,
    orders: u64,
    rejected: u64,
    trades: u64,
    cancels: u64,
    submit: Histogram,
    wal: Histogram,
}

impl Inner {
    /// The bucket for `unix`, created if it's the newest. `None` for a time older than
    /// the window, which then only counts in the totals.
    fn second(&mut self, unix: u64) -> Option<&mut Second> {
        let newest = self.seconds.back().map_or(unix, |s| s.unix.max(unix));
        while self
            .seconds
            .front()
            .is_some_and(|s| s.unix + WINDOW_SECS <= newest)
        {
            self.seconds.pop_front();
        }
        if unix + WINDOW_SECS <= newest {
            return None;
        }
        if self.seconds.back().is_none_or(|s| s.unix < unix) {
            self.seconds.push_back(Second {
                unix,
                ..Second::default()
            });
            return self.seconds.back_mut();
        }
        // Out of order but inside the window: its own bucket, or a new one in place.
        let at = self.seconds.partition_point(|s| s.unix < unix);
        if self.seconds.get(at).is_none_or(|s| s.unix != unix) {
            self.seconds.insert(
                at,
                Second {
                    unix,
                    ..Second::default()
                },
            );
        }
        self.seconds.get_mut(at)
    }
}

pub struct Metrics {
    started: Instant,
    inner: Mutex<Inner>,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            inner: Mutex::new(Inner::default()),
        }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema, PartialEq)]
pub struct SecondStats {
    /// Unix time of the second
    pub t: u64,
    pub orders: u64,
    pub rejected: u64,
    pub trades: u64,
    /// Submit latency percentiles in microseconds; null when no order arrived that second
    pub p50_us: Option<u32>,
    pub p99_us: Option<u32>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct StatsResponse {
    pub uptime_secs: u64,
    /// Since the server started (not reset by a market reset)
    pub orders: u64,
    pub rejected: u64,
    pub trades: u64,
    pub cancels: u64,
    /// The last 60 seconds, oldest first, one entry per second including empty ones
    pub seconds: Vec<SecondStats>,
    /// Submit latency percentiles over the whole window, in microseconds
    pub p50_us: Option<u32>,
    pub p99_us: Option<u32>,
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn micros(d: Duration) -> u32 {
    d.as_micros().min(u32::MAX as u128) as u32
}

/// Nearest-rank percentile of sorted samples.
fn percentile(sorted: &[u32], q: f64) -> Option<u32> {
    if sorted.is_empty() {
        return None;
    }
    let rank = ((q * sorted.len() as f64).ceil() as usize).clamp(1, sorted.len());
    Some(sorted[rank - 1])
}

impl Metrics {
    /// One submit, from the session lock request to the shard's reply. `accepted` is
    /// false when the session gate or risk checks rejected it.
    pub fn record_submit(&self, latency: Duration, accepted: bool) {
        self.record_submit_at(unix_now(), latency, accepted);
    }

    pub fn record_submit_at(&self, unix: u64, latency: Duration, accepted: bool) {
        let mut inner = self.inner.lock().unwrap();
        inner.orders += 1;
        if !accepted {
            inner.rejected += 1;
        }
        inner.submit.observe(latency);
        if let Some(second) = inner.second(unix) {
            second.orders += 1;
            if !accepted {
                second.rejected += 1;
            }
            if second.samples.len() < MAX_SAMPLES {
                second.samples.push(micros(latency));
            }
        }
    }

    pub fn record_trades(&self, n: usize) {
        if n == 0 {
            return;
        }
        let mut inner = self.inner.lock().unwrap();
        inner.trades += n as u64;
        if let Some(second) = inner.second(unix_now()) {
            second.trades += n as u64;
        }
    }

    pub fn record_cancel(&self) {
        self.inner.lock().unwrap().cancels += 1;
    }

    pub fn record_wal(&self, latency: Duration) {
        self.inner.lock().unwrap().wal.observe(latency);
    }

    pub fn stats(&self) -> StatsResponse {
        self.stats_at(unix_now())
    }

    pub fn stats_at(&self, now: u64) -> StatsResponse {
        let inner = self.inner.lock().unwrap();
        let mut all = Vec::new();
        let seconds = (now + 1 - WINDOW_SECS..=now)
            .map(|t| match inner.seconds.iter().find(|s| s.unix == t) {
                Some(s) => {
                    let mut sorted = s.samples.clone();
                    sorted.sort_unstable();
                    all.extend_from_slice(&sorted);
                    SecondStats {
                        t,
                        orders: s.orders,
                        rejected: s.rejected,
                        trades: s.trades,
                        p50_us: percentile(&sorted, 0.50),
                        p99_us: percentile(&sorted, 0.99),
                    }
                }
                None => SecondStats {
                    t,
                    orders: 0,
                    rejected: 0,
                    trades: 0,
                    p50_us: None,
                    p99_us: None,
                },
            })
            .collect();
        all.sort_unstable();
        StatsResponse {
            uptime_secs: self.started.elapsed().as_secs(),
            orders: inner.orders,
            rejected: inner.rejected,
            trades: inner.trades,
            cancels: inner.cancels,
            seconds,
            p50_us: percentile(&all, 0.50),
            p99_us: percentile(&all, 0.99),
        }
    }

    /// Prometheus text exposition format.
    pub fn prometheus(&self) -> String {
        let inner = self.inner.lock().unwrap();
        let mut out = String::new();
        let counter = |out: &mut String, name: &str, help: &str, value: u64| {
            let _ = writeln!(out, "# HELP {name} {help}");
            let _ = writeln!(out, "# TYPE {name} counter");
            let _ = writeln!(out, "{name} {value}");
        };
        counter(
            &mut out,
            "tse_orders_total",
            "Orders submitted, accepted or not.",
            inner.orders,
        );
        counter(
            &mut out,
            "tse_orders_rejected_total",
            "Orders rejected by the session gate or risk checks.",
            inner.rejected,
        );
        counter(
            &mut out,
            "tse_trades_total",
            "Trades executed.",
            inner.trades,
        );
        counter(
            &mut out,
            "tse_cancels_total",
            "Resting orders cancelled.",
            inner.cancels,
        );
        inner.submit.render(
            &mut out,
            "tse_submit_latency_seconds",
            "Order submit latency, from the session lock request to the shard's reply.",
        );
        inner.wal.render(
            &mut out,
            "tse_wal_write_latency_seconds",
            "Write-ahead log append and flush latency.",
        );
        let _ = writeln!(
            out,
            "# HELP tse_uptime_seconds Seconds since the server started."
        );
        let _ = writeln!(out, "# TYPE tse_uptime_seconds gauge");
        let _ = writeln!(
            out,
            "tse_uptime_seconds {}",
            self.started.elapsed().as_secs()
        );
        out
    }
}
