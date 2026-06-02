use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Write};
use uuid::Uuid;

/// Self-contained WAL entry using primitive types for serialization.
/// Engine domain types are mapped to strings/UUIDs at the call site to keep
/// this crate free of a hard dependency on the engine crate.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WalEntry {
    OrderSubmitted {
        order_id: Uuid,
        symbol: String,
        side: String,
        quantity: String,
        price: Option<String>,
        client_id: String,
    },
    OrderCancelled {
        order_id: Uuid,
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
        state: String,
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

    /// Appends a length-prefixed JSON record and flushes immediately for durability.
    pub fn append(&mut self, entry: &WalEntry) -> std::io::Result<()> {
        let serialized = serde_json::to_vec(entry)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        let len = (serialized.len() as u32).to_le_bytes();
        self.writer.write_all(&len)?;
        self.writer.write_all(&serialized)?;
        self.writer.flush()?;
        Ok(())
    }

    /// Reads all entries from a WAL file in order. Used for crash recovery.
    pub fn replay(path: &str) -> std::io::Result<Vec<WalEntry>> {
        let file = match File::open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(e),
        };
        let mut reader = BufReader::new(file);
        let mut entries = Vec::new();
        loop {
            let mut len_buf = [0u8; 4];
            match reader.read_exact(&mut len_buf) {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e),
            }
            let len = u32::from_le_bytes(len_buf) as usize;
            let mut buf = vec![0u8; len];
            reader.read_exact(&mut buf)?;
            let entry: WalEntry = serde_json::from_slice(&buf)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            entries.push(entry);
        }
        Ok(entries)
    }
}
