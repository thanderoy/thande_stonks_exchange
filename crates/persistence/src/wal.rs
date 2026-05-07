use std::io::{BufWriter, Write};
use std::fs::{File, OpenOptions};
use engine::order::Order;

pub enum WalEntry {
    OrderSubmitted(Order),
    OrderCancelled( id: OrderId, timestamp: DateTime<Utc>),
    TradeExecuted(Trade),
    SessionStateChanged ( symbol: Symbol,  state: SessionState ),
}

pub struct WriteAheadLog {
    writer: BufWriter<File>
}

impl WriteAheadLog {
    pub fn append(&mut self, entry: &WalEntry) -> std::io::Result<()> {
        let serialized = serde_json::to_vec(entry)?;
        self.writer.write_all(&serialized.len() as u32.to_le_bytes())?;
        self.writer.write_all(&serialized)?;
        self.writer.flush()?;
        Ok(())
    }

    pub fn replay(&self) -> impl Iterator<Item = WalEntry> {
        todo!("Implement file-based WAL replay here");
    }
}
