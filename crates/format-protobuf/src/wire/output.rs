use crate::{BoundedEncodeError, EncodeLimit};

/// The same codec writes either legacy bytes, checked lengths, or bounded bytes.
/// Child sinks represent packed fields and nested messages.
pub(super) trait Output: Sized {
    fn child(&self) -> Self;
    fn len(&self) -> usize;
    fn push(&mut self, byte: u8);
    fn extend_from_slice(&mut self, bytes: &[u8]);
    fn append(&mut self, child: Self);
}

impl Output for Vec<u8> {
    fn child(&self) -> Self {
        Vec::new()
    }

    fn len(&self) -> usize {
        self.len()
    }

    fn push(&mut self, byte: u8) {
        self.push(byte);
    }

    fn extend_from_slice(&mut self, bytes: &[u8]) {
        self.extend_from_slice(bytes);
    }

    fn append(&mut self, mut child: Self) {
        self.append(&mut child);
    }
}

pub(super) struct Count {
    length: Option<usize>,
    pub(super) all_zero: bool,
}

impl Count {
    pub(super) fn new() -> Self {
        Self {
            length: Some(0),
            all_zero: true,
        }
    }

    pub(super) fn finish(self) -> Result<usize, BoundedEncodeError> {
        self.length.ok_or(BoundedEncodeError::LengthOverflow)
    }

    fn add(&mut self, amount: usize) {
        self.length = self.length.and_then(|length| length.checked_add(amount));
    }
}

impl Output for Count {
    fn child(&self) -> Self {
        Self::new()
    }

    fn len(&self) -> usize {
        // Only used for counting a length prefix or testing emptiness. An
        // overflow is sticky through append and reported after validation,
        // never used to allocate or emitted to the caller.
        self.length.unwrap_or(usize::MAX)
    }

    fn push(&mut self, byte: u8) {
        self.add(1);
        self.all_zero &= byte == 0;
    }

    fn extend_from_slice(&mut self, bytes: &[u8]) {
        self.add(bytes.len());
        self.all_zero = self.all_zero && bytes.iter().all(|byte| *byte == 0);
    }

    fn append(&mut self, child: Self) {
        self.length = self
            .length
            .zip(child.length)
            .and_then(|(left, right)| left.checked_add(right));
        self.all_zero &= child.all_zero;
    }
}

pub(super) struct BoundedOutput {
    bytes: Vec<u8>,
    maximum: usize,
    error: Option<BoundedEncodeError>,
}

impl BoundedOutput {
    pub(super) fn new(maximum: usize) -> Self {
        Self {
            bytes: Vec::new(),
            maximum,
            error: None,
        }
    }

    pub(super) fn finish(self) -> Result<Vec<u8>, BoundedEncodeError> {
        match self.error {
            Some(error) => Err(error),
            None => Ok(self.bytes),
        }
    }

    fn reserve(&mut self, additional: usize) -> bool {
        if self.error.is_some() {
            return false;
        }
        let Some(length) = self.bytes.len().checked_add(additional) else {
            self.error = Some(BoundedEncodeError::LengthOverflow);
            return false;
        };
        if length > self.maximum {
            self.error = Some(BoundedEncodeError::LimitExceeded {
                limit: EncodeLimit::OutputBytes,
                maximum: self.maximum,
            });
            return false;
        }
        if length > self.bytes.capacity() {
            // Amortize small appends without Vec's uncapped geometric growth.
            // Every requested capacity is at most the validated final length.
            let capacity = self
                .bytes
                .capacity()
                .saturating_mul(2)
                .max(8)
                .min(self.maximum)
                .max(length);
            if let Err(error) = self.bytes.try_reserve_exact(capacity - self.bytes.len()) {
                self.error = Some(BoundedEncodeError::Allocation(error));
                return false;
            }
        }
        true
    }
}

impl Output for BoundedOutput {
    fn child(&self) -> Self {
        Self::new(self.maximum)
    }

    fn len(&self) -> usize {
        self.bytes.len()
    }

    fn push(&mut self, byte: u8) {
        if self.reserve(1) {
            self.bytes.push(byte);
        }
    }

    fn extend_from_slice(&mut self, bytes: &[u8]) {
        if self.reserve(bytes.len()) {
            self.bytes.extend_from_slice(bytes);
        }
    }

    fn append(&mut self, child: Self) {
        if self.error.is_none() {
            if let Some(error) = child.error {
                self.error = Some(error);
            } else {
                self.extend_from_slice(&child.bytes);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_overflow_is_sticky_across_nested_payloads() {
        let mut count = Count::new();
        count.add(usize::MAX);
        count.push(0);
        count.append(Count::new());
        assert!(matches!(
            count.finish(),
            Err(BoundedEncodeError::LengthOverflow)
        ));

        let mut parent = Count::new();
        let mut child = Count::new();
        child.add(usize::MAX);
        parent.push(0);
        parent.append(child);
        assert!(matches!(
            parent.finish(),
            Err(BoundedEncodeError::LengthOverflow)
        ));
    }

    #[test]
    fn buffers_check_growth_before_reserving() {
        let mut output = BoundedOutput::new(1);
        output.extend_from_slice(&[1, 2]);
        assert_eq!(output.bytes.capacity(), 0);
        assert!(matches!(
            output.finish(),
            Err(BoundedEncodeError::LimitExceeded { .. })
        ));

        let mut output = BoundedOutput::new(3);
        output.push(1);
        output.extend_from_slice(&[2, 3]);
        assert_eq!(output.bytes.capacity(), 3);
        assert_eq!(output.finish().unwrap(), [1, 2, 3]);
    }

    #[test]
    fn overflow_and_empty_child_allocation_failures_are_not_lost() {
        let mut output = BoundedOutput::new(1);
        output.push(1);
        assert!(!output.reserve(usize::MAX));
        assert!(matches!(
            output.finish(),
            Err(BoundedEncodeError::LengthOverflow)
        ));

        // Vec rejects a capacity above isize::MAX without a large allocation.
        let mut child = BoundedOutput::new(usize::MAX);
        assert!(!child.reserve(usize::MAX));
        assert_eq!(child.bytes.capacity(), 0);
        let mut parent = BoundedOutput::new(1);
        parent.append(child);
        assert!(matches!(
            parent.finish(),
            Err(BoundedEncodeError::Allocation(_))
        ));
    }
}
