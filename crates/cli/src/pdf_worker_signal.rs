//! The CLI's scoped signal policy while it owns an opted-in PDF acquisition.
//!
//! The caller must keep the guard (or a failed admission's recovery state) on
//! its launch thread and finish explicitly after the worker is closed. This
//! module does not own a child and cannot establish child closure. Signal policy
//! remains process-wide, so this route requires the CLI to be its only owner;
//! a readback cannot prevent arbitrary concurrent foreign policy changes.

use std::fmt;
use std::io;
use std::marker::PhantomData;
use std::rc::Rc;

use pdf_worker::host::CancellationFlag;

#[derive(Debug)]
enum SignalFailure {
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu")))]
    UnsupportedHost,
    AlreadyActive,
    WrongThread,
    ForeignDisposition(i32),
    CompetingDisposition(i32),
    MissingSavedDisposition(i32),
    Io {
        operation: &'static str,
        signal: Option<i32>,
        error: io::Error,
    },
}

impl fmt::Display for SignalFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(not(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu")))]
            Self::UnsupportedHost => f.write_str("PDF worker signals require Linux x86_64 GNU"),
            Self::AlreadyActive => f.write_str("another PDF signal guard still owns the policy"),
            Self::WrongThread => f.write_str("PDF signal restoration requires the launch thread"),
            Self::ForeignDisposition(signal) => {
                write!(f, "signal {signal} does not have its default disposition")
            }
            Self::CompetingDisposition(signal) => {
                write!(
                    f,
                    "signal {signal} no longer has this guard's installed policy"
                )
            }
            Self::MissingSavedDisposition(signal) => {
                write!(f, "signal {signal} has no retained original disposition")
            }
            Self::Io {
                operation,
                signal,
                error,
            } => {
                write!(f, "{operation}")?;
                if let Some(signal) = signal {
                    write!(f, " for signal {signal}")?;
                }
                write!(f, ": {error}")
            }
        }
    }
}

fn display_failures(f: &mut fmt::Formatter<'_>, failures: &[SignalFailure]) -> fmt::Result {
    for (index, failure) in failures.iter().enumerate() {
        if index != 0 {
            f.write_str("; ")?;
        }
        write!(f, "{failure}")?;
    }
    Ok(())
}

fn first_io_error(failures: &[SignalFailure]) -> Option<&(dyn std::error::Error + 'static)> {
    failures.iter().find_map(|failure| match failure {
        SignalFailure::Io { error, .. } => Some(error as &(dyn std::error::Error + 'static)),
        _ => None,
    })
}

/// A failed admission retains its primary errors and any unfinished rollback.
/// Initial admission and rollback retain at most four errors. Retries return
/// separate errors instead of growing this error forever.
#[must_use = "retain failed admission until needs_recovery() is false"]
pub(super) struct SignalBridgeError {
    failures: Vec<SignalFailure>,
    #[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
    recovery: Option<Box<platform::RecoveryState<'static, platform::Kernel>>>,
}

impl SignalBridgeError {
    pub(super) fn needs_recovery(&self) -> bool {
        #[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
        {
            self.recovery.is_some()
        }
        #[cfg(not(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu")))]
        {
            false
        }
    }

    /// Only for admission failure: no child has been launched by this guard.
    /// The saved thread identity prevents restoring another thread's mask.
    pub(super) fn retry_after_no_child(&mut self) -> Result<(), SignalRestoreError> {
        #[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
        if let Some(recovery) = &mut self.recovery {
            recovery.finish()?;
            self.recovery = None;
        }
        Ok(())
    }
}

impl fmt::Debug for SignalBridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SignalBridgeError")
            .field("failures", &self.failures)
            .field("needs_recovery", &self.needs_recovery())
            .finish()
    }
}

impl fmt::Display for SignalBridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PDF signal admission failed: ")?;
        display_failures(f, &self.failures)
    }
}

impl std::error::Error for SignalBridgeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        first_io_error(&self.failures)
    }
}

/// Each finite restoration attempt retains at most three operation errors.
#[derive(Debug)]
pub(super) struct SignalRestoreError {
    failures: Vec<SignalFailure>,
}

impl fmt::Display for SignalRestoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PDF signal restoration failed: ")?;
        display_failures(f, &self.failures)
    }
}

