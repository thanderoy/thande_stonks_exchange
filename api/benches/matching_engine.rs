use criterion::{Criterion, criterion_group, criterion_main};

fn bench_matching(c: &mut Criterion) {
    c.bench_function("match 1000 orders", |b| {
        b.iter(|| {
            let mut book = OrderBook::new(Symbol::from("AAPL"));
            for i in 0..1000 {
                let order = random_order();
                match_order(&mut book, order);
            }
        })
    });
}
