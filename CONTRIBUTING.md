# Contributing to ferrule

Work happens in scoped GitHub issues called lanes. GitHub issues are the only
live source for claims and status; this repository keeps no separate status list.

## 1. Find an unclaimed lane

- Start from a coordination issue's lane index, such as
  [#1090](https://github.com/DeandreT/ferrule/issues/1090), or search open issues
  without an assignee. `help wanted` marks issues suited to outside contributors.
- Open the issue itself before choosing it. A lane index describes scope; the
  issue's state, assignee and comments show whether it is still available.
- Each lane lists its outcome, reserved paths, exclusions and acceptance criteria.

```sh
gh issue list --state open --label "help wanted" --search "no:assignee"
```

## 2. Claim it

Comment on the issue with the files you plan to change and any shared resource
you need, then get assigned before editing. If you cannot assign yourself, the
maintainer assigns you in response to your comment. Report a newly discovered
gap as its own issue instead of widening your lane. See
[Parallel work](docs/parallel-work.md#claim-a-lane) for the full procedure.

## 3. Work in your own branch and worktree

```sh
git fetch origin
git worktree add ../ferrule-<topic> -b <type>/<topic> origin/main
```

Never work in another contributor's checkout or branch. Stay inside the issue's
reserved paths. Shared entry points such as workspace manifests, module
registrations, generated references and CI definitions need an agreed handoff in
the issue first; see [Shared files and handoffs](docs/parallel-work.md#shared-files-and-handoffs).

## 4. Use clean-room, self-authored fixtures

Write every fixture yourself from format knowledge and the behavior you are
testing. Do not commit proprietary or licensed samples, vendor installation
files, credentials, private evidence or output copied from another product.
Another application may serve only as a black-box behavioral reference, and
none of its files enter the repository. Prefer fixtures built in the test over
checked-in binaries.

## 5. Verify locally before review

Run the checks for your change on the head you will push; the
[review checklist](docs/development.md#before-requesting-review) maps them to
the CI workflows. A Rust change needs at least:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
```

plus the workspace nextest and doctest runs. Add the wasm web-demo build and the
generated-code suites when your change reaches them. Keep implementation, local
verification and external qualification distinct.

## 6. Open a concise pull request

- Use a conventional commit subject, such as `fix(format-csv): ...`.
- Link the lane issue. Summarize the changes; leave out test commands, results
  and pipeline status. Mention a limitation only when it affects review.
- Keep one bounded result per PR. Record qualification evidence and remaining
  acceptance checks in the issue.
- If you pause or stop, say so in the issue so another contributor can take it.
