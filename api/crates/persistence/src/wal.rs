use chrono::{DateTime, Utc};
use engine::order::{Order, Side};
use engine::session::SessionState;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Write};
use uuid::Uuid;

/// One state-changing event. Replaying `OrderSubmitted`, `OrderCancelled` and
/// `SessionStateChanged` in order rebuilds the books, positions and sessions; they carry
/// engine types so replay is lossless. `TradeExecuted` is an audit record only — replay
/// regenerates trades by re-matching.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WalEntry {
    OrderSubmitted {
        order: Order,
    },
    OrderCancelled {
        order_id: Uuid,
        symbol: String,
        side: Side,
        timestamp: String,
    },
    TradeExecuted {
        trade_id: Uuid,
        symbol: String,
        price: String,
        quantity: String,
        buyer_order_id: Uuid,
        seller_order_id: Uuid,
        executed_at: String,
    },
    SessionStateChanged {
        symbol: String,
        state: SessionState,
    },
    /// A participant created at runtime (a guest joining). Replay re-registers it with
    /// these limits, so its later orders pass the participant lookup again.
    ParticipantRegistered {
        client_id: String,
        display_name: String,
        max_notional_per_order: Decimal,
        max_position: Decimal,
        max_orders_per_second: u32,
        /// Absent in entries written before it was recorded.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        registered_at: Option<DateTime<Utc>>,
    },
}

pub struct WriteAheadLog {
    path: String,
    writer: BufWriter<File>,
}

impl WriteAheadLog {
    pub fn open(path: &str) -> std::io::Result<Self> {
        Ok(Self {
            path: path.to_string(),
            writer: BufWriter::new(open_append(path)?),
        })
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    /// Starts a new log at the same path holding only `entries`, and keeps the old one at
    /// `<path>.prev`, replacing any earlier `.prev`. The new log is written and synced to
    /// `<path>.next` first and then renamed over `path`, so a crash leaves either the old
    /// log or the new one in place.
    pub fn rotate(&mut self, entries: &[WalEntry]) -> std::io::Result<()> {
        self.writer.flush()?;
        let next = format!("{}.next", self.path);
        let prev = format!("{}.prev", self.path);
        {
            let mut writer = BufWriter::new(File::create(&next)?);
            for entry in entries {
                write_record(&mut writer, entry)?;
            }
            writer.flush()?;
            writer.get_ref().sync_all()?;
        }
        match std::fs::remove_file(&prev) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
        if std::fs::hard_link(&self.path, &prev).is_err() {
            std::fs::copy(&self.path, &prev)?;
        }
        std::fs::rename(&next, &self.path)?;
        self.writer = BufWriter::new(open_append(&self.path)?);
        Ok(())
    }

    /// Replays the log and opens it for appending. A record torn by a crash mid-write is
    /// truncated away first, so new entries don't land after unreadable bytes.
    pub fn open_for_recovery(path: &str) -> std::io::Result<(Self, Vec<WalEntry>)> {
        let (entries, valid_len) = read_entries(path)?;
        let file_len = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        if file_len > valid_len {
            OpenOptions::new()
                .write(true)
                .open(path)?
                .set_len(valid_len)?;
        }
        Ok((Self::open(path)?, entries))
    }

    /// Appends a length-prefixed JSON record and flushes immediately for durability.
    pub fn append(&mut self, entry: &WalEntry) -> std::io::Result<()> {
        write_record(&mut self.writer, entry)?;
        self.writer.flush()
    }

    /// Reads all entries from a WAL file in order, ignoring a torn final record.
    pub fn replay(path: &str) -> std::io::Result<Vec<WalEntry>> {
        read_entries(path).map(|(entries, _)| entries)
    }
}

fn open_append(path: &str) -> std::io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

fn write_record(writer: &mut impl Write, entry: &WalEntry) -> std::io::Result<()> {
    let serialized = serde_json::to_vec(entry).map_err(std::io::Error::other)?;
    writer.write_all(&(serialized.len() as u32).to_le_bytes())?;
    writer.write_all(&serialized)
}

/// Returns the complete entries and the byte length they occupy.
fn read_entries(path: &str) -> std::io::Result<(Vec<WalEntry>, u64)> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((vec![], 0)),
        Err(e) => return Err(e),
    };
    let mut reader = BufReader::new(file);
    let mut entries = Vec::new();
    let mut valid_len = 0u64;
    loop {
        let mut len_buf = [0u8; 4];
        match reader.read_exact(&mut len_buf) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e),
        }
        let len = u32::from_le_bytes(len_buf) as usize;
        let mut buf = vec![0u8; len];
        match reader.read_exact(&mut buf) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e),
        }
        let entry: WalEntry = serde_json::from_slice(&buf)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        entries.push(entry);
        valid_len += 4 + len as u64;
    }
    Ok((entries, valid_len))
}
