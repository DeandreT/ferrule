use ir::{Instance, Value};

use crate::ProtobufError;

/// Errors specific to opt-in bounded encoding. Native codec errors retain
/// their owned type, path, and message without extending [`ProtobufError`].
#[derive(Debug, thiserror::Error)]
pub enum BoundedEncodeError {
    #[error("{0}")]
    Protobuf(#[from] ProtobufError),
    #[error("invalid protobuf encoding limit {limit:?}: {maximum}")]
    InvalidLimit { limit: EncodeLimit, maximum: usize },
    #[error("protobuf encoding exceeds {limit:?} limit of {maximum}")]
    LimitExceeded { limit: EncodeLimit, maximum: usize },
    #[error("protobuf encoded length overflows usize")]
    LengthOverflow,
    #[error("could not allocate protobuf output: {0}")]
    Allocation(#[source] std::collections::TryReserveError),
}

/// A resource controlled by [`EncodeLimits`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodeLimit {
    OutputBytes,
    Values,
    Fields,
    InputBytes,
    Depth,
}

/// Finite data budgets for the opt-in bounded encoders.
///
/// Zero is valid for every budget, including empty protobuf output. Byte/count
/// budgets cannot exceed `isize::MAX`, and depth cannot exceed 128. The root
/// instance is one value at depth zero; each group member or sequence item adds
/// one level. Fields counts all supplied group members, and input bytes counts
/// their UTF-8 names plus all supplied string values (including bytes fields).
/// Repeated/mapped-sequence containers also count as values.
///
/// These are data limits, not an exact process-memory ceiling: the caller owns
/// the layout/input, and hash tables, paths, allocator overhead, and up to one
/// output buffer per active message/packed field need additional memory.
#[derive(Clone, Copy, Debug)]
pub struct EncodeLimits {
    pub max_output_bytes: usize,
    pub max_values: usize,
    pub max_fields: usize,
    pub max_input_bytes: usize,
    pub max_depth: usize,
}

impl Default for EncodeLimits {
    fn default() -> Self {
        Self {
            max_output_bytes: crate::MAX_MESSAGE_BYTES,
            max_values: 1_000_000,
            max_fields: 1_000_000,
            max_input_bytes: crate::MAX_MESSAGE_BYTES,
            max_depth: 128,
        }
    }
}

impl EncodeLimits {
    pub(crate) fn validate(self, instance: &Instance) -> Result<(), BoundedEncodeError> {
        for (limit, maximum) in [
            (EncodeLimit::OutputBytes, self.max_output_bytes),
            (EncodeLimit::Values, self.max_values),
            (EncodeLimit::Fields, self.max_fields),
            (EncodeLimit::InputBytes, self.max_input_bytes),
            (EncodeLimit::Depth, self.max_depth),
        ] {
            if maximum > isize::MAX as usize || (limit == EncodeLimit::Depth && maximum > 128) {
                return Err(BoundedEncodeError::InvalidLimit { limit, maximum });
            }
        }
        InputBudget {
            limits: self,
            values: 0,
            fields: 0,
            bytes: 0,
        }
        .visit(instance, 0)
    }
}

pub(crate) fn check(
    actual: usize,
    limit: EncodeLimit,
    maximum: usize,
) -> Result<(), BoundedEncodeError> {
    if actual > maximum {
        Err(BoundedEncodeError::LimitExceeded { limit, maximum })
    } else {
        Ok(())
    }
}

fn add(
    total: &mut usize,
    amount: usize,
    limit: EncodeLimit,
    maximum: usize,
) -> Result<(), BoundedEncodeError> {
    *total = total
        .checked_add(amount)
        .ok_or(BoundedEncodeError::LimitExceeded { limit, maximum })?;
    check(*total, limit, maximum)
}

struct InputBudget {
    limits: EncodeLimits,
    values: usize,
    fields: usize,
    bytes: usize,
}

impl InputBudget {
    fn visit(&mut self, instance: &Instance, depth: usize) -> Result<(), BoundedEncodeError> {
        check(depth, EncodeLimit::Depth, self.limits.max_depth)?;
        add(
            &mut self.values,
            1,
            EncodeLimit::Values,
            self.limits.max_values,
        )?;
        match instance {
            Instance::Group(fields) => {
                add(
                    &mut self.fields,
                    fields.len(),
                    EncodeLimit::Fields,
                    self.limits.max_fields,
                )?;
                // Check the immediate value count before walking or allocating
                // anything proportional to a wide supplied group.
                check(
                    self.values.checked_add(fields.len()).ok_or(
                        BoundedEncodeError::LimitExceeded {
                            limit: EncodeLimit::Values,
                            maximum: self.limits.max_values,
                        },
                    )?,
                    EncodeLimit::Values,
                    self.limits.max_values,
                )?;
                for (name, value) in fields {
                    self.add_bytes(name.len())?;
                    self.visit(value, depth + 1)?;
                }
            }
            Instance::Repeated(values) | Instance::MappedSequence(values) => {
                check(
                    self.values.checked_add(values.len()).ok_or(
                        BoundedEncodeError::LimitExceeded {
                            limit: EncodeLimit::Values,
                            maximum: self.limits.max_values,
                        },
                    )?,
                    EncodeLimit::Values,
                    self.limits.max_values,
                )?;
                for value in values {
                    self.visit(value, depth + 1)?;
                }
            }
            Instance::Scalar(Value::String(value)) => self.add_bytes(value.len())?,
            // Document sets are not a protobuf value. The native codec rejects
            // the container without opening any of its documents.
            Instance::Scalar(_) | Instance::DocumentSet(_) => {}
        }
        Ok(())
    }

    fn add_bytes(&mut self, amount: usize) -> Result<(), BoundedEncodeError> {
        add(
            &mut self.bytes,
            amount,
            EncodeLimit::InputBytes,
            self.limits.max_input_bytes,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_count_overflow_is_a_typed_limit_without_allocation() {
        let mut total = usize::MAX;
        assert!(matches!(
            add(&mut total, 1, EncodeLimit::Values, 10),
            Err(BoundedEncodeError::LimitExceeded {
                limit: EncodeLimit::Values,
                maximum: 10,
            })
        ));
    }
}
