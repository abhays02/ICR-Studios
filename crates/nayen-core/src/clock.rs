//! The engine's clock.
//!
//! Injectable because `std::time::Instant` is a stub on `wasm32-unknown-unknown`:
//! it compiles, never panics, and every `elapsed()` is zero. A page that reports
//! `recognize 0.0 ms` reads as "instant", which is a far worse lie than reporting
//! nothing — so on WASM the host installs a real clock and, until it does, the
//! default returns `f64::NAN`, which `serde_json` renders as `null`.
//!
//! Native builds get a monotonic clock with no setup, so the CLI never has to
//! think about this.

use std::cell::Cell;

/// A wall-clock reading in milliseconds. Only differences are meaningful.
pub type Clock = fn() -> f64;

thread_local! {
    static CLOCK: Cell<Clock> = const { Cell::new(default_clock) };
    #[cfg(not(target_arch = "wasm32"))]
    static ORIGIN: std::time::Instant = std::time::Instant::now();
}

/// Replace the clock. A host does this once, before its first page.
pub fn set_clock(clock: Clock) {
    CLOCK.with(|slot| slot.set(clock));
}

/// Current reading from the installed clock.
pub fn now_ms() -> f64 {
    CLOCK.with(|slot| slot.get())()
}

/// True when no host clock was installed and timings are therefore meaningless.
pub fn is_unmeasured() -> bool {
    now_ms().is_nan()
}

#[cfg(not(target_arch = "wasm32"))]
fn default_clock() -> f64 {
    ORIGIN.with(|origin| origin.elapsed().as_secs_f64() * 1000.0)
}

#[cfg(target_arch = "wasm32")]
fn default_clock() -> f64 {
    f64::NAN
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_native_clock_advances() {
        if cfg!(target_arch = "wasm32") {
            assert!(now_ms().is_nan(), "no host clock installed");
            return;
        }
        let a = now_ms();
        let mut acc = 0u64;
        for i in 0..200_000u64 {
            acc = acc.wrapping_add(i);
        }
        std::hint::black_box(acc);
        let b = now_ms();
        assert!(b > a, "clock went backwards or never moved: {a} -> {b}");
    }

    #[test]
    fn an_installed_clock_replaces_the_default() {
        fn fixed() -> f64 {
            42.0
        }
        let previous = CLOCK.with(|slot| slot.get());
        set_clock(fixed);
        assert_eq!(now_ms(), 42.0);
        assert!(!is_unmeasured());
        set_clock(previous);
    }

    #[test]
    fn nan_serializes_as_null_not_as_zero() {
        // The whole point of the NAN default: a missing clock must not look like
        // a fast one.
        fn no_clock() -> f64 {
            f64::NAN
        }
        let previous = CLOCK.with(|slot| slot.get());
        set_clock(no_clock);
        assert!(is_unmeasured());
        assert_eq!(serde_json::to_string(&now_ms()).unwrap(), "null");
        set_clock(previous);
    }
}
