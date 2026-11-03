//! The thread CPU clock: spans on it count work, not waiting. Its own test binary, because
//! the clock choice is process-wide.

use std::time::Duration;

#[cfg(unix)]
#[test]
fn a_sleeping_span_costs_no_cpu_time_on_the_cpu_clock() {
    assert!(prof::set_cpu_clock(true));
    let _ = prof::drain();
    {
        let _sleep = prof::span("sleep");
        std::thread::sleep(Duration::from_millis(30));
    }
    {
        let _work = prof::span("work");
        // Spin until the CPU clock itself has moved 20 ms: on a busy machine a wall-clock
        // deadline would give the thread far less than that.
        let start = prof::now_ns();
        let mut x = 1u64;
        while prof::now_ns() < start + 20_000_000 {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
        }
        std::hint::black_box(x);
    }
    let records = prof::drain();
    let get = |name: &str| records.iter().find(|r| r.path == name).expect(name).incl_ns;
    assert!(
        get("sleep") < 5_000_000,
        "sleep booked {} ns of CPU",
        get("sleep")
    );
    assert!(
        get("work") > 15_000_000,
        "work booked {} ns of CPU",
        get("work")
    );
    assert!(!prof::set_cpu_clock(false));
}

#[cfg(not(unix))]
#[test]
fn without_a_cpu_clock_the_wall_clock_stays() {
    assert!(!prof::set_cpu_clock(true));
}
