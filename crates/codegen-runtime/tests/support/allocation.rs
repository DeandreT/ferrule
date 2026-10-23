//! A counting global allocator for per-call endpoint cost attribution.
//!
//! Allocations are recorded on the measuring thread. Each measured row
//! reports the mean allocation count, allocated bytes, and peak live bytes
//! above the starting point for one call, plus mean wall-clock time.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::hint::black_box;
use std::time::Instant;

struct Counting;

thread_local! {
    static ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
    static ALLOCATED: Cell<u64> = const { Cell::new(0) };
    static LIVE: Cell<i64> = const { Cell::new(0) };
    static PEAK: Cell<i64> = const { Cell::new(0) };
}

fn record_allocation(bytes: usize) {
    let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
    let _ = ALLOCATED.try_with(|total| total.set(total.get() + bytes as u64));
    adjust_live(bytes as i64);
}

fn adjust_live(delta: i64) {
    let _ = LIVE.try_with(|live| {
        let next = live.get() + delta;
        live.set(next);
        let _ = PEAK.try_with(|peak| peak.set(peak.get().max(next)));
    });
}

// SAFETY: every operation delegates to the system allocator unchanged; the
// thread-local counters never allocate.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record_allocation(layout.size());
        // SAFETY: forwarded with the caller's layout.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record_allocation(layout.size());
        // SAFETY: forwarded with the caller's layout.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        adjust_live(-(layout.size() as i64));
        // SAFETY: forwarded with the caller's pointer and layout.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        record_allocation(new_size);
        adjust_live(-(layout.size() as i64));
        // SAFETY: forwarded with the caller's pointer, layout, and size.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

#[derive(Clone, Copy, Debug)]
pub struct Cost {
    pub allocations: u64,
    pub allocated: u64,
    pub peak: i64,
    pub micros: f64,
}

/// Mean cost of one call after one unmeasured warm-up call.
pub fn measure<T>(iterations: u32, mut call: impl FnMut() -> T) -> Cost {
    drop(black_box(call()));
    let mut allocations = 0;
    let mut allocated = 0;
    let mut peak = 0;
    let started = Instant::now();
    for _ in 0..iterations {
        let (count, total, live) = (
            ALLOCATIONS.with(Cell::get),
            ALLOCATED.with(Cell::get),
            LIVE.with(Cell::get),
        );
        PEAK.with(|value| value.set(live));
        drop(black_box(call()));
        allocations += ALLOCATIONS.with(Cell::get) - count;
        allocated += ALLOCATED.with(Cell::get) - total;
        peak = peak.max(PEAK.with(Cell::get) - live);
    }
    let micros = started.elapsed().as_secs_f64() * 1e6 / f64::from(iterations);
    Cost {
        allocations: allocations / u64::from(iterations),
        allocated: allocated / u64::from(iterations),
        peak,
        micros,
    }
}

/// Live bytes on the measuring thread that `make` leaves allocated.
pub fn retained<T>(make: impl FnOnce() -> T) -> (T, i64) {
    let live = LIVE.with(Cell::get);
    let value = make();
    (value, LIVE.with(Cell::get) - live)
}

pub fn header() {
    println!("| Operation | Allocations | Allocated bytes | Peak live bytes | Mean µs |");
    println!("| --- | ---: | ---: | ---: | ---: |");
}

pub fn row(label: &str, cost: Cost) {
    println!(
        "| {label} | {} | {} | {} | {:.1} |",
        cost.allocations, cost.allocated, cost.peak, cost.micros
    );
}