impl std::error::Error for SignalRestoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        first_io_error(&self.failures)
    }
}

#[must_use = "retain this guard until explicit signal restoration after worker closure"]
pub(super) struct PdfSignalGuard {
    #[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
    state: platform::RecoveryState<'static, platform::Kernel>,
    // A signal mask belongs to a thread. The live guard cannot move to one.
    _launch_thread: PhantomData<Rc<()>>,
}

impl PdfSignalGuard {
    pub(super) fn begin() -> Result<Self, SignalBridgeError> {
        #[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
        {
            match platform::begin_transaction(
                platform::Kernel,
                &platform::ACTIVE,
                &platform::PENDING,
            ) {
                Ok(state) => Ok(Self {
                    state,
                    _launch_thread: PhantomData,
                }),
                Err(error) => Err(SignalBridgeError {
                    failures: error.failures,
                    recovery: error.recovery,
                }),
            }
        }
        #[cfg(not(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu")))]
        {
            Err(SignalBridgeError {
                failures: vec![SignalFailure::UnsupportedHost],
            })
        }
    }

    pub(super) fn observe(&self, cancel: &CancellationFlag) {
        #[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
        platform::observe_pending(&platform::PENDING, cancel);
        #[cfg(not(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu")))]
        let _ = cancel;
    }

    /// The caller must have established worker closure before this operation.
    /// Failure leaves custody with this same guard for explicit later retries.
    pub(super) fn finish_after_closed(&mut self) -> Result<(), SignalRestoreError> {
        #[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
        self.state.finish()?;
        Ok(())
    }
}

impl Drop for PdfSignalGuard {
    fn drop(&mut self) {
        // Deliberately no restoration, child operations, or ACTIVE release.
        // Only checked explicit restoration can release the process-local slot.
    }
}

#[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
mod platform {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread::{self, ThreadId};

    use super::{CancellationFlag, SignalFailure, SignalRestoreError, io};

    pub(super) static ACTIVE: AtomicBool = AtomicBool::new(false);
    pub(super) static PENDING: AtomicBool = AtomicBool::new(false);
    const SIGNALS: [i32; 2] = [libc::SIGINT, libc::SIGTERM];

    // D01 / D07: the complete C ABI handler consists of one lock-free atomic
    // store on the admitted x86_64 target. It cannot exit or discard an owner.
    extern "C" fn capture_signal(_signal: libc::c_int) {
        PENDING.store(true, Ordering::Relaxed);
    }

    pub(super) fn observe_pending(pending: &AtomicBool, cancel: &CancellationFlag) {
        // This is a latched bit; observing it cannot erase a concurrent signal.
        if pending.load(Ordering::Relaxed) {
            cancel.cancel();
        }
    }

    // Injection is limited to this signal transaction. Test backends never
    // install a real handler, change a real mask, launch a process, or wait.
    pub(super) trait Backend {
        type Action: Copy;
        type Mask: Copy;

        fn block(&mut self) -> io::Result<Self::Mask>;
        fn restore_mask(&mut self, mask: &Self::Mask) -> io::Result<()>;
        fn query(&mut self, signal: i32) -> io::Result<Self::Action>;
        fn install(&mut self, signal: i32) -> io::Result<Self::Action>;
        fn restore(&mut self, signal: i32, action: &Self::Action) -> io::Result<()>;
        fn is_default(action: &Self::Action) -> bool;
        fn is_handler(action: &Self::Action) -> bool;
        fn same_policy(left: &Self::Action, right: &Self::Action) -> io::Result<bool>;
    }

    pub(super) struct Kernel;

    impl Kernel {
        fn signal_set() -> io::Result<libc::sigset_t> {
            // Zero is a valid initial representation; libc fills this live set.
            let mut set = unsafe { std::mem::zeroed::<libc::sigset_t>() };
            // SAFETY: set is initialized writable storage for this synchronous call.
            if unsafe { libc::sigemptyset(&mut set) } == -1 {
                return Err(io::Error::last_os_error());
            }
            for signal in SIGNALS {
                // SAFETY: the live set and both supported signal numbers are valid.
                if unsafe { libc::sigaddset(&mut set, signal) } == -1 {
                    return Err(io::Error::last_os_error());
                }
            }
            Ok(set)
        }

