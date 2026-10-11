//! Test-only, launch-thread-local boundary faults. Never compiled into an
//! ordinary library or fixture executable; no fake wait status is returned.
use std::cell::RefCell;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[derive(Default)]
struct State {
    point: Option<(&'static str, &'static str)>,
    consumed: bool,
    defer_wait: bool,
    deferred_calls: usize,
    journal: Vec<String>,
    output: Option<PathBuf>,
    payload_count: usize,
}
thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }
pub(super) struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        STATE.with(|s| *s.borrow_mut() = State::default());
    }
}
pub(super) fn arm(point: Option<(&'static str, &'static str)>, output: &Path) -> Guard {
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        assert!(state.output.is_none(), "fault state must be disjoint");
        *state = State {
            point,
            output: Some(output.to_owned()),
            ..State::default()
        };
    });
    Guard
}
pub(super) fn probe(operation: &str, name: &str) -> io::Result<()> {
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        if state.output.is_none() {
            return Ok(());
        }
        state.journal.push(format!("{operation}:{name}"));
        if !state.consumed
            && state
                .point
                .is_some_and(|(op, file)| op == operation && (file == name || file == "*"))
        {
            state.consumed = true;
            state
                .journal
                .push("injected EIO before underlying operation".into());
            return Err(io::Error::from_raw_os_error(libc::EIO));
        }
        Ok(())
    })
}
pub(super) fn retained(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut f = OpenOptions::new().write(true).create_new(true).open(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    drop(f);
    File::open(
        path.parent()
            .ok_or_else(|| io::Error::other("original parent"))?,
    )?
    .sync_all()
}
pub(super) fn retain_payload(name: &str, bytes: &[u8]) -> io::Result<()> {
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        let Some(output) = state.output.clone() else {
            return Ok(());
        };
        state.payload_count += 1;
        let path = output.join(format!(
            "attempt-{:03}-{name}.original",
            state.payload_count
        ));
        retained(&path, bytes)?;
        state.journal.push(format!(
            "complete attempted payload retained:{name}:{}",
            bytes.len()
        ));
        Ok(())
    })
}
pub(super) fn set_deferred_wait(defer: bool) {
    STATE.with(|s| s.borrow_mut().defer_wait = defer);
}
pub(super) fn defer_wait() -> bool {
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        if state.defer_wait {
            state.deferred_calls += 1;
            state
                .journal
                .push("injected wait Pending; no wait4 call".into());
            true
        } else {
            false
        }
    })
}
pub(super) fn facts() -> (bool, usize, Vec<String>) {
    STATE.with(|s| {
        let state = s.borrow();
        (state.consumed, state.deferred_calls, state.journal.clone())
    })
}
