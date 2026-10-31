#![no_main]

use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{LazyLock, Mutex};

use libfuzzer_sys::{Corpus, fuzz_target};
use mfd_package_fuzz::{MAX_INPUT_BYTES, check, sandbox};

/// Checks run on one long-lived worker thread, which alone carries the
/// sandbox's seccomp filter. Processes spawned from a filtered thread inherit
/// the filter without its signal handler and die on their first trapped call,
/// so libFuzzer's own thread must stay unfiltered: sanitizers start
/// `llvm-symbolizer` from it to describe new coverage and crashes.
///
/// A panic on the worker still aborts through libFuzzer's panic hook, and
/// the opens and writes that save the crash artifact are performed (only
/// filesystem changes, execution and network calls are refused), so
/// findings keep their artifacts.
struct Worker {
    inputs: Mutex<SyncSender<Vec<u8>>>,
    done: Mutex<Receiver<()>>,
}

/// The main thread's default stack size, so stack-depth findings match a
/// single-threaded host.
const WORKER_STACK_BYTES: usize = 8 << 20;

static WORKER: LazyLock<Worker> = LazyLock::new(|| {
    sandbox::warn_if_unsupported();
    // If a filtered process is started anyway, writing to it must not kill
    // the fuzzer before it records the input.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    // SAFETY: ignoring SIGPIPE only turns broken-pipe writes into errors.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
    let (inputs, pending) = sync_channel::<Vec<u8>>(0);
    let (finished, done) = sync_channel(0);
    let spawned = std::thread::Builder::new()
        .name("mfd-package-check".into())
        .stack_size(WORKER_STACK_BYTES)
        .spawn(move || {
            for bytes in pending {
                check(&bytes);
                if finished.send(()).is_err() {
                    return;
                }
            }
        });
    if let Err(error) = spawned {
        panic!("could not spawn the check worker: {error}");
    }
    Worker {
        inputs: Mutex::new(inputs),
        done: Mutex::new(done),
    }
});

fuzz_target!(|bytes: &[u8]| -> Corpus {
    if bytes.len() > MAX_INPUT_BYTES {
        return Corpus::Reject;
    }
    let worker = &*WORKER;
    let (Ok(inputs), Ok(done)) = (worker.inputs.lock(), worker.done.lock()) else {
        panic!("check worker lock poisoned");
    };
    if inputs.send(bytes.to_vec()).is_err() || done.recv().is_err() {
        panic!("check worker stopped");
    }
    Corpus::Keep
});
