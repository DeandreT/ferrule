//! Ferrule's version 1 read-only C interface.

mod wire;

use std::{
    borrow::Cow,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr, slice,
};

use ferrule_host::{
    FileCodecError, InspectionError, InspectionLimits, LoadedProject, ValidationError,
    ValidationLimits, inspect, load, validate,
};
use serde_json::{Value, json};

pub struct FerruleHostProject(LoadedProject);
pub struct FerruleHostBuffer(Box<[u8]>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
enum Status {
    Ok = 0,
    InvalidArgument = 1,
    DecodeFailed = 2,
    UnsupportedProjectVersion = 3,
    ProjectTooLarge = 4,
    InspectionLimitExceeded = 5,
    ValidationLimitExceeded = 6,
    InternalError = 7,
}

#[derive(Debug)]
struct Failure {
    status: Status,
    code: &'static str,
    message: Cow<'static, str>,
}

impl Failure {
    fn new(status: Status, code: &'static str, message: impl Into<Cow<'static, str>>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    fn argument(message: &'static str) -> Self {
        Self::new(Status::InvalidArgument, "invalidArgument", message)
    }

    fn internal() -> Self {
        Self::new(
            Status::InternalError,
            "internalError",
            "The native host could not complete the request.",
        )
    }

    fn decode(error: FileCodecError) -> Self {
        let (status, code) = match error {
            FileCodecError::TooLarge { .. } => (Status::ProjectTooLarge, "projectTooLarge"),
            FileCodecError::UnsupportedVersion { .. } => (
                Status::UnsupportedProjectVersion,
                "unsupportedProjectVersion",
            ),
            _ => (Status::DecodeFailed, "decodeFailed"),
        };
        Self::new(status, code, error.to_string())
    }

    fn inspect(error: InspectionError) -> Self {
        let code = match error {
            InspectionError::ItemLimitExceeded | InspectionError::ItemCountOverflow => {
                "inspectionItemLimit"
            }
            InspectionError::SchemaDepthExceeded => "schemaDepthLimit",
            InspectionError::ScopeDepthExceeded => "scopeDepthLimit",
            _ => return Self::internal(),
        };
        Self::new(Status::InspectionLimitExceeded, code, error.to_string())
    }

    fn validate(error: ValidationError) -> Self {
        let code = match error {
            ValidationError::Inspection(error) => return Self::inspect(error),
            ValidationError::IssueLimitExceeded => "validationIssueLimit",
            ValidationError::OwnerIdentityTooLong => "ownerIdentityTooLong",
            _ => return Self::internal(),
        };
        Self::new(Status::ValidationLimitExceeded, code, error.to_string())
    }

    fn into_bytes(self) -> Vec<u8> {
        json!({ "code": self.code, "message": self.message })
            .to_string()
            .into_bytes()
    }
}

fn guard<T>(operation: impl FnOnce() -> Result<T, Failure>) -> Result<T, Failure> {
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(result) => result,
        Err(payload) => {
            // Disposing a custom payload must not cause a second unwind across C.
            if let Err(nested) = catch_unwind(AssertUnwindSafe(|| drop(payload))) {
                std::mem::forget(nested);
            }
            Err(Failure::internal())
        }
    }
}

fn buffer(bytes: Vec<u8>) -> *mut FerruleHostBuffer {
    Box::into_raw(Box::new(FerruleHostBuffer(bytes.into_boxed_slice())))
}

#[unsafe(no_mangle)]
pub extern "C" fn ferrule_host_abi_version() -> u32 {
    1
}

/// Open a project from borrowed bytes.
///
/// # Safety
///
/// Non-null output slots must be valid and writable for one pointer. The byte
/// pointer must be valid for reads of the given length unless the length is zero.
/// The slots and input must be disjoint.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ferrule_host_v1_load(
    bytes: *const u8,
    length: usize,
    maximum_bytes: u32,
    project: *mut *mut FerruleHostProject,
    error: *mut *mut FerruleHostBuffer,
) -> u32 {
    if !project.is_null() {
        unsafe { project.write(ptr::null_mut()) };
    }
    if error.is_null() {
        return Status::InvalidArgument as u32;
    }
    unsafe { error.write(ptr::null_mut()) };

    let result = guard(|| {
        if project.is_null() {
            return Err(Failure::argument("The project output slot is required."));
        }
        if (bytes.is_null() && length != 0) || length > isize::MAX as usize {
            return Err(Failure::argument("The input byte range is invalid."));
        }
        let maximum_bytes = maximum_bytes as usize;
        if length > maximum_bytes {
            return Err(Failure::decode(FileCodecError::TooLarge {
                bytes: length,
                max: maximum_bytes,
            }));
        }
        let native = if length == 0 {
            &[]
        } else {
            unsafe { slice::from_raw_parts(bytes, length) }
        };
        load(native, maximum_bytes).map_err(Failure::decode)
    });
    match result {
        Ok(loaded) => {
            unsafe { project.write(Box::into_raw(Box::new(FerruleHostProject(loaded)))) };
            Status::Ok as u32
        }
        Err(failure) => {
            let status = failure.status;
            unsafe { error.write(buffer(failure.into_bytes())) };
            status as u32
        }
    }
}