        fn replace(signal: i32, action: &libc::sigaction) -> io::Result<libc::sigaction> {
            let mut old = unsafe { std::mem::zeroed::<libc::sigaction>() };
            // SAFETY: both initialized action objects live through the call; the
            // returned full previous disposition is copied into owned storage.
            if unsafe { libc::sigaction(signal, action, &mut old) } == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(old)
        }
    }

    impl Backend for Kernel {
        type Action = libc::sigaction;
        type Mask = libc::sigset_t;

        fn block(&mut self) -> io::Result<Self::Mask> {
            let set = Self::signal_set()?;
            let mut old = unsafe { std::mem::zeroed::<libc::sigset_t>() };
            // SAFETY: this changes only the calling thread's mask and saves its
            // exact previous set. pthread_sigmask returns an error number itself.
            let result = unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &set, &mut old) };
            if result != 0 {
                return Err(io::Error::from_raw_os_error(result));
            }
            Ok(old)
        }

        fn restore_mask(&mut self, mask: &Self::Mask) -> io::Result<()> {
            // SAFETY: the retained initialized set belongs to this launch thread.
            let result =
                unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, mask, std::ptr::null_mut()) };
            if result != 0 {
                return Err(io::Error::from_raw_os_error(result));
            }
            Ok(())
        }

        fn query(&mut self, signal: i32) -> io::Result<Self::Action> {
            let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
            // SAFETY: null new action requests a read; action is writable live storage.
            if unsafe { libc::sigaction(signal, std::ptr::null(), &mut action) } == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(action)
        }

        fn install(&mut self, signal: i32) -> io::Result<Self::Action> {
            let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
            action.sa_sigaction = capture_signal as *const () as libc::sighandler_t;
            action.sa_mask = Self::signal_set()?;
            // No SA_SIGINFO/SA_RESETHAND: repeated signals only latch the bit.
            action.sa_flags = 0;
            action.sa_restorer = None;
            Self::replace(signal, &action)
        }

        fn restore(&mut self, signal: i32, action: &Self::Action) -> io::Result<()> {
            Self::replace(signal, action).map(|_| ())
        }

        fn is_default(action: &Self::Action) -> bool {
            action.sa_sigaction == libc::SIG_DFL
        }

        fn is_handler(action: &Self::Action) -> bool {
            action.sa_sigaction == capture_signal as *const () as libc::sighandler_t
                && action.sa_flags & libc::SA_SIGINFO == 0
        }

        fn same_policy(left: &Self::Action, right: &Self::Action) -> io::Result<bool> {
            if left.sa_sigaction != right.sa_sigaction
                || left.sa_flags != right.sa_flags
                || left.sa_restorer.map(|f| f as *const () as usize)
                    != right.sa_restorer.map(|f| f as *const () as usize)
            {
                return Ok(false);
            }
            // Linux's valid signal numbers are 1..=64. Compare membership, not
            // struct padding or uninitialized bytes, including libc's reserved
            // signal bits. These ordinary-context calls are outside the handler.
            for signal in 1..=64 {
                // SAFETY: both stored masks are initialized; each signal is valid.
                let left_member = unsafe { libc::sigismember(&left.sa_mask, signal) };
                if left_member == -1 {
                    return Err(io::Error::last_os_error());
                }
                // SAFETY: as above, with the second retained initialized mask.
                let right_member = unsafe { libc::sigismember(&right.sa_mask, signal) };
                if right_member == -1 {
                    return Err(io::Error::last_os_error());
                }
                if left_member != right_member {
                    return Ok(false);
                }
            }
            Ok(true)
        }
    }

    pub(super) struct RecoveryState<'a, B: Backend> {
        backend: B,
        slot: &'a AtomicBool,
        old_actions: [Option<B::Action>; 2],
        installed: [bool; 2],
        installed_policy: [Option<B::Action>; 2],
        mask_to_restore: Option<B::Mask>,
        launch_thread: ThreadId,
        complete: bool,
    }

    impl<B: Backend> RecoveryState<'_, B> {
        pub(super) fn finish(&mut self) -> Result<(), SignalRestoreError> {
            if self.complete {
                return Ok(());
            }
            if thread::current().id() != self.launch_thread {
                return Err(SignalRestoreError {
                    failures: vec![SignalFailure::WrongThread],
                });
            }
            let mut failures = Vec::new();
            if self.mask_to_restore.is_none() && self.installed.iter().any(|installed| *installed) {
                match self.backend.block() {
                    Ok(mask) => self.mask_to_restore = Some(mask),
                    Err(error) => {
                        return Err(SignalRestoreError {
                            failures: vec![io_failure("block during restoration", None, error)],
                        });
                    }
                }
            }
            for (index, signal) in SIGNALS.into_iter().enumerate() {
                if !self.installed[index] {
                    continue;
                }
                let current = match self.backend.query(signal) {
                    Ok(current) => current,
                    Err(error) => {
                        failures.push(io_failure(
                            "read installed disposition",
                            Some(signal),
                            error,
                        ));
                        continue;
                    }
                };
                if !B::is_handler(&current) {
                    // D04: never overwrite an observed competing disposition.
                    failures.push(SignalFailure::CompetingDisposition(signal));
                    continue;
                }
                if let Some(expected) = self.installed_policy[index] {
                    match B::same_policy(&current, &expected) {
                        Ok(true) => {}
                        Ok(false) => {
                            failures.push(SignalFailure::CompetingDisposition(signal));
                            continue;
                        }
                        Err(error) => {
                            failures.push(io_failure(
                                "compare installed policy",
                                Some(signal),
                                error,
                            ));
                            continue;
                        }
                    }
                }
                let Some(old) = self.old_actions[index] else {
                    failures.push(SignalFailure::MissingSavedDisposition(signal));
                    continue;
                };
                match self.backend.restore(signal, &old) {
                    Ok(()) => {
                        self.installed[index] = false;
                        self.installed_policy[index] = None;
                    }
                    Err(error) => {
                        failures.push(io_failure("restore disposition", Some(signal), error))
                    }
                }
            }
            if let Some(mask) = self.mask_to_restore {
                match self.backend.restore_mask(&mask) {
                    Ok(()) => self.mask_to_restore = None,
                    Err(error) => {
                        failures.push(io_failure("restore launch-thread mask", None, error))
                    }
                }
            }
            if self.installed.iter().all(|installed| !installed) && self.mask_to_restore.is_none() {
                self.complete = true;
                self.slot.store(false, Ordering::Release);
            }
            if failures.is_empty() {
                Ok(())
            } else {
                Err(SignalRestoreError { failures })
            }
        }
    }

    pub(super) struct BeginTransactionError<'a, B: Backend> {
        pub(super) failures: Vec<SignalFailure>,
        pub(super) recovery: Option<Box<RecoveryState<'a, B>>>,
    }

    impl<B: Backend> std::fmt::Debug for BeginTransactionError<'_, B> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("BeginTransactionError")
                .field("failures", &self.failures)
                .field("needs_recovery", &self.recovery.is_some())
                .finish()
        }
    }

    fn io_failure(operation: &'static str, signal: Option<i32>, error: io::Error) -> SignalFailure {
        SignalFailure::Io {
            operation,
            signal,
            error,
        }
    }

    fn rollback<'a, B: Backend>(
        mut state: RecoveryState<'a, B>,
        mut failures: Vec<SignalFailure>,
    ) -> BeginTransactionError<'a, B> {
        if let Err(error) = state.finish() {
            failures.extend(error.failures);
        }
        let recovery = if state.complete {
            None
        } else {
            Some(Box::new(state))
        };
        BeginTransactionError { failures, recovery }
    }

    pub(super) fn begin_transaction<'a, B: Backend>(
        backend: B,
        slot: &'a AtomicBool,
        pending: &AtomicBool,
    ) -> Result<RecoveryState<'a, B>, BeginTransactionError<'a, B>> {
        if slot
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(BeginTransactionError {
                failures: vec![SignalFailure::AlreadyActive],
                recovery: None,
            });
        }
        let mut state = RecoveryState {
            backend,
            slot,
            old_actions: [None; 2],
            installed: [false; 2],
            installed_policy: [None; 2],
            mask_to_restore: None,
            launch_thread: thread::current().id(),
            complete: false,
        };
        pending.store(false, Ordering::Relaxed);
        match state.backend.block() {
            Ok(mask) => state.mask_to_restore = Some(mask),
            Err(error) => {
                return Err(rollback(
                    state,
                    vec![io_failure("block during admission", None, error)],
                ));
            }
        }
        // D02: check both defaults before changing either disposition.
        for (index, signal) in SIGNALS.into_iter().enumerate() {
            match state.backend.query(signal) {
                Ok(action) if B::is_default(&action) => state.old_actions[index] = Some(action),
                Ok(_) => {
                    return Err(rollback(
                        state,
                        vec![SignalFailure::ForeignDisposition(signal)],
                    ));
                }
                Err(error) => {
                    return Err(rollback(
                        state,
                        vec![io_failure("read original disposition", Some(signal), error)],
                    ));
                }
            }
        }
        for (index, signal) in SIGNALS.into_iter().enumerate() {
            match state.backend.install(signal) {
                Ok(actual_old) => {
                    // The actual exchange result is the exact policy to restore.
                    state.old_actions[index] = Some(actual_old);
                    state.installed[index] = true;
                    if !B::is_default(&actual_old) {
                        return Err(rollback(
                            state,
                            vec![SignalFailure::ForeignDisposition(signal)],
                        ));
                    }
                }
                Err(error) => {
                    // D03: failure installing TERM explicitly rolls INT back.
                    return Err(rollback(
                        state,
                        vec![io_failure("install disposition", Some(signal), error)],
                    ));
                }
            }
            match state.backend.query(signal) {
                Ok(action) if B::is_handler(&action) => {
                    state.installed_policy[index] = Some(action)
                }
                Ok(_) => {
                    return Err(rollback(
                        state,
                        vec![SignalFailure::CompetingDisposition(signal)],
                    ));
                }
                Err(error) => {
                    return Err(rollback(
                        state,
                        vec![io_failure(
                            "read installed disposition",
                            Some(signal),
                            error,
                        )],
                    ));
                }
            }
        }
        if let Some(mask) = state.mask_to_restore {
            if let Err(error) = state.backend.restore_mask(&mask) {
                return Err(rollback(
                    state,
                    vec![io_failure("restore admission mask", None, error)],
                ));
            }
            state.mask_to_restore = None;
        }
        Ok(state)
    }

    #[cfg(test)]
    mod tests {
        use std::cell::RefCell;
        use std::rc::Rc;

        use super::*;

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        enum Handler {
            Default,
            Ours,
            Foreign,
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        struct Action {
            handler: Handler,
            flags: u8,
            mask: u64,
        }

        #[derive(Clone, Copy)]
        struct Mask(u64);

        struct FakeBackend {
            actions: [Action; 2],
            mask: Mask,
            changes: Rc<RefCell<Vec<&'static str>>>,
            restored: Rc<RefCell<Vec<(i32, Action)>>>,
            restored_masks: Rc<RefCell<Vec<u64>>>,
            fail_term_install: bool,
            fail_int_restore: bool,
            fail_mask_restore: bool,
        }

        impl FakeBackend {
            fn new() -> Self {
                Self {
                    actions: [Action {
                        handler: Handler::Default,
                        flags: 7,
                        mask: 0x80,
                    }; 2],
                    mask: Mask(0x100),
                    changes: Rc::new(RefCell::new(Vec::new())),
                    restored: Rc::new(RefCell::new(Vec::new())),
                    restored_masks: Rc::new(RefCell::new(Vec::new())),
                    fail_term_install: false,
                    fail_int_restore: false,
                    fail_mask_restore: false,
                }
            }

            fn index(signal: i32) -> usize {
                usize::from(signal == libc::SIGTERM)
            }
        }

        impl Backend for FakeBackend {
            type Action = Action;
            type Mask = Mask;

            fn block(&mut self) -> io::Result<Mask> {
                let old = self.mask;
                self.mask = Mask(old.0 | 0x03);
                Ok(old)
            }

            fn restore_mask(&mut self, mask: &Mask) -> io::Result<()> {
                if self.fail_mask_restore {
                    return Err(io::Error::from_raw_os_error(libc::EIO));
                }
                self.mask = *mask;
                self.restored_masks.borrow_mut().push(mask.0);
                Ok(())
            }

            fn query(&mut self, signal: i32) -> io::Result<Action> {
                Ok(self.actions[Self::index(signal)])
            }

            fn install(&mut self, signal: i32) -> io::Result<Action> {
                if signal == libc::SIGTERM && self.fail_term_install {
                    self.changes.borrow_mut().push("install_SIGTERM_failed");
                    return Err(io::Error::from_raw_os_error(libc::EIO));
                }
                self.changes.borrow_mut().push(if signal == libc::SIGINT {
                    "install_SIGINT"
                } else {
                    "install_SIGTERM"
                });
                let index = Self::index(signal);
                let old = self.actions[index];
                self.actions[index] = Action {
                    handler: Handler::Ours,
                    flags: 0,
                    mask: 0x03,
                };
                Ok(old)
            }

            fn restore(&mut self, signal: i32, action: &Action) -> io::Result<()> {
                if signal == libc::SIGINT && self.fail_int_restore {
                    return Err(io::Error::from_raw_os_error(libc::EIO));
                }
                self.changes.borrow_mut().push(if signal == libc::SIGINT {
                    "restore_SIGINT"
                } else {
                    "restore_SIGTERM"
                });
                self.actions[Self::index(signal)] = *action;
                self.restored.borrow_mut().push((signal, *action));
                Ok(())
            }

            fn is_default(action: &Action) -> bool {
                action.handler == Handler::Default
            }
            fn is_handler(action: &Action) -> bool {
                action.handler == Handler::Ours
            }
            fn same_policy(left: &Action, right: &Action) -> io::Result<bool> {
                Ok(left == right)
            }
        }

        #[test]
        fn d01_handler_only_latches_bit_and_observation_cancels() {
            PENDING.store(false, Ordering::Relaxed);
            capture_signal(libc::SIGTERM);
            assert!(PENDING.load(Ordering::Relaxed));
            let cancel = CancellationFlag::new();
            observe_pending(&PENDING, &cancel);
            assert!(cancel.is_cancelled());
            PENDING.store(false, Ordering::Relaxed);
        }

        #[test]
        fn d02_foreign_default_admission_never_installs() {
            let slot = AtomicBool::new(false);
            let pending = AtomicBool::new(false);
            let mut backend = FakeBackend::new();
            backend.actions[0].handler = Handler::Foreign;
            let changes = Rc::clone(&backend.changes);
            let error = begin_transaction(backend, &slot, &pending)
                .err()
                .expect("foreign policy refuses");
            assert!(matches!(
                error.failures.as_slice(),
                [SignalFailure::ForeignDisposition(libc::SIGINT)]
            ));
            assert!(error.recovery.is_none());
            assert!(changes.borrow().is_empty());
            assert!(!slot.load(Ordering::Acquire));
        }

        #[test]
        fn d03_second_install_failure_restores_first_exactly() {
            let slot = AtomicBool::new(false);
            let pending = AtomicBool::new(false);
            let mut backend = FakeBackend::new();
            backend.fail_term_install = true;
            let changes = Rc::clone(&backend.changes);
            let restored = Rc::clone(&backend.restored);
            let restored_masks = Rc::clone(&backend.restored_masks);
            let error = begin_transaction(backend, &slot, &pending)
                .err()
                .expect("TERM install refuses");
            assert_eq!(error.failures.len(), 1);
            assert!(error.recovery.is_none());
            assert_eq!(
                changes.borrow().as_slice(),
                ["install_SIGINT", "install_SIGTERM_failed", "restore_SIGINT"]
            );
            assert_eq!(
                restored.borrow().as_slice(),
                [(
                    libc::SIGINT,
                    Action {
                        handler: Handler::Default,
                        flags: 7,
                        mask: 0x80
                    }
                )]
            );
            assert_eq!(restored_masks.borrow().as_slice(), [0x100]);
            assert!(!slot.load(Ordering::Acquire));
        }

        #[test]
        fn failed_install_rollback_retains_slot_and_original_error_for_retry() {
            let slot = AtomicBool::new(false);
            let pending = AtomicBool::new(false);
            let mut backend = FakeBackend::new();
            backend.fail_term_install = true;
            backend.fail_int_restore = true;
            let mut error = begin_transaction(backend, &slot, &pending)
                .err()
                .expect("TERM install refuses");
            assert_eq!(error.failures.len(), 2);
            let mut recovery = error
                .recovery
                .take()
                .expect("failed rollback retains policy");
            assert!(slot.load(Ordering::Acquire));
            recovery.backend.fail_int_restore = false;
            recovery.finish().expect("explicit rollback retry");
            assert_eq!(
                recovery.backend.changes.borrow().as_slice(),
                ["install_SIGINT", "install_SIGTERM_failed", "restore_SIGINT"]
            );
            assert_eq!(
                recovery.backend.actions[0],
                Action {
                    handler: Handler::Default,
                    flags: 7,
                    mask: 0x80
                }
            );
            assert_eq!(recovery.backend.mask.0, 0x100);
            assert!(!slot.load(Ordering::Acquire));
        }

        #[test]
        fn d04_competing_restore_policy_is_not_overwritten() {
            let slot = AtomicBool::new(false);
            let pending = AtomicBool::new(false);
            let mut state =
                begin_transaction(FakeBackend::new(), &slot, &pending).expect("admitted defaults");
            let ours = state.backend.actions[0];
            let foreign = Action {
                handler: Handler::Foreign,
                flags: 3,
                mask: 0x200,
            };
            state.backend.actions[0] = foreign;
            let error = state.finish().expect_err("foreign policy retains custody");
            assert!(matches!(
                error.failures.as_slice(),
                [SignalFailure::CompetingDisposition(libc::SIGINT)]
            ));
            assert_eq!(state.backend.actions[0], foreign);
            assert!(slot.load(Ordering::Acquire));
            assert!(!state.complete);
            // Model the competing owner returning our policy; only then can the
            // same retained state finish. No real signal policy is changed here.
            state.backend.actions[0] = ours;
            state.finish().expect("explicit later recovery");
            assert!(!slot.load(Ordering::Acquire));
        }

        #[test]
        fn d07_repeated_signal_observation_retains_guard_and_changes_no_policy() {
            let slot = AtomicBool::new(false);
            let pending = AtomicBool::new(false);
            let mut state =
                begin_transaction(FakeBackend::new(), &slot, &pending).expect("admitted defaults");
            let changes = state.backend.changes.borrow().len();
            let cancel = CancellationFlag::new();
            pending.store(true, Ordering::Relaxed);
            observe_pending(&pending, &cancel);
            pending.store(true, Ordering::Relaxed);
            observe_pending(&pending, &cancel);
            assert!(cancel.is_cancelled());
            assert!(slot.load(Ordering::Acquire));
            assert_eq!(state.installed, [true, true]);
            assert_eq!(state.backend.changes.borrow().len(), changes);
            assert!(!state.complete);
            state
                .finish()
                .expect("closure is explicitly supplied by caller");
        }

        #[test]
        fn admission_slot_refuses_reentry_without_releasing_the_owner() {
            let slot = AtomicBool::new(true);
            let pending = AtomicBool::new(true);
            let error = begin_transaction(FakeBackend::new(), &slot, &pending)
                .err()
                .expect("slot is occupied");
            assert!(matches!(
                error.failures.as_slice(),
                [SignalFailure::AlreadyActive]
            ));
            assert!(slot.load(Ordering::Acquire));
            assert!(pending.load(Ordering::Relaxed));
        }

        #[test]
        fn mask_restore_failure_retains_exact_mask_for_retry() {
            let slot = AtomicBool::new(false);
            let pending = AtomicBool::new(false);
            let mut backend = FakeBackend::new();
            backend.fail_mask_restore = true;
            let mut error = begin_transaction(backend, &slot, &pending)
                .err()
                .expect("mask restore refuses");
            let mut recovery = error.recovery.take().expect("mask responsibility retained");
            assert_eq!(recovery.installed, [false, false]);
            assert_eq!(
                recovery.mask_to_restore.expect("original mask retained").0,
                0x100
            );
            assert!(slot.load(Ordering::Acquire));
            recovery.backend.fail_mask_restore = false;
            recovery.finish().expect("same-thread mask retry");
            assert_eq!(recovery.backend.mask.0, 0x100);
            assert!(!slot.load(Ordering::Acquire));
        }

        #[test]
        fn dropping_unfinished_model_never_releases_slot() {
            let slot = AtomicBool::new(false);
            let pending = AtomicBool::new(false);
            let state =
                begin_transaction(FakeBackend::new(), &slot, &pending).expect("admitted defaults");
            drop(state);
            assert!(slot.load(Ordering::Acquire));
        }
    }
}
