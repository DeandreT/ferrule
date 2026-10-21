//! Host cancellation, kept independent of tracing and debugging.
//!
//! Runners check a host flag at acquisition and publication boundaries. During
//! evaluation the engine's only cooperative control points belong to its debug
//! hook protocol, so a runner wraps the host's optional debug hook in
//! [`CancellableHook`]. The wrapper forwards only the callbacks the host hook
//! requested: requesting cancellation never invokes a host observer, and a
//! cancelled run stops at the next node, input, or target-write control point.
//! Cooperative cancellation cannot interrupt a synchronous parser or request.

use std::sync::atomic::{AtomicBool, Ordering};

use engine::{DebugDecision, DebugHook};

pub(crate) fn check_cancelled(cancellation: Option<&AtomicBool>) -> anyhow::Result<()> {
    if is_cancelled(cancellation) {
        return Err(engine::EngineError::DebugCancelled.into());
    }
    Ok(())
}

fn is_cancelled(cancellation: Option<&AtomicBool>) -> bool {
    cancellation.is_some_and(|flag| flag.load(Ordering::Acquire))
}

/// A debug hook that observes nothing; it lets cancellation use the engine's
/// control points when the host supplied no debug hook of its own.
pub(crate) struct NoDebugHook;

impl DebugHook for NoDebugHook {
    fn before_target_write(&self, _: &engine::PendingTargetWrite) -> DebugDecision {
        DebugDecision::Resume
    }

    fn wants_node_values(&self) -> bool {
        false
    }

    fn wants_node_inputs(&self) -> bool {
        false
    }
}

/// Adds host cancellation to a borrowed debug hook without changing which
/// callbacks reach it.
pub(crate) struct CancellableHook<'a, H: DebugHook + ?Sized> {
    pub(crate) inner: &'a H,
    pub(crate) cancellation: Option<&'a AtomicBool>,
}

impl<H: DebugHook + ?Sized> CancellableHook<'_, H> {
    fn cancelled(&self) -> bool {
        is_cancelled(self.cancellation)
    }

    fn control(&self, callback: impl FnOnce() -> DebugDecision) -> DebugDecision {
        if self.cancelled() {
            return DebugDecision::Cancel;
        }
        let decision = callback();
        if self.cancelled() {
            DebugDecision::Cancel
        } else {
            decision
        }
    }
}

impl<H: DebugHook + ?Sized> DebugHook for CancellableHook<'_, H> {
    // Value and input control points are requested once cancellation is
    // pending, so a run without target writes still stops promptly. `control`
    // refuses before the inner hook sees a callback it did not request.
    fn wants_node_values(&self) -> bool {
        self.inner.wants_node_values() || self.cancelled()
    }

    fn wants_function_node_values(&self) -> bool {
        self.inner.wants_function_node_values() || self.cancelled()
    }

    fn wants_node_inputs(&self) -> bool {
        self.inner.wants_node_inputs() || self.cancelled()
    }

    fn wants_function_node_inputs(&self) -> bool {
        self.inner.wants_function_node_inputs() || self.cancelled()
    }

    fn wants_node_failures(&self) -> bool {
        self.inner.wants_node_failures()
    }

    fn wants_function_node_failures(&self) -> bool {
        self.inner.wants_function_node_failures()
    }

    fn wants_owned_context_for_node_failure(&self, failure: &engine::PendingNodeFailure) -> bool {
        self.inner.wants_owned_context_for_node_failure(failure)
    }

    fn source_field_probe(&self) -> Option<(usize, String)> {
        self.inner.source_field_probe()
    }

    fn before_target_write(&self, value: &engine::PendingTargetWrite) -> DebugDecision {
        self.control(|| self.inner.before_target_write(value))
    }

    fn after_node_value(&self, value: &engine::PendingNodeValue) -> DebugDecision {
        self.control(|| self.inner.after_node_value(value))
    }

    fn after_function_node_value(&self, value: &engine::PendingFunctionNodeValue) -> DebugDecision {
        self.control(|| self.inner.after_function_node_value(value))
    }

    fn after_node_input(&self, value: &engine::PendingNodeInput) -> DebugDecision {
        self.control(|| self.inner.after_node_input(value))
    }

    fn after_function_node_input(&self, value: &engine::PendingFunctionNodeInput) -> DebugDecision {
        self.control(|| self.inner.after_function_node_input(value))
    }

    fn after_node_failure(&self, value: &engine::PendingNodeFailure) -> DebugDecision {
        self.control(|| self.inner.after_node_failure(value))
    }

    fn after_node_failure_with_owned_context(
        &self,
        failure: &engine::PendingNodeFailure,
        error: &engine::EngineError,
        context: Result<engine::TraceOwnedWitness<'_>, &engine::TraceOwnedContextError>,
    ) -> DebugDecision {
        self.control(|| {
            self.inner
                .after_node_failure_with_owned_context(failure, error, context)
        })
    }

    fn after_function_node_failure(
        &self,
        value: &engine::PendingFunctionNodeFailure,
    ) -> DebugDecision {
        self.control(|| self.inner.after_function_node_failure(value))
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn pending_cancellation_requests_control_points_without_calling_the_host_hook() {
        let flag = AtomicBool::new(false);
        let hook = CancellableHook {
            inner: &NoDebugHook,
            cancellation: Some(&flag),
        };
        assert!(!hook.wants_node_values());
        assert!(!hook.wants_function_node_values());
        assert!(!hook.wants_node_inputs());
        assert!(!hook.wants_function_node_inputs());
        let called = Cell::new(0);
        let resume = || {
            called.set(called.get() + 1);
            DebugDecision::Resume
        };
        assert_eq!(hook.control(resume), DebugDecision::Resume);

        flag.store(true, Ordering::Release);
        assert!(hook.wants_node_values());
        assert!(hook.wants_function_node_values());
        assert!(hook.wants_node_inputs());
        assert!(hook.wants_function_node_inputs());
        assert!(!hook.wants_node_failures());
        assert_eq!(hook.control(resume), DebugDecision::Cancel);
        assert_eq!(called.get(), 1, "a cancelled run never calls the host hook");
        assert!(matches!(
            check_cancelled(Some(&flag))
                .unwrap_err()
                .downcast_ref::<engine::EngineError>(),
            Some(engine::EngineError::DebugCancelled)
        ));
    }

    #[test]
    fn cancellation_requested_during_a_callback_overrides_its_decision() {
        let flag = AtomicBool::new(false);
        let hook = CancellableHook {
            inner: &NoDebugHook,
            cancellation: Some(&flag),
        };
        let decision = hook.control(|| {
            flag.store(true, Ordering::Release);
            DebugDecision::Resume
        });
        assert_eq!(decision, DebugDecision::Cancel);
        assert!(check_cancelled(None).is_ok());
    }
}
