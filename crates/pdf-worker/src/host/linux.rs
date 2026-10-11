use super::OwnedReapUsage;
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

pub fn errno() -> io::Error {
    io::Error::last_os_error()
}
pub fn ordinary_sigchld() -> io::Result<()> {
    let mut action = std::mem::MaybeUninit::<libc::sigaction>::uninit();
    // Valid writable action storage; query only, no disposition change.
    if unsafe { libc::sigaction(libc::SIGCHLD, std::ptr::null(), action.as_mut_ptr()) } != 0 {
        return Err(errno());
    }
    let action = unsafe { action.assume_init() };
    if action.sa_sigaction != libc::SIG_DFL || action.sa_flags & libc::SA_NOCLDWAIT != 0 {
        return Err(io::Error::other("exclusive ordinary SIGCHLD required"));
    }
    Ok(())
}
pub fn pidfd(pid: u32) -> io::Result<OwnedFd> {
    let raw = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::pid_t, 0u32) };
    if raw < 0 {
        Err(errno())
    } else {
        Ok(unsafe { OwnedFd::from_raw_fd(raw as i32) })
    }
}
pub fn signal(fd: &OwnedFd, signal: i32) -> io::Result<()> {
    let rc = unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            fd.as_raw_fd(),
            signal,
            std::ptr::null::<libc::siginfo_t>(),
            0u32,
        )
    };
    if rc < 0 { Err(errno()) } else { Ok(()) }
}
pub fn nonblocking(fd: i32) -> io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        Err(errno())
    } else {
        Ok(())
    }
}
#[derive(Debug)]
pub enum Wait {
    Pending,
    Reaped { status: i32, usage: OwnedReapUsage },
}
/// Sole reap authority. No Child wait/try_wait or second accounting waiter exists.
pub fn own_wait(pid: u32) -> io::Result<Wait> {
    wait_once(pid, |expected, status, usage| {
        let rc = unsafe { libc::wait4(expected, status, libc::WNOHANG, usage) };
        let error = if rc < 0 {
            errno().raw_os_error().unwrap_or(libc::EIO)
        } else {
            0
        };
        (rc, error)
    })
}
// FnOnce makes the one-attempt boundary explicit. Tests inject only the returned
// kernel fields; they cannot qualify actual wait/PIDfd behavior.
fn wait_once(
    pid: u32,
    invoke: impl FnOnce(libc::pid_t, &mut i32, &mut libc::rusage) -> (libc::pid_t, i32),
) -> io::Result<Wait> {
    let mut status = 0;
    let mut r: libc::rusage = unsafe { std::mem::zeroed() };
    let (rc, error) = invoke(pid as libc::pid_t, &mut status, &mut r);
    if rc == 0 || rc < 0 && error == libc::EINTR {
        return Ok(Wait::Pending);
    }
    if rc < 0 {
        return Err(io::Error::from_raw_os_error(error));
    }
    if rc != pid as libc::pid_t || !(libc::WIFEXITED(status) || libc::WIFSIGNALED(status)) {
        return Err(io::Error::other("unexpected exact-child wait status"));
    }
    Ok(Wait::Reaped {
        status,
        usage: OwnedReapUsage {
            user_seconds: r.ru_utime.tv_sec,
            user_microseconds: r.ru_utime.tv_usec,
            system_seconds: r.ru_stime.tv_sec,
            system_microseconds: r.ru_stime.tv_usec,
            maxrss_kib: r.ru_maxrss,
            raw_counters: [
                r.ru_ixrss,
                r.ru_idrss,
                r.ru_isrss,
                r.ru_minflt,
                r.ru_majflt,
                r.ru_nswap,
                r.ru_inblock,
                r.ru_oublock,
                r.ru_msgsnd,
                r.ru_msgrcv,
                r.ru_nsignals,
                r.ru_nvcsw,
                r.ru_nivcsw,
            ],
        },
    })
}
pub fn children(pid: u32) -> io::Result<Vec<u32>> {
    use std::io::Read;
    let mut text = String::new();
    File::open(format!("/proc/{pid}/task/{pid}/children"))?
        .take(4097)
        .read_to_string(&mut text)?;
    if text.len() > 4096 {
        return Err(io::Error::other("worker children cap"));
    }
    let children = text
        .split_whitespace()
        .map(|s| s.parse::<u32>().map_err(io::Error::other))
        .collect::<io::Result<Vec<_>>>()?;
    if children.len() > 32 || children.contains(&0) {
        return Err(io::Error::other("worker children identity"));
    }
    Ok(children)
}
pub fn birth(pid: u32) -> io::Result<(u64, u32)> {
    use std::io::Read;
    let mut bytes = String::new();
    File::open(format!("/proc/{pid}/stat"))?
        .take(4097)
        .read_to_string(&mut bytes)?;
    if bytes.len() > 4096 {
        return Err(io::Error::other("proc stat cap"));
    }
    let (_, tail) = bytes
        .rsplit_once(')')
        .ok_or_else(|| io::Error::other("proc stat format"))?;
    let fields = tail.split_whitespace().collect::<Vec<_>>();
    let parse = |i: usize| {
        fields
            .get(i)
            .ok_or_else(|| io::Error::other("proc stat fields"))?
            .parse::<u64>()
            .map_err(io::Error::other)
    };
    Ok((
        parse(19)?,
        u32::try_from(parse(1)?).map_err(io::Error::other)?,
    ))
}
pub fn setup_worker(parent: u32) -> io::Result<()> {
    if unsafe { libc::getppid() } != parent as i32 {
        return Err(io::Error::other("parent changed before policy"));
    }
    if unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0) } != 0 {
        return Err(errno());
    }
    if unsafe { libc::getppid() } != parent as i32 {
        return Err(io::Error::other("parent changed during policy"));
    }
    for (resource, soft, hard) in [
        (libc::RLIMIT_AS, 536870912, 536870912),
        (libc::RLIMIT_CPU, 30, 31),
        (libc::RLIMIT_FSIZE, 67108864, 67108864),
        (libc::RLIMIT_CORE, 0, 0),
    ] {
        let mut old = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        if unsafe { libc::getrlimit(resource, &mut old) } != 0 {
            return Err(errno());
        }
        if !inherited_admits(old.rlim_cur, old.rlim_max, soft, hard) {
            return Err(io::Error::other("cannot raise inherited limit"));
        }
        let new = libc::rlimit {
            rlim_cur: soft,
            rlim_max: hard,
        };
        if unsafe { libc::setrlimit(resource, &new) } != 0 {
            return Err(errno());
        }
        if unsafe { libc::getrlimit(resource, &mut old) } != 0 {
            return Err(errno());
        }
        if old.rlim_cur != soft || old.rlim_max != hard {
            return Err(io::Error::other("effective limit mismatch"));
        }
    }
    // The new worker needs only stdio and cwd; never inherit caller descriptors.
    if unsafe { libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, 0u32) } < 0 {
        return Err(errno());
    }
    Ok(())
}

