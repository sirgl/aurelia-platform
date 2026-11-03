//! The span tree: nesting, exclusive time, calls, runtime switch, dynamic names.

use std::time::Duration;

fn busy(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

#[test]
fn nested_spans_split_inclusive_and_exclusive_time() {
    let _ = prof::drain();
    {
        let _outer = prof::span("outer");
        busy(6);
        for _ in 0..3 {
            let _inner = prof::span("inner");
            busy(4);
        }
    }
    let records = prof::drain();
    let outer = records.iter().find(|r| r.path == "outer").expect("outer");
    let inner = records
        .iter()
        .find(|r| r.path == "outer > inner")
        .expect("inner");
    assert_eq!((outer.calls, inner.calls), (1, 3));
    assert!(inner.incl_ns >= 12_000_000 && inner.excl_ns == inner.incl_ns);
    assert!(
        outer.incl_ns >= outer.excl_ns + inner.incl_ns - 1000,
        "{outer:?} {inner:?}"
    );
    assert!(outer.excl_ns >= 6_000_000);
    // A drain starts a fresh window.
    assert!(prof::drain().is_empty());
}

#[test]
fn the_same_name_under_two_callers_is_two_nodes() {
    let _ = prof::drain();
    {
        let _a = prof::span("a");
        let _x = prof::span("leaf");
    }
    {
        let _b = prof::span("b");
        let _x = prof::span("leaf");
    }
    let paths: Vec<String> = prof::drain().into_iter().map(|r| r.path).collect();
    assert!(paths.contains(&"a > leaf".to_string()) && paths.contains(&"b > leaf".to_string()));
}

#[test]
fn debug_names_and_the_runtime_switch() {
    let _ = prof::drain();
    {
        let _s = prof::span_debug("step/", &Some(3));
    }
    prof::set_active(false);
    {
        let _s = prof::span("off");
    }
    prof::set_active(true);
    let paths: Vec<String> = prof::drain().into_iter().map(|r| r.path).collect();
    assert_eq!(paths, vec!["step/Some(3)".to_string()]);
}

#[test]
fn a_thread_sees_only_its_own_spans() {
    let _ = prof::drain();
    std::thread::spawn(|| {
        let _s = prof::span("other thread");
        assert_eq!(prof::drain().len(), 0, "open spans have not booked yet");
    })
    .join()
    .unwrap();
    assert!(prof::drain().is_empty());
}