/// Report an immutable project's inventory.
///
/// # Safety
///
/// A non-null project must remain live and a non-null output slot must be
/// valid and writable for one pointer, disjoint from the project.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ferrule_host_v1_inspect(
    project: *const FerruleHostProject,
    maximum_items: u32,
    maximum_text_bytes: u32,
    result: *mut *mut FerruleHostBuffer,
) -> u32 {
    unsafe {
        report(project, result, |project| {
            let limits = InspectionLimits {
                maximum_items,
                maximum_text_bytes: maximum_text_bytes as usize,
            };
            wire::inspection(
                inspect(project.project(), project.layout(), limits).map_err(Failure::inspect)?,
            )
        })
    }
}

/// Report an immutable project's validation results.
///
/// # Safety
///
/// A non-null project must remain live and a non-null output slot must be
/// valid and writable for one pointer, disjoint from the project.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ferrule_host_v1_validate(
    project: *const FerruleHostProject,
    maximum_items: u32,
    maximum_text_bytes: u32,
    maximum_issues: u32,
    maximum_diagnostics: u32,
    result: *mut *mut FerruleHostBuffer,
) -> u32 {
    unsafe {
        report(project, result, |project| {
            let limits = ValidationLimits {
                inspection: InspectionLimits {
                    maximum_items,
                    maximum_text_bytes: maximum_text_bytes as usize,
                },
                maximum_issues: maximum_issues as usize,
                maximum_diagnostics: maximum_diagnostics as usize,
            };
            wire::validation(
                validate(project.project(), project.layout(), limits).map_err(Failure::validate)?,
            )
        })
    }
}

unsafe fn report(
    project: *const FerruleHostProject,
    result: *mut *mut FerruleHostBuffer,
    operation: impl FnOnce(&LoadedProject) -> Result<Value, Failure>,
) -> u32 {
    if result.is_null() {
        return Status::InvalidArgument as u32;
    }
    unsafe { result.write(ptr::null_mut()) };
    let outcome = guard(|| {
        let project = unsafe { project.as_ref() }
            .ok_or_else(|| Failure::argument("A project handle is required."))?;
        let json = operation(&project.0)?;
        serde_json::to_vec(&json).map_err(|_| Failure::internal())
    });
    let (status, bytes) = match outcome {
        Ok(bytes) => (Status::Ok, bytes),
        Err(failure) => (failure.status, failure.into_bytes()),
    };
    unsafe { result.write(buffer(bytes)) };
    status as u32
}

/// Borrow a report or error's bytes until its buffer is released.
///
/// # Safety
///
/// A non-null buffer must remain live for the entire borrow. A non-null length
/// slot must be writable and disjoint from the buffer and its bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ferrule_host_v1_buffer_data(
    buffer: *const FerruleHostBuffer,
    length: *mut usize,
) -> *const u8 {
    if length.is_null() {
        return ptr::null();
    }
    let bytes = unsafe { buffer.as_ref() }.map(|buffer| buffer.0.as_ref());
    unsafe { length.write(bytes.map_or(0, <[u8]>::len)) };
    bytes.map_or(ptr::null(), <[u8]>::as_ptr)
}

/// Release a report or error.
///
/// # Safety
///
/// A non-null buffer must be an owned handle from this library with no active
/// borrows. Release it once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ferrule_host_v1_buffer_free(buffer: *mut FerruleHostBuffer) {
    if !buffer.is_null() {
        drop(unsafe { Box::from_raw(buffer) });
    }
}

/// Release a project.
///
/// # Safety
///
/// A non-null project must be an owned handle from this library with no active
/// calls borrowing it. Release it once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ferrule_host_v1_project_free(project: *mut FerruleHostProject) {
    if !project.is_null() {
        drop(unsafe { Box::from_raw(project) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panics_become_structured_internal_errors() {
        let failure = guard::<()>(|| panic!("test panic")).unwrap_err();
        assert_eq!(failure.status, Status::InternalError);
        let error: Value = serde_json::from_slice(&failure.into_bytes()).unwrap();
        assert_eq!(error["code"], "internalError");
        assert!(!error.to_string().contains("test panic"));
    }

    #[test]
    fn immutable_project_handles_can_be_shared_between_threads() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<FerruleHostProject>();
    }
}