fn inherited_admits(
    old_soft: libc::rlim_t,
    old_hard: libc::rlim_t,
    soft: libc::rlim_t,
    hard: libc::rlim_t,
) -> bool {
    old_soft >= soft && old_hard >= hard
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn b06_no_inherited_limit_raise() {
        for (soft, hard) in [
            (536870912, 536870912),
            (30, 31),
            (67108864, 67108864),
            (0, 0),
        ] {
            let equal = inherited_admits(soft, hard, soft, hard);
            eprintln!("limit equal original {soft} {hard} {equal}");
            assert!(equal);
            if soft > 0 {
                assert!(!inherited_admits(soft - 1, hard, soft, hard));
            }
            if hard > 0 {
                assert!(!inherited_admits(soft, hard - 1, soft, hard));
            }
        }
    }
    fn raw() -> libc::rusage {
        let mut r: libc::rusage = unsafe { std::mem::zeroed() };
        r.ru_utime.tv_sec = 1;
        r.ru_utime.tv_usec = 234567;
        r.ru_stime.tv_usec = 890123;
        r.ru_maxrss = 1234;
        r.ru_minflt = 12;
        r.ru_majflt = 2;
        r.ru_inblock = 3;
        r.ru_oublock = 4;
        r.ru_nvcsw = 5;
        r.ru_nivcsw = 6;
        r
    }
    #[test]
    fn d19_single_attempt_eintr_pending_then_own_reap() {
        let mut calls = 0;
        let mut reaped = false;
        for (rc, error) in [(-1, libc::EINTR), (0, 0), (201, 0)] {
            assert!(!reaped);
            let result = wait_once(201, |pid, status, r| {
                assert_eq!(pid, 201);
                calls += 1;
                *status = 0;
                *r = raw();
                (rc, error)
            });
            eprintln!("D19 original {result:#?}");
            match result.unwrap() {
                Wait::Pending => {}
                Wait::Reaped { status, usage } => {
                    reaped = true;
                    assert_eq!(status, 0);
                    assert_eq!(usage.maxrss_kib, 1234);
                    assert_eq!(usage.raw_counters, [0, 0, 0, 12, 2, 0, 3, 4, 0, 0, 0, 5, 6]);
                }
            }
        }
        // The owner guards subsequent polls with own_reap; no extra call occurs.
        if !reaped {
            let _ = wait_once(201, |_, _, _| {
                calls += 1;
                (0, 0)
            });
        }
        assert_eq!(calls, 3);
        assert!(reaped);
    }
    #[test]
    fn d20_raw_nonzero_usage_and_lost_authority_are_distinct() {
        let result = wait_once(202, |_, status, r| {
            *status = 1792;
            *r = raw();
            (202, 0)
        });
        eprintln!("D20 original {result:#?}");
        match result.unwrap() {
            Wait::Reaped { status, usage } => {
                assert_eq!(libc::WEXITSTATUS(status), 7);
                assert_eq!(
                    (
                        usage.user_seconds,
                        usage.user_microseconds,
                        usage.system_seconds,
                        usage.system_microseconds,
                        usage.maxrss_kib
                    ),
                    (1, 234567, 0, 890123, 1234)
                );
                assert_eq!(usage.raw_counters, [0, 0, 0, 12, 2, 0, 3, 4, 0, 0, 0, 5, 6]);
            }
            other => panic!("{other:#?}"),
        }
        let missing = wait_once(202, |_, _, _| (-1, libc::ECHILD));
        eprintln!("D18 original {missing:#?}");
        assert_eq!(missing.unwrap_err().raw_os_error(), Some(libc::ECHILD));
        assert!(
            wait_once(202, |_, status, _| {
                *status = 0;
                (203, 0)
            })
            .is_err()
        );
    }
}
