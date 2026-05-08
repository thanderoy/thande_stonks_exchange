use persistence::wal::{WalEntry, WriteAheadLog};
use tempfile::NamedTempFile;
use uuid::Uuid;

fn tmp_path(file: &NamedTempFile) -> &str {
    file.path().to_str().unwrap()
}

fn order_submitted(symbol: &str) -> WalEntry {
    WalEntry::OrderSubmitted {
        order_id: Uuid::new_v4(),
        symbol: symbol.to_string(),
        side: "buy".to_string(),
        quantity: "10".to_string(),
        price: Some("100.00".to_string()),
        client_id: "client1".to_string(),
    }
}

#[test]
fn replay_empty_file_returns_empty_vec() {
    let tmp = NamedTempFile::new().unwrap();
    let entries = WriteAheadLog::replay(tmp_path(&tmp)).unwrap();
    assert!(entries.is_empty());
}

#[test]
fn replay_nonexistent_path_returns_empty_vec() {
    let entries = WriteAheadLog::replay("/tmp/wal_does_not_exist_xyz.log").unwrap();
    assert!(entries.is_empty());
}

#[test]
fn append_and_replay_single_entry() {
    let tmp = NamedTempFile::new().unwrap();
    let entry = order_submitted("AAPL");
    {
        let mut wal = WriteAheadLog::open(tmp_path(&tmp)).unwrap();
        wal.append(&entry).unwrap();
    }
    let replayed = WriteAheadLog::replay(tmp_path(&tmp)).unwrap();
    assert_eq!(replayed.len(), 1);
    assert_eq!(replayed[0], entry);
}

#[test]
fn append_and_replay_multiple_entries_in_order() {
    let tmp = NamedTempFile::new().unwrap();
    let entries = vec![
        order_submitted("AAPL"),
        WalEntry::OrderCancelled { order_id: Uuid::new_v4(), timestamp: "2024-01-01T00:00:00Z".to_string() },
        WalEntry::TradeExecuted {
            trade_id: Uuid::new_v4(),
            symbol: "AAPL".to_string(),
            price: "100.00".to_string(),
            quantity: "5".to_string(),
            buyer_order_id: Uuid::new_v4(),
            seller_order_id: Uuid::new_v4(),
            executed_at: "2024-01-01T00:00:01Z".to_string(),
        },
        WalEntry::SessionStateChanged { symbol: "AAPL".to_string(), state: "open".to_string() },
    ];
    {
        let mut wal = WriteAheadLog::open(tmp_path(&tmp)).unwrap();
        for e in &entries {
            wal.append(e).unwrap();
        }
    }
    assert_eq!(WriteAheadLog::replay(tmp_path(&tmp)).unwrap(), entries);
}

#[test]
fn append_across_separate_opens_accumulates_entries() {
    let tmp = NamedTempFile::new().unwrap();
    {
        let mut wal = WriteAheadLog::open(tmp_path(&tmp)).unwrap();
        wal.append(&order_submitted("AAPL")).unwrap();
    }
    {
        let mut wal = WriteAheadLog::open(tmp_path(&tmp)).unwrap();
        wal.append(&order_submitted("MSFT")).unwrap();
    }
    assert_eq!(WriteAheadLog::replay(tmp_path(&tmp)).unwrap().len(), 2);
}

#[test]
fn all_entry_variants_roundtrip() {
    let tmp = NamedTempFile::new().unwrap();
    let variants = vec![
        WalEntry::OrderSubmitted {
            order_id: Uuid::new_v4(),
            symbol: "X".to_string(),
            side: "sell".to_string(),
            quantity: "1".to_string(),
            price: None,
            client_id: "c".to_string(),
        },
        WalEntry::OrderCancelled { order_id: Uuid::new_v4(), timestamp: "t".to_string() },
        WalEntry::TradeExecuted {
            trade_id: Uuid::new_v4(),
            symbol: "X".to_string(),
            price: "1".to_string(),
            quantity: "1".to_string(),
            buyer_order_id: Uuid::new_v4(),
            seller_order_id: Uuid::new_v4(),
            executed_at: "t".to_string(),
        },
        WalEntry::SessionStateChanged { symbol: "X".to_string(), state: "closed".to_string() },
    ];
    {
        let mut wal = WriteAheadLog::open(tmp_path(&tmp)).unwrap();
        for v in &variants {
            wal.append(v).unwrap();
        }
    }
    assert_eq!(WriteAheadLog::replay(tmp_path(&tmp)).unwrap(), variants);
}
