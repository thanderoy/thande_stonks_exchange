use common::{OrderId, Symbol};

#[test]
fn order_id_new_is_unique() {
    let a = OrderId::new();
    let b = OrderId::new();
    assert_ne!(a, b);
}

#[test]
fn order_id_default_is_unique() {
    let a = OrderId::default();
    let b = OrderId::default();
    assert_ne!(a, b);
}

#[test]
fn symbol_roundtrip() {
    let sym = Symbol::new("AAPL");
    assert_eq!(sym.as_str(), "AAPL");
    assert_eq!(sym.to_string(), "AAPL");
}

#[test]
fn symbol_equality() {
    assert_eq!(Symbol::new("MSFT"), Symbol::new("MSFT"));
    assert_ne!(Symbol::new("MSFT"), Symbol::new("AAPL"));
}
