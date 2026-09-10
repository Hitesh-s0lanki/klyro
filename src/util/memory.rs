//! A global allocator that keeps a running byte total, so INFO can
//! report real memory use instead of estimating it by walking the
//! keyspace.
//!
//! The counters are the only cost: one relaxed atomic add and one
//! relaxed subtract per allocation. Relaxed is enough because nothing
//! orders other memory against these values - they are read only to be
//! printed.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

static ALLOCATED: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

pub struct CountingAllocator;

fn record_growth(bytes: usize) {
    let total = ALLOCATED.fetch_add(bytes, Ordering::Relaxed) + bytes;
    PEAK.fetch_max(total, Ordering::Relaxed);
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            record_growth(layout.size());
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            record_growth(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        ALLOCATED.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) };
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() {
            // Add before subtracting: the total is unsigned, and a
            // shrink that subtracted first could briefly underflow.
            record_growth(new_size);
            ALLOCATED.fetch_sub(layout.size(), Ordering::Relaxed);
        }
        new_ptr
    }
}

/// Bytes currently held by live allocations.
pub fn used_bytes() -> usize {
    ALLOCATED.load(Ordering::Relaxed)
}

/// The high-water mark of [`used_bytes`] since startup.
pub fn peak_bytes() -> usize {
    PEAK.load(Ordering::Relaxed)
}

/// Renders a byte count the way Redis's INFO does, e.g. `1.51M`.
pub fn human_bytes(bytes: usize) -> String {
    const UNITS: [(f64, &str); 3] = [
        (1024.0 * 1024.0 * 1024.0, "G"),
        (1024.0 * 1024.0, "M"),
        (1024.0, "K"),
    ];
    let value = bytes as f64;
    for (scale, suffix) in UNITS {
        if value >= scale {
            return format!("{:.2}{}", value / scale, suffix);
        }
    }
    format!("{}B", bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The counter is process-wide and `cargo test` runs test threads in
    // parallel, so another thread's allocations and frees land in these
    // readings too. The assertions are therefore directional with a
    // wide margin, not exact: an exact delta would flake.
    #[test]
    fn allocating_moves_the_counter() {
        const SIZE: usize = 8 * 1024 * 1024;
        let before = used_bytes();
        let held: Vec<u8> = vec![0; SIZE];
        let during = used_bytes();
        assert!(
            during > before + SIZE / 2,
            "expected a large jump, went {before} -> {during}"
        );

        drop(held);
        let after = used_bytes();
        assert!(
            after < during - SIZE / 2,
            "expected the free to be counted, went {during} -> {after}"
        );
        assert!(peak_bytes() >= during);
    }

    #[test]
    fn human_bytes_picks_a_unit() {
        assert_eq!(human_bytes(512), "512B");
        assert_eq!(human_bytes(2048), "2.00K");
        assert_eq!(human_bytes(3 * 1024 * 1024), "3.00M");
        assert_eq!(human_bytes(2 * 1024 * 1024 * 1024), "2.00G");
    }
}
