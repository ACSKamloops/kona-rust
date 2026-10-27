#![forbid(unsafe_code)]

use kona::pool::{self, MIN_LEN};

// Each scenario gets fresh OnceLocks, shelves and a 1 MiB budget. Other tests
// cannot populate the pool or change the budget while an assertion is running.
fn isolated(name: &str, run: impl FnOnce()) {
    isolated_with_budget(name, "1", run);
}

fn isolated_with_budget(name: &str, budget: &str, run: impl FnOnce()) {
    if std::env::var("KONA_POOL_TEST").as_deref() == Ok(name) {
        run();
        return;
    }
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture"])
        .env("KONA_POOL_TEST", name)
        .env("KONA_POOL_MB", budget)
        .status().unwrap();
    assert!(status.success(), "isolated pool scenario {name} failed");
}

#[test]
fn counts_all_retained_capacity() {
    isolated("counts_all_retained_capacity", || {
        let mut values = vec![1.0; MIN_LEN * 2];
        values.truncate(MIN_LEN);
        let bytes = values.capacity() * size_of::<f64>();
        pool::give(values);
        let stats = pool::stats();
        assert_eq!(stats.retained_bytes, bytes);
        assert_eq!(stats.accounted_bytes, bytes);
    });
}

#[test]
fn budget_is_shared_between_numeric_types() {
    isolated("budget_is_shared_between_numeric_types", || {
        pool::give(vec![1.0; 70_000]);
        pool::give(vec![1_i64; 70_000]);
        let stats = pool::stats();
        assert!(stats.retained_bytes <= 1 << 20, "{stats:?}");
        assert_eq!(stats.retained_bytes, stats.accounted_bytes);
    });
}

#[test]
fn truncated_buffer_can_grow_without_reallocation() {
    isolated("truncated_buffer_can_grow_without_reallocation", || {
        let values = pool::take::<f64>(MIN_LEN * 2);
        let allocation = values.as_ptr();
        pool::give(values);
        let values = pool::take::<f64>(MIN_LEN);
        assert_eq!(values.as_ptr(), allocation);
        pool::give(values);
        let values = pool::take::<f64>(MIN_LEN * 2);
        assert_eq!(values.as_ptr(), allocation);
        assert_eq!(values.len(), MIN_LEN * 2);
        assert_eq!(pool::stats().retained_bytes, 0);
    });
}

#[test]
fn oversized_capacity_is_rejected_after_truncation() {
    isolated("oversized_capacity_is_rejected_after_truncation", || {
        let mut values = vec![1_i64; (1 << 20) / size_of::<i64>() + 1];
        values.truncate(MIN_LEN);
        pool::give(values);
        assert_eq!(pool::stats().retained_bytes, 0);
    });
}

#[test]
fn zero_budget_disables_retention() {
    isolated_with_budget("zero_budget_disables_retention", "0", || {
        pool::give(vec![1_i64; MIN_LEN]);
        pool::give(vec![1.0; MIN_LEN]);
        assert_eq!(pool::stats().retained_bytes, 0);
        assert_eq!(pool::stats().buffers, 0);
    });
}

#[test]
fn buffer_count_is_also_combined() {
    isolated_with_budget("buffer_count_is_also_combined", "4", || {
        for _ in 0..25 {
            pool::give(vec![1_i64; MIN_LEN]);
            pool::give(vec![1.0; MIN_LEN]);
        }
        assert_eq!(pool::stats().buffers, 48);
        assert_eq!(pool::stats().retained_bytes, 48 * MIN_LEN * 8);
    });
}

#[test]
fn concurrent_transfers_keep_the_combined_budget() {
    isolated("concurrent_transfers_keep_the_combined_budget", || {
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    for i in 0..100 {
                        let n = MIN_LEN * (1 + i % 4);
                        let mut floats = pool::take::<f64>(n);
                        let mut integers = pool::take::<i64>(n);
                        floats.fill(1.5);
                        integers.fill(2);
                        pool::give(floats);
                        pool::give(integers);
                    }
                });
            }
        });
        let stats = pool::stats();
        assert_eq!(stats.accounted_bytes, stats.retained_bytes);
        assert!(stats.retained_bytes <= 1 << 20);
        assert!(stats.buffers <= 48);
    });
}
