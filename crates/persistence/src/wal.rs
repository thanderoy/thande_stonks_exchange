use engine::order::{Order, Side};
use engine::session::SessionState;
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
}

pub struct WriteAheadLog {
    writer: BufWriter<File>,
}

impl WriteAheadLog {
    pub fn open(path: &str) -> std::io::Result<Self> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self {
            writer: BufWriter::new(file),
        })
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
        let serialized = serde_json::to_vec(entry).map_err(std::io::Error::other)?;
        let len = (serialized.len() as u32).to_le_bytes();
        self.writer.write_all(&len)?;
        self.writer.write_all(&serialized)?;
        self.writer.flush()?;
        Ok(())
    }

    /// Reads all entries from a WAL file in order, ignoring a torn final record.
    pub fn replay(path: &str) -> std::io::Result<Vec<WalEntry>> {
        read_entries(path).map(|(entries, _)| entries)
    }
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
