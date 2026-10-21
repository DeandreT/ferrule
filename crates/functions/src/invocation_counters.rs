//! Explicit mutable state for one invocation, not a scalar builtin or global.

use std::collections::HashMap;

/// Logical storage limits chosen by the invocation owner. Zero admits no keys
/// or no retained text respectively; no default silently selects a policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvocationCounterLimits {
    pub max_counters: usize,
    /// Combined UTF-8 bytes of retained keys and present restart tokens.
    pub max_text_bytes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InvocationCounterError {
    #[error("invocation counter count exceeds {maximum}")]
    CountLimit { maximum: usize },
    #[error("invocation counter text exceeds {maximum} UTF-8 bytes")]
    TextLimit { maximum: usize },
    #[error("invocation counter text size overflow")]
    SizeOverflow,
    #[error("invocation counter allocation failed")]
    Allocation,
}

struct Counter {
    value: i64,
    restart: Option<String>,
}

/// A caller-owned counter table. Reuse it across documents of one invocation;
/// construct another table for another invocation. It is deliberately not Clone
/// or serializable and is not attached to immutable execution parameters.
pub struct InvocationCounters {
    limits: InvocationCounterLimits,
    counters: HashMap<String, Counter>,
    text_bytes: usize,
}

impl InvocationCounters {
    pub fn new(limits: InvocationCounterLimits) -> Self {
        Self {
            limits,
            counters: HashMap::new(),
            text_bytes: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.counters.len()
    }

    pub fn is_empty(&self) -> bool {
        self.counters.is_empty()
    }

    pub fn retained_text_bytes(&self) -> usize {
        self.text_bytes
    }

    /// Return start on a new key or a changed restart token. Otherwise add this
    /// call's step with signed 64-bit wrapping; a changed start alone is ignored.
    /// None and Some("") are distinct. Limits refuse without changing any counter.
    pub fn next(
        &mut self,
        key: &str,
        start: i64,
        step: i64,
        restart: Option<&str>,
    ) -> Result<i64, InvocationCounterError> {
        if let Some(counter) = self.counters.get_mut(key)
            && counter.restart.as_deref() == restart
        {
            counter.value = counter.value.wrapping_add(step);
            return Ok(counter.value);
        }

        let old = self.counters.get(key);
        if old.is_none() && self.counters.len() >= self.limits.max_counters {
            return Err(InvocationCounterError::CountLimit {
                maximum: self.limits.max_counters,
            });
        }
        let previous_restart_bytes = old.and_then(|c| c.restart.as_ref()).map_or(0, String::len);
        let new_key_bytes = if old.is_none() { key.len() } else { 0 };
        let text_bytes = self
            .text_bytes
            .checked_sub(previous_restart_bytes)
            .and_then(|bytes| bytes.checked_add(new_key_bytes))
            .and_then(|bytes| bytes.checked_add(restart.map_or(0, str::len)))
            .ok_or(InvocationCounterError::SizeOverflow)?;
        if text_bytes > self.limits.max_text_bytes {
            return Err(InvocationCounterError::TextLimit {
                maximum: self.limits.max_text_bytes,
            });
        }
        let restart = restart.map(copy_text).transpose()?;
        if let Some(counter) = self.counters.get_mut(key) {
            counter.value = start;
            counter.restart = restart;
        } else {
            let key = copy_text(key)?;
            self.counters
                .try_reserve(1)
                .map_err(|_| InvocationCounterError::Allocation)?;
            self.counters.insert(
                key,
                Counter {
                    value: start,
                    restart,
                },
            );
        }
        self.text_bytes = text_bytes;
        Ok(start)
    }

    /// Forget only this exact key, releasing its logical text/count budget.
    pub fn reset(&mut self, key: &str) -> bool {
        let Some(counter) = self.counters.remove(key) else {
            return false;
        };
        self.text_bytes -= key.len() + counter.restart.as_ref().map_or(0, String::len);
        true
    }
}

fn copy_text(text: &str) -> Result<String, InvocationCounterError> {
    let mut owned = String::new();
    owned
        .try_reserve_exact(text.len())
        .map_err(|_| InvocationCounterError::Allocation)?;
    owned.push_str(text);
    Ok(owned)
}
