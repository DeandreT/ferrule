#!/usr/bin/env python3
"""Inactive lifecycle library. No live containment/provider adapter is installed.

Adapters are administrator-owned code, not JSON receipts or workflow inputs.
See docs/ci-evidence-lifecycle.md for the trust boundary and deployment gates.
"""
import contextlib
from dataclasses import dataclass, asdict
import fcntl
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import secrets
import stat
import sys
import tempfile
import time


def load(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(name + ".py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


compact = load("compact-job-evidence")
jobs = load("job-evidence")
require = jobs.require
GIB = 1024**3
MAX_MANIFEST = 64 * 1024**2
STATES = {"CLOSED_UNARCHIVED", "ARCHIVED_LOCAL", "PRESERVED", "RETIRING", "RETIRED"}


@dataclass(frozen=True)
class ObjectRef:
    provider: str
    destination: str
    key: str
    version: str
    sha256: str
    size: int


@dataclass(frozen=True)
class Retention:
    object: ObjectRef
    until_ns: int
    enforced: bool


class Containment:
    @contextlib.contextmanager
    def lease(self, preparation):
        """Exclude all producers, then prove descendant closure AND tmp compliance.

        Match the supplied preparation to the supervisor's own job identity,
        root inode and confinement policy. Keep exclusion until exit, including
        across all local mutations. A runner-writable receipt is not authority.
        A production implementation must consult its privileged supervisor; a
        process census or a workflow-supplied receipt is not this authority.
        """
        raise ValueError("Trusted producer containment and temporary-root attestation unavailable")
        yield  # Make the refusing default a context manager.


class Provider:
    """Trusted adapter: bounded exact-version transfer and enforced retention.

    put must be idempotent for key+digest, never overwrite a different object,
    and return an immutable provider version. read writes that exact version
    to the supplied file, with a hard maximum of ref.size bytes. retention
    queries provider-side enforcement, not an upload response or local claim.
    No implementation here treats a CI artifact or local copy as durable.
    """
    def put(self, key, source, digest, size):
        raise ValueError("Approved durable provider/destination/credentials unavailable")

    def read(self, ref, destination):
        raise ValueError("Approved durable provider unavailable")

    def retention(self, ref):
        raise ValueError("Provider retention authority unavailable")


def encoded(value):
    return (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode()


def digest_file(file):
    file.seek(0)
    digest = hashlib.file_digest(file, "sha256").hexdigest()
    size = file.tell()
    file.seek(0)
    return digest, size


def safe_member(path):
    require(isinstance(path, str) and (path == "." or (
        not path.startswith("/") and "\\" not in path and "\x00" not in path
        and all(part not in ("", ".", "..") for part in path.split("/")))),
        "Unsafe manifest member")


class BoundedWriter:
    """Sequential provider readback cannot exceed its exact declared size."""
    def __init__(self, destination, maximum):
        self.destination, self.remaining = destination, maximum

    def write(self, data):
        require(len(data) <= self.remaining, "Provider readback exceeds declared size")
        written = self.destination.write(data)
        self.remaining -= written
        return written


class Lifecycle:
    def __init__(self, home, state_root, *, containment=None, provider=None,
                 destination=None, retain_until_ns=None, reserve_gib=16,
                 available=None, runner_uid=None):
        self.home, self.state_root = Path(home), Path(state_root)
        self.containment = containment or Containment()
        self.provider = provider or Provider()
        # This policy comes from protected administrator configuration, never
        # from job metadata or the provider's own proposed retention deadline.
        self.destination = destination
        self.retain_until_ns = retain_until_ns
        self.runner_uid = os.getuid() if runner_uid is None else runner_uid
        require(type(self.runner_uid) is int and self.runner_uid >= 0, "Invalid runner owner")
        require(reserve_gib in (8, 16), "Unknown storage reserve")
        self.reserve_gib = reserve_gib
        self.available = available or (lambda: min(
            os.statvfs(path).f_bavail * os.statvfs(path).f_frsize
            for path in (self.home, self.state_root)))

    def _save(self, directory, name, record):
        data = encoded(record)
        require(len(data) <= MAX_MANIFEST, "Lifecycle state exceeds bound")
        pending = name + "." + secrets.token_hex(8) + ".pending"
        fd = os.open(pending, os.O_WRONLY | os.O_CREAT | os.O_EXCL | jobs.FILE_FLAGS,
                     0o600, dir_fd=directory)
        with os.fdopen(fd, "wb") as output:
            output.write(data)
            output.flush()
            os.fsync(output.fileno())
        os.replace(pending, name, src_dir_fd=directory, dst_dir_fd=directory)
        os.fsync(directory)

    def _read(self, directory, name):
        fd = os.open(name, compact.READ_FLAGS | os.O_NONBLOCK, dir_fd=directory)
        with os.fdopen(fd, "rb") as source:
            info = os.fstat(source.fileno())
            require(stat.S_ISREG(info.st_mode) and info.st_uid == os.getuid()
                    and info.st_nlink == 1 and stat.S_IMODE(info.st_mode) == 0o600
                    and info.st_size <= MAX_MANIFEST, "Unsafe lifecycle state")
            data = source.read(MAX_MANIFEST + 1)
            require(len(data) == info.st_size, "Lifecycle state changed")
            return json.loads(data)

    def _archive(self, root, state_fd, state, name):
        entries = compact.inventory(root, owner=self.runner_uid)
        require(all(not stat.S_ISLNK(item[2]) for item in entries.values()),
                "Symlink evidence needs a qualified preservation policy")
        size = sum(item[5] for item in entries.values() if stat.S_ISREG(item[2]))
        require(self.available() >= (self.reserve_gib + 4) * GIB + size * 2 + MAX_MANIFEST,
                "Archive staging would breach the reserve/headroom gate")
        archive_name = name + ".tar.gz"
        # A crash before the journal commits may leave this file. Only this
        # private controller-owned staging file is replaceable; never originals.
        pending = archive_name + "." + secrets.token_hex(8) + ".pending"
        fd = os.open(pending, os.O_RDWR | os.O_CREAT | os.O_EXCL | jobs.FILE_FLAGS,
                     0o600, dir_fd=state_fd)
        with os.fdopen(fd, "w+b") as output:
            members = compact.write_archive(root, entries, output)
            output.flush()
            os.fsync(output.fileno())
            output.seek(0)
            compact.verify_archive(output, members)
            digest, size = digest_file(output)
        require(compact.inventory(root, owner=self.runner_uid) == entries, "Evidence changed during archive")
        os.replace(pending, archive_name, src_dir_fd=state_fd, dst_dir_fd=state_fd)
        os.fsync(state_fd)
        manifest = {"schema": "ferrule-ci-lifecycle-archive-v1",
                    "preparation": state["preparation"], "archive_sha256": digest,
                    "archive_size": size, "members": members,
                    "entries": entries}
        self._save(state_fd, name + ".manifest.json", manifest)
        state.update(state="ARCHIVED_LOCAL", manifest=manifest)
        self._save(state_fd, name + ".json", state)

    def _preserve(self, state_fd, state, name):
        require(isinstance(self.destination, tuple) and len(self.destination) == 2
                and all(isinstance(value, str) and value for value in self.destination),
                "Approved provider and destination are required")
        require(type(self.retain_until_ns) is int and self.retain_until_ns > time.time_ns(),
                "An administrator-approved future retention deadline is required")
        manifest = state["manifest"]
        for path in manifest["members"]:
            safe_member(path)
        require(set(manifest["members"]) == set(manifest["entries"])
                and "." in manifest["entries"], "Incomplete owned manifest")
        # Never trust serialized provider receipts. Each retry reuploads by exact
        # idempotent key, reads the returned immutable version, and queries the
        # provider's retention authority for both bodies.
        refs = []
        for suffix in (".tar.gz", ".manifest.json"):
            fd = os.open(name + suffix, compact.READ_FLAGS | os.O_NONBLOCK, dir_fd=state_fd)
            with os.fdopen(fd, "rb") as source:
                info = os.fstat(source.fileno())
                require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1
                        and info.st_uid == os.getuid() and stat.S_IMODE(info.st_mode) == 0o600,
                        "Unsafe local preservation object")
                digest, size = digest_file(source)
                if suffix == ".tar.gz":
                    require((digest, size) == (manifest["archive_sha256"], manifest["archive_size"]),
                            "Local archive identity mismatch")
                    compact.verify_archive(source, manifest["members"])
                    source.seek(0)
                else:
                    require(source.read(MAX_MANIFEST + 1) == encoded(manifest),
                            "Local manifest identity mismatch")
                    source.seek(0)
                key = name + "/" + digest + suffix
                ref = self.provider.put(key, source, digest, size)
                require(type(ref) is ObjectRef and (ref.provider, ref.destination) == self.destination
                        and ref.key == key and bool(ref.version)
                        and (ref.sha256, ref.size) == (digest, size), "Provider object identity mismatch")
                # Anonymous controller-owned file; no provider-selected paths.
                require(self.available() >= (self.reserve_gib + 4) * GIB + size,
                        "Readback staging would breach the reserve/headroom gate")
                with tempfile.TemporaryFile(dir=self.state_root) as readback:
                    self.provider.read(ref, BoundedWriter(readback, size))
                    require(digest_file(readback) == (digest, size), "Provider readback mismatch")
                    if suffix == ".tar.gz":
                        compact.verify_archive(readback, manifest["members"])
                retention = self.provider.retention(ref)
                require(type(retention) is Retention and retention.object == ref
                        and retention.enforced is True
                        and type(retention.until_ns) is int
                        and retention.until_ns >= self.retain_until_ns,
                        "Durable retention is not enforced for the exact object version")
                refs.append(asdict(ref))
        return refs

    def _retire(self, parent, root, state_fd, state, name):
        expected = state["manifest"]["entries"]
        pin = os.stat(name, dir_fd=parent, follow_symlinks=False)
        require(compact.signature(pin)[:5] == tuple(expected["."])[:5], "Job root was replaced")
        current = compact.inventory(root, owner=self.runner_uid)
        # Before first unlink the original snapshot must match completely.
        # During recovery only removals are allowed; directory times/link counts
        # naturally change, but their inode/owner/mode identity must not.
        if state["state"] != "RETIRING":
            require(current == {key: tuple(value) for key, value in expected.items()},
                    "Original evidence no longer matches the owned manifest")
            state["state"] = "RETIRING"
            self._save(state_fd, name + ".json", state)
        require(set(current) <= set(expected), "Unexpected evidence during retirement")
        for path, actual in current.items():
            old = tuple(expected[path])
            require(actual[:5] == old[:5] if stat.S_ISDIR(actual[2]) else actual == old,
                    "Evidence changed during retirement recovery")
        # No recursive deletion or globbing: only manifest-named original inodes.
        for path in sorted(set(current) - {"."}, key=lambda p: (p.count("/"), p), reverse=True):
            safe_member(path)
            with compact.parent_directory(root, path) as directory:
                leaf = path.rsplit("/", 1)[-1]
                actual = os.stat(leaf, dir_fd=directory, follow_symlinks=False)
                old = tuple(expected[path])
                require(compact.signature(actual)[:5] == old[:5] if stat.S_ISDIR(actual.st_mode)
                        else compact.signature(actual) == old, "Retirement target was replaced")
                if stat.S_ISDIR(actual.st_mode):
                    os.rmdir(leaf, dir_fd=directory)
                else:
                    os.unlink(leaf, dir_fd=directory)
                os.fsync(directory)
        pin = os.stat(name, dir_fd=parent, follow_symlinks=False)
        require(compact.signature(pin)[:5] == tuple(expected["."])[:5], "Job root was replaced")
        os.rmdir(name, dir_fd=parent)
        os.fsync(parent)

    def run(self, job_id, *, retire=False):
        require(re.fullmatch(r"[A-Za-z0-9_.-]{1,240}", job_id) is not None
                and job_id not in (".", ".."), "Expected a prepared job ID")
        require(self.state_root != self.home and self.home not in self.state_root.parents,
                "Lifecycle state must be outside the runner home")
        with contextlib.ExitStack() as held:
            state_fd = held.enter_context(jobs.owned_directory(self.state_root))
            require(stat.S_IMODE(os.fstat(state_fd).st_mode) == 0o700, "State directory must be private")
            compact.reject_mounts(state_fd)
            lock = os.open(".lock", os.O_RDWR | os.O_CREAT | jobs.FILE_FLAGS, 0o600, dir_fd=state_fd)
            held.callback(os.close, lock)
            info = os.fstat(lock)
            require(stat.S_ISREG(info.st_mode) and info.st_uid == os.getuid()
                    and info.st_nlink == 1 and stat.S_IMODE(info.st_mode) == 0o600, "Unsafe lifecycle lock")
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            require(self.home.is_absolute() and self.home.resolve(strict=True) == self.home,
                    "Runner home must be canonical")
            home_fd = compact.owned_directory(self.home, owner=self.runner_uid)
            held.callback(os.close, home_fd)
            tmp = compact.owned_directory("ci-tmp", parent=home_fd, owner=self.runner_uid)
            held.callback(os.close, tmp)
            parent = compact.owned_directory("jobs", parent=tmp, owner=self.runner_uid)
            held.callback(os.close, parent)
            compact.reject_mounts(tmp, descendants=False)
            compact.reject_mounts(parent, descendants=False)
            try:
                state = self._read(state_fd, job_id + ".json")
                require(state["schema"] == "ferrule-ci-lifecycle-v1" and state["job_id"] == job_id
                        and state["state"] in STATES
                        and state["preparation"]["job_id"] == job_id, "Unowned lifecycle state")
            except FileNotFoundError:
                state = None
            # Lease receives retained identity even after partial deletion of the
            # original preparation receipt. State is protected supervisor data.
            if state is None:
                root = compact.owned_directory(job_id, parent=parent, owner=self.runner_uid)
                held.callback(os.close, root)
                preparation = jobs.retained_job(root, job_id, owner=self.runner_uid)
            else:
                preparation = state["preparation"]
                root = None
            held.enter_context(self.containment.lease(preparation))
            if state is None:
                state = {"schema": "ferrule-ci-lifecycle-v1", "job_id": job_id,
                         "state": "CLOSED_UNARCHIVED", "preparation": preparation}
                self._save(state_fd, job_id + ".json", state)
            elif state["state"] != "RETIRED":
                try:
                    root = compact.owned_directory(job_id, parent=parent, owner=self.runner_uid)
                    held.callback(os.close, root)
                except FileNotFoundError:
                    require(state["state"] == "RETIRING", "Missing unretired job root")
                if root is not None:
                    require(jobs.directory_identity(root, owner=self.runner_uid) == preparation["job_directory"],
                            "Job root identity mismatch")
                    if state["state"] != "RETIRING":
                        require(jobs.retained_job(root, job_id, owner=self.runner_uid) == preparation,
                                "Preparation changed")
            if state["state"] == "RETIRED":
                require(job_id not in os.listdir(parent), "Retired job root was recreated")
                return state["state"]
            if state["state"] == "CLOSED_UNARCHIVED":
                self._archive(root, state_fd, state, job_id)
            refs = self._preserve(state_fd, state, job_id)
            if state["state"] != "RETIRING":
                state.update(state="PRESERVED", objects=refs)
                self._save(state_fd, job_id + ".json", state)
            if retire:
                require(self.retain_until_ns > time.time_ns(),
                        "Approved retention deadline elapsed during preservation")
                if root is not None:
                    self._retire(parent, root, state_fd, state, job_id)
                state["state"] = "RETIRED"
                self._save(state_fd, job_id + ".json", state)
            return state["state"]


if __name__ == "__main__":
    print("Inactive: configure and qualify privileged containment, an approved durable "
          "destination, enforced retention, and least-privilege credentials. "
          "No live adapter or cleanup command is installed.", file=sys.stderr)
    sys.exit(2)
