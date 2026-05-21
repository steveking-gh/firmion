// Utility to help manage recursive descent depth.

// Don't clutter upstream docs.rs for an otherwise private library.
#![doc(hidden)]

use std::cell::Cell;
use std::marker::PhantomData;

/// Uniform recursion depth limit shared across all libraries. This is a
/// Somewhat arbitrary limit large enough for practical use.
pub const MAX_RECURSION_DEPTH: usize = 200;

thread_local! {
    static DEPTH: Cell<usize> = const { Cell::new(0) };
}

/// RAII guard for recursive descent depth tracking.
///
/// A thread-local counter tracks total recursion depth across all libraries
/// in the call chain.  Any library that calls `DepthGuard::enter` contributes
/// to the same counter, giving a single confident bound on stack usage
/// regardless of which library is currently executing.
///
/// # Example
///
/// ```
/// use firmion_diags::depth_guard::{DepthGuard, MAX_RECURSION_DEPTH};
///
/// fn parse_expr() -> bool {
///     let Some(_guard) = DepthGuard::enter(MAX_RECURSION_DEPTH) else {
///         return false; // depth exceeded
///     };
///     // recursive calls here -- _guard decrements on any return
///     true
/// }
/// ```
pub struct DepthGuard {
    /// Depth at the moment this guard was created.
    created_at_depth: usize,
    /// Opt out of Send and Sync: the guard must be dropped on the same thread
    /// that created it, since the counter is thread-local.
    _marker: PhantomData<*mut ()>,
}

impl DepthGuard {
    /// Attempt to descend one level.
    ///
    /// Increments the thread-local counter and returns `Some(guard)` if the
    /// new depth is within `limit`.  Returns `None` without modifying the
    /// counter if the limit would be exceeded.  The counter decrements
    /// automatically when the returned guard is dropped.
    pub fn enter(limit: usize) -> Option<Self> {
        DEPTH.with(|d| {
            let current = d.get();
            if current >= limit {
                None
            } else {
                let new_depth = current + 1;
                d.set(new_depth);
                Some(DepthGuard {
                    created_at_depth: new_depth,
                    _marker: PhantomData,
                })
            }
        })
    }

    /// Depth at the moment this guard was created.
    pub fn depth(&self) -> usize {
        self.created_at_depth
    }

    /// Current recursion depth on this thread.
    pub fn current() -> usize {
        DEPTH.with(|d| d.get())
    }

    /// Explicitly ascend.  Equivalent to dropping the guard; provided for
    /// call sites that prefer a named exit over a silent drop.
    pub fn exit(self) {}
}

impl Drop for DepthGuard {
    fn drop(&mut self) {
        // Depth was incremented on enter, so it is always >= 1 here.
        DEPTH.with(|d| d.set(d.get() - 1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_increments_and_drop_decrements() {
        {
            let guard = DepthGuard::enter(10).unwrap();
            assert_eq!(guard.depth(), 1);
            assert_eq!(DepthGuard::current(), 1);
        }
        assert_eq!(DepthGuard::current(), 0);
    }

    #[test]
    fn nested_depth_tracks_correctly() {
        let g1 = DepthGuard::enter(10).unwrap();
        assert_eq!(g1.depth(), 1);
        let g2 = DepthGuard::enter(10).unwrap();
        assert_eq!(g2.depth(), 2);
        let g3 = DepthGuard::enter(10).unwrap();
        assert_eq!(g3.depth(), 3);
        drop(g3);
        assert_eq!(DepthGuard::current(), 2);
        drop(g2);
        assert_eq!(DepthGuard::current(), 1);
        drop(g1);
        assert_eq!(DepthGuard::current(), 0);
    }

    #[test]
    fn enter_returns_none_at_limit() {
        let _g1 = DepthGuard::enter(2).unwrap();
        let _g2 = DepthGuard::enter(2).unwrap();
        // depth is now 2, at limit -- next enter must fail
        assert!(DepthGuard::enter(2).is_none());
        // counter must be unchanged after a failed enter
        assert_eq!(DepthGuard::current(), 2);
    }

    #[test]
    fn exit_decrements() {
        let guard = DepthGuard::enter(10).unwrap();
        assert_eq!(DepthGuard::current(), 1);
        guard.exit();
        assert_eq!(DepthGuard::current(), 0);
    }

    #[test]
    fn limit_zero_rejects_immediately() {
        assert!(DepthGuard::enter(0).is_none());
        assert_eq!(DepthGuard::current(), 0);
    }

    #[test]
    fn cross_library_depth_is_cumulative() {
        // Simulate two independent libraries both using DepthGuard.
        // Neither library sees the other's depth explicitly, but both
        // contribute to the same counter, so the combined limit holds.
        fn lib_a() -> Option<DepthGuard> { DepthGuard::enter(3) }
        fn lib_b() -> Option<DepthGuard> { DepthGuard::enter(3) }

        let _a = lib_a().unwrap();  // depth 1
        let _b = lib_b().unwrap();  // depth 2
        let _c = lib_a().unwrap();  // depth 3
        assert!(lib_b().is_none()); // depth 4 > limit 3
        assert_eq!(DepthGuard::current(), 3);
    }

    #[test]
    fn depth_snapshot_matches_creation_depth() {
        let g1 = DepthGuard::enter(10).unwrap();
        let g2 = DepthGuard::enter(10).unwrap();
        // Each guard remembers its own creation depth.
        assert_eq!(g1.depth(), 1);
        assert_eq!(g2.depth(), 2);
    }
}
