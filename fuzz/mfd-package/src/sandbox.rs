//! Detects host filesystem, process and network system calls made during an
//! import.
//!
//! On Linux x86_64, [`monitor`] installs a per-thread seccomp filter once.
//! The filter traps every system call in [`TRAPPED`], and every x32-ABI call,
//! into a `SIGSYS` handler, which counts the call while the thread is armed.
//! The handler then performs a trapped read-only call itself, tagged so the
//! filter lets it through. A call in [`DENIED`] (a filesystem change, mount,
//! `io_uring` or network call) made while armed fails with `EPERM` instead,
//! and an x32 call always fails with `EPERM`.
//!
//! Process creation and program execution are instead refused by the filter
//! itself, with a seccomp errno rather than a trap, so the decision holds
//! even when the caller has SIGSYS blocked, which `posix_spawn` does around
//! its clone, and never depends on the handler, which a created child would
//! reset to the default: `execve`/`execveat`, `fork`, `vfork` and a `clone`
//! without `CLONE_THREAD` fail with `EPERM`, and `clone3` with `ENOSYS` so
//! glibc retries through the refused legacy `clone`. A `clone` with
//! `CLONE_THREAD` is allowed, so the importer and libFuzzer keep creating
//! threads. These refusals are not counted; a count would need the handler,
//! which cannot run while SIGSYS is blocked.
//!
//! Opens are performed rather than denied so libFuzzer keeps working: a
//! crash, timeout or out-of-memory report raised during an import still
//! writes its artifact from the importing thread. Every open is still
//! counted, and a descriptor-based change such as `ftruncate` needs one. The
//! harness reports any counted call as a finding after the import returns.
//!
//! The handler does not count glibc's one-time read of [`ALLOCATOR_PROBE`],
//! which `free` performs the first time it shrinks a thread arena's heap.
//!
//! Monitors are serialized process-wide. Threads that an import spawns
//! inherit the filter but not the armed flag, so a trapped call on a thread
//! that never installed the filter is charged to the one armed monitor. On
//! other platforms [`monitor`] only runs the closure, [`SUPPORTED`] is
//! `false`, and [`warn_if_unsupported`] says so once.

/// Host access observed while one closure ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostAccess {
    /// Number of trapped system calls.
    pub calls: usize,
    /// Number of the first trapped system call, if any.
    pub first: Option<i64>,
}

impl HostAccess {
    pub fn none(&self) -> bool {
        self.calls == 0
    }
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub use linux::{
    ALLOCATOR_PROBE, DENIED, SUPPORTED, TRAPPED, X32_SYSCALL_BIT, monitor, syscall_name,
};

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
pub const SUPPORTED: bool = false;

/// Runs `f` without host-access detection on unsupported platforms.
#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
pub fn monitor<T>(f: impl FnOnce() -> T) -> (T, HostAccess) {
    (
        f(),
        HostAccess {
            calls: 0,
            first: None,
        },
    )
}

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
pub fn syscall_name(_number: i64) -> &'static str {
    "unknown"
}

