use common::Symbol;
use engine::order::{Order, Side, TimeInForce};
use engine::session::SessionState;
use persistence::wal::{WalEntry, WriteAheadLog};
use rust_decimal_macros::dec;
use std::io::Write;
use tempfile::NamedTempFile;
use uuid::Uuid;

fn tmp_path(file: &NamedTempFile) -> &str {
    file.path().to_str().unwrap()
}

fn order_submitted(symbol: &str) -> WalEntry {
    WalEntry::OrderSubmitted {
        order: Order::new_limit(
            Symbol::new(symbol),
            Side::Buy,
            dec!(100.00),
            dec!(10),
            TimeInForce::GoodTillCanceled,
            "client1",
        ),
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
        WalEntry::OrderCancelled {
            order_id: Uuid::new_v4(),
            symbol: "AAPL".to_string(),
            side: Side::Buy,
            timestamp: "2024-01-01T00:00:00Z".to_string(),
        },
        WalEntry::TradeExecuted {
            trade_id: Uuid::new_v4(),
            symbol: "AAPL".to_string(),
            price: "100.00".to_string(),
            quantity: "5".to_string(),
            buyer_order_id: Uuid::new_v4(),
            seller_order_id: Uuid::new_v4(),
            executed_at: "2024-01-01T00:00:01Z".to_string(),
        },
        WalEntry::SessionStateChanged {
            symbol: "AAPL".to_string(),
            state: SessionState::Open,
        },
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
            order: Order::new_market(Symbol::new("X"), Side::Sell, dec!(1), "c"),
        },
        WalEntry::OrderCancelled {
            order_id: Uuid::new_v4(),
            symbol: "X".to_string(),
            side: Side::Sell,
            timestamp: "t".to_string(),
        },
        WalEntry::TradeExecuted {
            trade_id: Uuid::new_v4(),
            symbol: "X".to_string(),
            price: "1".to_string(),
            quantity: "1".to_string(),
            buyer_order_id: Uuid::new_v4(),
            seller_order_id: Uuid::new_v4(),
            executed_at: "t".to_string(),
        },
        WalEntry::SessionStateChanged {
            symbol: "X".to_string(),
            state: SessionState::Closed,
        },
        participant_registered(),
    ];
    {
        let mut wal = WriteAheadLog::open(tmp_path(&tmp)).unwrap();
        for v in &variants {
            wal.append(v).unwrap();
        }
    }
    assert_eq!(WriteAheadLog::replay(tmp_path(&tmp)).unwrap(), variants);
}

fn participant_registered() -> WalEntry {
    WalEntry::ParticipantRegistered {
        client_id: "g-abc123xyz0".to_string(),
        display_name: "Test Guest".to_string(),
        max_notional_per_order: dec!(100000.50),
        max_position: dec!(10000),
        max_orders_per_second: 5,
        registered_at: None,
    }
}

#[test]
fn participant_registered_roundtrips_with_exact_decimals() {
    let tmp = NamedTempFile::new().unwrap();
    let entry = participant_registered();
    WriteAheadLog::open(tmp_path(&tmp))
        .unwrap()
        .append(&entry)
        .unwrap();
    let replayed = WriteAheadLog::replay(tmp_path(&tmp)).unwrap();
    assert_eq!(replayed, vec![entry]);
    match &replayed[0] {
        WalEntry::ParticipantRegistered {
            max_notional_per_order,
            ..
        } => assert_eq!(max_notional_per_order.to_string(), "100000.50"),
        other => panic!("unexpected entry {other:?}"),
    }
}

/// Simulates a crash mid-append: a length prefix promising more bytes than were written.
fn append_torn_record(path: &str) {
    let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
    file.write_all(&100u32.to_le_bytes()).unwrap();
    file.write_all(b"{\"type\":").unwrap();
}

#[test]
fn replay_ignores_torn_final_record() {
    let tmp = NamedTempFile::new().unwrap();
    let entry = order_submitted("AAPL");
    WriteAheadLog::open(tmp_path(&tmp))
        .unwrap()
        .append(&entry)
        .unwrap();
    append_torn_record(tmp_path(&tmp));

    assert_eq!(WriteAheadLog::replay(tmp_path(&tmp)).unwrap(), vec![entry]);
}

#[test]
fn open_for_recovery_truncates_torn_tail_so_new_appends_replay() {
    let tmp = NamedTempFile::new().unwrap();
    let first = order_submitted("AAPL");
    WriteAheadLog::open(tmp_path(&tmp))
        .unwrap()
        .append(&first)
        .unwrap();
    append_torn_record(tmp_path(&tmp));

    let second = order_submitted("MSFT");
    {
        let (mut wal, recovered) = WriteAheadLog::open_for_recovery(tmp_path(&tmp)).unwrap();
        assert_eq!(recovered, vec![first.clone()]);
        wal.append(&second).unwrap();
    }
    assert_eq!(
        WriteAheadLog::replay(tmp_path(&tmp)).unwrap(),
        vec![first, second]
    );
}

#[test]
fn open_for_recovery_creates_missing_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("new.wal");
    let (_, entries) = WriteAheadLog::open_for_recovery(path.to_str().unwrap()).unwrap();
    assert!(entries.is_empty());
    assert!(path.exists());
}

#[test]
fn participant_entries_without_registered_at_still_parse() {
    let json = r#"{"type":"participant_registered","client_id":"g-old","display_name":"Old",
        "max_notional_per_order":"1","max_position":"1","max_orders_per_second":1}"#;
    let entry: WalEntry = serde_json::from_str(json).unwrap();
    assert!(matches!(
        entry,
        WalEntry::ParticipantRegistered {
            registered_at: None,
            ..
        }
    ));
}

#[test]
fn rotate_starts_a_new_log_and_keeps_the_old_one() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("exchange.wal");
    let path = path.to_str().unwrap();

    let mut wal = WriteAheadLog::open(path).unwrap();
    wal.append(&order_submitted("AAPL")).unwrap();
    wal.append(&order_submitted("MSFT")).unwrap();

    wal.rotate(&[participant_registered()]).unwrap();
    wal.append(&order_submitted("GOOGL")).unwrap();

    let current = WriteAheadLog::replay(path).unwrap();
    assert_eq!(current.len(), 2);
    assert_eq!(current[0], participant_registered());
    assert!(
        matches!(&current[1], WalEntry::OrderSubmitted { order } if order.symbol.as_str() == "GOOGL")
    );

    let previous = WriteAheadLog::replay(&format!("{path}.prev")).unwrap();
    assert_eq!(previous.len(), 2);
    assert!(!std::path::Path::new(&format!("{path}.next")).exists());

    // A second rotation replaces the older .prev.
    wal.rotate(&[]).unwrap();
    assert_eq!(WriteAheadLog::replay(path).unwrap().len(), 0);
    assert_eq!(
        WriteAheadLog::replay(&format!("{path}.prev"))
            .unwrap()
            .len(),
        2
    );
}
