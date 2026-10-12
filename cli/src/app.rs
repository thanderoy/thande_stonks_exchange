use chrono::{DateTime, Local};
use rust_decimal::Decimal;
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct BookSnapshot {
    pub bids: Vec<(Decimal, Decimal)>,
    pub asks: Vec<(Decimal, Decimal)>,
    pub updated_at: Instant,
}

impl Default for BookSnapshot {
    fn default() -> Self {
        Self {
            bids: vec![],
            asks: vec![],
            updated_at: Instant::now(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct EventEntry {
    pub ts: DateTime<Local>,
    pub kind: EventKind,
    pub symbol: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EventKind {
    Trade,
    Accepted,
    Cancelled,
    Session,
    System,
}

pub struct Metrics {
    pub total_submits: u64,
    pub total_trades: u64,
    pub submit_timestamps: VecDeque<Instant>,
    pub trade_timestamps: VecDeque<Instant>,
    pub latencies_ms: VecDeque<f64>,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            total_submits: 0,
            total_trades: 0,
            submit_timestamps: VecDeque::new(),
            trade_timestamps: VecDeque::new(),
            latencies_ms: VecDeque::new(),
        }
    }
}

impl Metrics {
    pub fn record_submit(&mut self, latency_ms: f64) {
        let now = Instant::now();
        self.total_submits += 1;
        self.submit_timestamps.push_back(now);
        self.latencies_ms.push_back(latency_ms);
        // keep rolling 60s window
        let cutoff = now - Duration::from_secs(60);
        while self.submit_timestamps.front().map_or(false, |t| *t < cutoff) {
            self.submit_timestamps.pop_front();
        }
        if self.latencies_ms.len() > 1000 {
            self.latencies_ms.pop_front();
        }
    }

    pub fn record_trade(&mut self) {
        let now = Instant::now();
        self.total_trades += 1;
        self.trade_timestamps.push_back(now);
        let cutoff = now - Duration::from_secs(60);
        while self.trade_timestamps.front().map_or(false, |t| *t < cutoff) {
            self.trade_timestamps.pop_front();
        }
    }

    pub fn submits_per_sec(&self) -> f64 {
        let cutoff = Instant::now() - Duration::from_secs(5);
        let count = self.submit_timestamps.iter().filter(|t| **t >= cutoff).count();
        count as f64 / 5.0
    }

    pub fn trades_per_sec(&self) -> f64 {
        let cutoff = Instant::now() - Duration::from_secs(5);
        let count = self.trade_timestamps.iter().filter(|t| **t >= cutoff).count();
        count as f64 / 5.0
    }

    pub fn latency_p50(&self) -> f64 {
        percentile(&self.latencies_ms, 50.0)
    }

    pub fn latency_p99(&self) -> f64 {
        percentile(&self.latencies_ms, 99.0)
    }

    pub fn fill_rate(&self) -> f64 {
        if self.total_submits == 0 {
            return 0.0;
        }
        (self.total_trades as f64 / self.total_submits as f64) * 100.0
    }
}

fn percentile(data: &VecDeque<f64>, pct: f64) -> f64 {
    if data.is_empty() {
        return 0.0;
    }
    let mut sorted: Vec<f64> = data.iter().cloned().collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let idx = ((pct / 100.0) * (sorted.len() - 1) as f64) as usize;
    sorted[idx.min(sorted.len() - 1)]
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    Normal,
    SubmitForm,
    CancelInput,
}

#[derive(Debug, Clone, Default)]
pub struct OrderForm {
    pub symbol: String,
    pub side: String,   // "Buy" or "Sell"
    pub price: String,
    pub qty: String,
    pub client: String,
    pub active_field: usize, // 0=side,1=price,2=qty,3=client
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct CancelForm {
    pub order_id: String,
    pub symbol: String,
    pub side: String,
    pub error: Option<String>,
}

pub struct MonitorState {
    pub base_url: String,
    pub symbols: Vec<String>,
    pub selected: usize,
    pub books: HashMap<String, BookSnapshot>,
    pub sessions: HashMap<String, String>,
    pub events: VecDeque<EventEntry>,
    pub metrics: Metrics,
    pub mode: InputMode,
    pub form: OrderForm,
    pub cancel_form: CancelForm,
    pub connected: bool,
    pub log: VecDeque<String>,
    pub auto_stream: bool,
    pub auto_interval_ms: u64,
}

impl MonitorState {
    pub fn new(base_url: String, symbols: Vec<String>) -> Self {
        let mut sessions = HashMap::new();
        let mut books = HashMap::new();
        for sym in &symbols {
            sessions.insert(sym.clone(), "Unknown".to_string());
            books.insert(sym.clone(), BookSnapshot::default());
        }
        Self {
            base_url,
            symbols,
            selected: 0,
            books,
            sessions,
            events: VecDeque::new(),
            metrics: Metrics::default(),
            mode: InputMode::Normal,
            form: OrderForm::default(),
            cancel_form: CancelForm::default(),
            connected: false,
            log: VecDeque::new(),
            auto_stream: false,
            auto_interval_ms: 200,
        }
    }

    pub fn selected_symbol(&self) -> &str {
        self.symbols.get(self.selected).map(|s| s.as_str()).unwrap_or("")
    }

    pub fn push_event(&mut self, entry: EventEntry) {
        self.events.push_front(entry);
        if self.events.len() > 200 {
            self.events.pop_back();
        }
    }

    pub fn push_log(&mut self, msg: String) {
        self.events.push_front(EventEntry {
            ts: Local::now(),
            kind: EventKind::System,
            symbol: String::new(),
            detail: msg,
        });
        if self.events.len() > 200 {
            self.events.pop_back();
        }
    }
}