/// Prints once per process that imports run without host-access detection,
/// so a run on an unsupported platform cannot pass for a checked one.
pub fn warn_if_unsupported() {
    static WARNED: std::sync::Once = std::sync::Once::new();
    if !SUPPORTED {
        WARNED.call_once(|| {
            eprintln!(
                "mfd-package-fuzz: host-I/O detection needs Linux x86_64; imports on this \
                 platform run unmonitored"
            );
        });
    }
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod linux {
    use std::cell::Cell;
    use std::collections::BTreeMap;
    use std::ffi::{c_int, c_void};
    use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
    use std::sync::{Mutex, Once, PoisonError};

    use seccompiler::{
        SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter,
        SeccompRule, TargetArch, sock_filter,
    };

    use super::HostAccess;

    pub const SUPPORTED: bool = true;

    /// Filesystem path, directory, process-image, mount and network calls.
    /// None of them takes a sixth argument, so the handler can tag its own
    /// re-issued call there.
    pub const TRAPPED: &[(i64, &str)] = &[
        (libc::SYS_open, "open"),
        (libc::SYS_openat, "openat"),
        (libc::SYS_openat2, "openat2"),
        (libc::SYS_creat, "creat"),
        (libc::SYS_open_by_handle_at, "open_by_handle_at"),
        (libc::SYS_name_to_handle_at, "name_to_handle_at"),
        (libc::SYS_stat, "stat"),
        (libc::SYS_lstat, "lstat"),
        (libc::SYS_newfstatat, "newfstatat"),
        (libc::SYS_statx, "statx"),
        (libc::SYS_statfs, "statfs"),
        (libc::SYS_access, "access"),
        (libc::SYS_faccessat, "faccessat"),
        (libc::SYS_faccessat2, "faccessat2"),
        (libc::SYS_readlink, "readlink"),
        (libc::SYS_readlinkat, "readlinkat"),
        (libc::SYS_getdents, "getdents"),
        (libc::SYS_getdents64, "getdents64"),
        (libc::SYS_getcwd, "getcwd"),
        (libc::SYS_chdir, "chdir"),
        (libc::SYS_chroot, "chroot"),
        (libc::SYS_getxattr, "getxattr"),
        (libc::SYS_lgetxattr, "lgetxattr"),
        (libc::SYS_listxattr, "listxattr"),
        (libc::SYS_llistxattr, "llistxattr"),
        (libc::SYS_setxattr, "setxattr"),
        (libc::SYS_lsetxattr, "lsetxattr"),
        (libc::SYS_removexattr, "removexattr"),
        (libc::SYS_lremovexattr, "lremovexattr"),
        (libc::SYS_inotify_add_watch, "inotify_add_watch"),
        (libc::SYS_fanotify_mark, "fanotify_mark"),
        (libc::SYS_mkdir, "mkdir"),
        (libc::SYS_mkdirat, "mkdirat"),
        (libc::SYS_mknod, "mknod"),
        (libc::SYS_mknodat, "mknodat"),
        (libc::SYS_rmdir, "rmdir"),
        (libc::SYS_unlink, "unlink"),
        (libc::SYS_unlinkat, "unlinkat"),
        (libc::SYS_rename, "rename"),
        (libc::SYS_renameat, "renameat"),
        (libc::SYS_renameat2, "renameat2"),
        (libc::SYS_link, "link"),
        (libc::SYS_linkat, "linkat"),
        (libc::SYS_symlink, "symlink"),
        (libc::SYS_symlinkat, "symlinkat"),
        (libc::SYS_truncate, "truncate"),
        (libc::SYS_chmod, "chmod"),
        (libc::SYS_fchmodat, "fchmodat"),
        (libc::SYS_fchmodat2, "fchmodat2"),
        (libc::SYS_chown, "chown"),
        (libc::SYS_lchown, "lchown"),
        (libc::SYS_fchownat, "fchownat"),
        (libc::SYS_utime, "utime"),
        (libc::SYS_utimes, "utimes"),
        (libc::SYS_futimesat, "futimesat"),
        (libc::SYS_utimensat, "utimensat"),
        (libc::SYS_mount, "mount"),
        (libc::SYS_umount2, "umount2"),
        (libc::SYS_pivot_root, "pivot_root"),
        (libc::SYS_mount_setattr, "mount_setattr"),
        (libc::SYS_open_tree, "open_tree"),
        (libc::SYS_move_mount, "move_mount"),
        (libc::SYS_fsopen, "fsopen"),
        (libc::SYS_fspick, "fspick"),
        // io_uring submits file and network operations that seccomp never
        // sees, so its setup is the only point to stop it.
        (libc::SYS_io_uring_setup, "io_uring_setup"),
        (libc::SYS_socket, "socket"),
        (libc::SYS_connect, "connect"),
    ];

    /// Trapped calls that fail with `EPERM` while armed instead of running:
    /// every filesystem change, program execution, mount, `io_uring` and
    /// network call. Opens stay performed (see the module documentation).
    pub const DENIED: &[i64] = &[
        libc::SYS_creat,
        libc::SYS_open_by_handle_at,
        libc::SYS_chroot,
        libc::SYS_setxattr,
        libc::SYS_lsetxattr,
        libc::SYS_removexattr,
        libc::SYS_lremovexattr,
        libc::SYS_mkdir,
        libc::SYS_mkdirat,
        libc::SYS_mknod,
        libc::SYS_mknodat,
        libc::SYS_rmdir,
        libc::SYS_unlink,
        libc::SYS_unlinkat,
        libc::SYS_rename,
        libc::SYS_renameat,
        libc::SYS_renameat2,
        libc::SYS_link,
        libc::SYS_linkat,
        libc::SYS_symlink,
        libc::SYS_symlinkat,
        libc::SYS_truncate,
        libc::SYS_chmod,
        libc::SYS_fchmodat,
        libc::SYS_fchmodat2,
        libc::SYS_chown,
        libc::SYS_lchown,
        libc::SYS_fchownat,
        libc::SYS_utime,
        libc::SYS_utimes,
        libc::SYS_futimesat,
        libc::SYS_utimensat,
        libc::SYS_mount,
        libc::SYS_umount2,
        libc::SYS_pivot_root,
        libc::SYS_mount_setattr,
        libc::SYS_open_tree,
        libc::SYS_move_mount,
        libc::SYS_fsopen,
        libc::SYS_fspick,
        libc::SYS_io_uring_setup,
        libc::SYS_socket,
        libc::SYS_connect,
    ];

    /// Set in the number of an x32-ABI call, which reaches the same kernel
    /// entry points under other numbers. The filter traps all of them.
    pub const X32_SYSCALL_BIT: i64 = 0x4000_0000;

    /// glibc's allocator opens this file once per process, the first time
    /// `free` shrinks the heap of a thread arena (`check_may_shrink_heap`),
    /// so an import that frees memory on the worker thread can open it. It
    /// is allocator state rather than an access the import asked for.
    pub const ALLOCATOR_PROBE: &[u8] = b"/proc/sys/vm/overcommit_memory\0";

    /// Sixth-argument tag of calls the handler re-issues ("ferrule!").
    const PASS: u64 = 0x6665_7272_756c_6521;

    /// Seccomp return actions and classic BPF opcodes for the prepended
    /// process-creation and execve decisions and the x32 check.
    const RET_TRAP: u32 = 0x0003_0000;
    const RET_ERRNO_EPERM: u32 = 0x0005_0000 | (libc::EPERM as u32);
    const RET_ERRNO_ENOSYS: u32 = 0x0005_0000 | (libc::ENOSYS as u32);
    const BPF_LD_W_ABS: u16 = 0x20;
    const BPF_ALU_AND_K: u16 = 0x54;
    const BPF_JEQ_K: u16 = 0x15;
    const BPF_JGE_K: u16 = 0x35;
    const BPF_RET_K: u16 = 0x06;

    /// Offsets into `seccomp_data`: the syscall number and the low word of
    /// its first argument (clone's flags).
    const NR_OFFSET: u32 = 0;
    const ARG0_LOW_OFFSET: u32 = 16;

    thread_local! {
        static INSTALLED: Cell<bool> = const { Cell::new(false) };
        static ARMED: Cell<bool> = const { Cell::new(false) };
        static CALLS: Cell<usize> = const { Cell::new(0) };
        static FIRST: Cell<i64> = const { Cell::new(-1) };
    }

    /// Held for the whole of each [`monitor`], so at most one thread is
    /// armed and foreign calls belong to its import.
    static MONITOR: Mutex<()> = Mutex::new(());
    static ARMED_THREADS: AtomicUsize = AtomicUsize::new(0);
    static FOREIGN_CALLS: AtomicUsize = AtomicUsize::new(0);
    static FOREIGN_FIRST: AtomicI64 = AtomicI64::new(-1);
    static HANDLER: Once = Once::new();

    pub fn syscall_name(number: i64) -> &'static str {
        if number & X32_SYSCALL_BIT != 0 {
            return "x32 system call";
        }
        TRAPPED
            .iter()
            .find(|(candidate, _)| *candidate == number)
            .map_or("unknown", |(_, name)| name)
    }

    /// Runs `f` on the current thread and returns the host access it made.
    ///
    /// Panics if the filter cannot be installed: the harness must not run
    /// imports without detection on a supported platform.
    pub fn monitor<T>(f: impl FnOnce() -> T) -> (T, HostAccess) {
        install();
        let _serial = MONITOR.lock().unwrap_or_else(PoisonError::into_inner);
        CALLS.with(|calls| calls.set(0));
        FIRST.with(|first| first.set(-1));
        FOREIGN_CALLS.store(0, Ordering::SeqCst);
        FOREIGN_FIRST.store(-1, Ordering::SeqCst);
        let value = {
            let _armed = Armed::new();
            f()
        };
        let foreign = FOREIGN_CALLS.load(Ordering::SeqCst);
        let own = CALLS.with(Cell::get);
        let first = match FIRST.with(Cell::get) {
            -1 if foreign > 0 => FOREIGN_FIRST.load(Ordering::SeqCst),
            number => number,
        };
        let access = HostAccess {
            calls: own + foreign,
            first: (first >= 0).then_some(first),
        };
        (value, access)
    }

    /// Disarms on drop, including when the monitored closure unwinds.
    struct Armed;

    impl Armed {
        fn new() -> Self {
            ARMED_THREADS.fetch_add(1, Ordering::SeqCst);
            ARMED.with(|armed| armed.set(true));
            Self
        }
    }

    impl Drop for Armed {
        fn drop(&mut self) {
            ARMED.with(|armed| armed.set(false));
            ARMED_THREADS.fetch_sub(1, Ordering::SeqCst);
        }
    }

    fn install() {
        if INSTALLED.with(Cell::get) {
            return;
        }
        HANDLER.call_once(|| {
            // SAFETY: the handler only touches const-initialized thread
            // locals without destructors, atomics and the saved registers.
            let installed = unsafe {
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = on_sigsys as *const () as usize;
                action.sa_flags = libc::SA_SIGINFO;
                libc::sigemptyset(&mut action.sa_mask);
                libc::sigaction(libc::SIGSYS, &action, std::ptr::null_mut())
            };
            assert_eq!(installed, 0, "could not install the SIGSYS handler");
        });
        let rules: BTreeMap<i64, Vec<SeccompRule>> = TRAPPED
            .iter()
            .map(|(number, _)| (*number, vec![untagged()]))
            .collect();
        let filter = SeccompFilter::new(
            rules,
            SeccompAction::Allow,
            SeccompAction::Trap,
            TargetArch::x86_64,
        )
        .unwrap_or_else(|error| panic!("seccomp filter must build: {error}"));
        let compiled: seccompiler::BpfProgram = filter
            .try_into()
            .unwrap_or_else(|error| panic!("seccomp filter must compile: {error}"));
        // Decide process creation and program execution directly, ahead of
        // the generated rules (whose jumps are relative and unaffected), with
        // a seccomp errno rather than a trap. The kernel applies it without a
        // signal, so it holds even while the caller has SIGSYS blocked, which
        // `posix_spawn` does around its clone, and it never depends on the
        // handler, which a created child would reset to the default:
        //
        // - execve/execveat fail with EPERM, so no program is ever run.
        // - fork, vfork and a clone without CLONE_THREAD fail with EPERM, so
        //   no new process is ever created; clone3 fails with ENOSYS so glibc
        //   retries through the legacy clone, which is refused the same way.
        // - a clone with CLONE_THREAD falls through to the generated rules,
        //   which do not trap it, so thread creation keeps working.
        // - every x32-ABI call traps, and the handler refuses it.
        //
        // Process creation is refused but not counted: a reliable count would
        // need the handler, which cannot run with SIGSYS blocked.
        let errno = |nr: i64, action: u32| {
            [
                stmt(BPF_LD_W_ABS, NR_OFFSET),
                jump(BPF_JEQ_K, nr as u32, 0, 1),
                stmt(BPF_RET_K, action),
            ]
        };
        let mut program = Vec::new();
        program.extend(errno(libc::SYS_execve, RET_ERRNO_EPERM));
        program.extend(errno(libc::SYS_execveat, RET_ERRNO_EPERM));
        program.extend(errno(libc::SYS_fork, RET_ERRNO_EPERM));
        program.extend(errno(libc::SYS_vfork, RET_ERRNO_EPERM));
        program.extend(errno(libc::SYS_clone3, RET_ERRNO_ENOSYS));
        // clone: refuse only when CLONE_THREAD is clear (a new process).
        program.extend([
            stmt(BPF_LD_W_ABS, NR_OFFSET),
            jump(BPF_JEQ_K, libc::SYS_clone as u32, 0, 4),
            stmt(BPF_LD_W_ABS, ARG0_LOW_OFFSET),
            stmt(BPF_ALU_AND_K, libc::CLONE_THREAD as u32),
            jump(BPF_JEQ_K, 0, 0, 1),
            stmt(BPF_RET_K, RET_ERRNO_EPERM),
        ]);
        // x32-ABI calls trap, and the handler refuses them.
        program.extend([
            stmt(BPF_LD_W_ABS, NR_OFFSET),
            jump(BPF_JGE_K, X32_SYSCALL_BIT as u32, 0, 1),
            stmt(BPF_RET_K, RET_TRAP),
        ]);
        program.extend(compiled);
        // SAFETY: setting no_new_privs only restricts this process.
        let no_new_privs = unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) };
        assert_eq!(no_new_privs, 0, "could not set no_new_privs");
        seccompiler::apply_filter(&program)
            .unwrap_or_else(|error| panic!("seccomp filter must apply: {error}"));
        INSTALLED.with(|installed| installed.set(true));
    }

    fn untagged() -> SeccompRule {
        let condition = SeccompCondition::new(5, SeccompCmpArgLen::Qword, SeccompCmpOp::Ne, PASS)
            .unwrap_or_else(|error| panic!("seccomp condition must build: {error}"));
        SeccompRule::new(vec![condition])
            .unwrap_or_else(|error| panic!("seccomp rule must build: {error}"))
    }

    fn stmt(code: u16, k: u32) -> sock_filter {
        sock_filter {
            code,
            jt: 0,
            jf: 0,
            k,
        }
    }

    fn jump(code: u16, k: u32, jt: u8, jf: u8) -> sock_filter {
        sock_filter { code, jt, jf, k }
    }

    extern "C" fn on_sigsys(_signal: c_int, _info: *mut libc::siginfo_t, context: *mut c_void) {
        // SAFETY: the kernel passes a valid ucontext for SA_SIGINFO handlers.
        let registers = unsafe { &mut (*context.cast::<libc::ucontext_t>()).uc_mcontext.gregs };
        // The kernel rolls the result register back to the call number.
        let number = registers[libc::REG_RAX as usize];
        let path = match number {
            libc::SYS_open => registers[libc::REG_RDI as usize],
            libc::SYS_openat => registers[libc::REG_RSI as usize],
            _ => 0,
        };
        // SAFETY: `path` is the path argument the caller passes to the
        // kernel. The comparison stops at the first differing byte, so it
        // never reads past the caller's terminating NUL.
        let allocator = path != 0
            && ALLOCATOR_PROBE
                .iter()
                .enumerate()
                .all(|(offset, byte)| unsafe { *(path as *const u8).add(offset) } == *byte);
        let own = ARMED.with(Cell::get) && !allocator;
        let foreign = !own
            && !allocator
            && !INSTALLED.with(Cell::get)
            && ARMED_THREADS.load(Ordering::SeqCst) > 0;
        if own {
            CALLS.with(|calls| calls.set(calls.get() + 1));
            FIRST.with(|first| {
                if first.get() < 0 {
                    first.set(number);
                }
            });
        } else if foreign {
            FOREIGN_CALLS.fetch_add(1, Ordering::SeqCst);
            let _ = FOREIGN_FIRST.compare_exchange(-1, number, Ordering::SeqCst, Ordering::SeqCst);
        }
        // An x32 call is never re-issued: the filter would trap the tagged
        // copy again, inside this handler.
        let x32 = number & X32_SYSCALL_BIT != 0;
        registers[libc::REG_RAX as usize] = if x32 || ((own || foreign) && DENIED.contains(&number))
        {
            -i64::from(libc::EPERM)
        } else {
            let arguments = [
                registers[libc::REG_RDI as usize],
                registers[libc::REG_RSI as usize],
                registers[libc::REG_RDX as usize],
                registers[libc::REG_R10 as usize],
                registers[libc::REG_R8 as usize],
            ];
            // SAFETY: re-issues the trapped call with its original arguments.
            unsafe { tagged_syscall(number, arguments) }
        };
    }

    /// Issues a raw system call whose unused sixth argument carries [`PASS`].
    unsafe fn tagged_syscall(number: i64, arguments: [i64; 5]) -> i64 {
        let result: i64;
        // SAFETY: the caller supplies the arguments of a trapped call, none
        // of which reads the sixth argument register.
        unsafe {
            std::arch::asm!(
                "syscall",
                inlateout("rax") number => result,
                in("rdi") arguments[0],
                in("rsi") arguments[1],
                in("rdx") arguments[2],
                in("r10") arguments[3],
                in("r8") arguments[4],
                in("r9") PASS,
                lateout("rcx") _,
                lateout("r11") _,
                options(nostack),
            );
        }
        result
    }
}
