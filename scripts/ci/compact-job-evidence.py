#!/usr/bin/env python3
"""Losslessly compact idle CI evidence; keep verified archives without expiry."""
import contextlib
import fcntl
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import stat
import tarfile
import time

GIB = 1024**3
DIRECTORY_FLAGS = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC
READ_FLAGS = os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC
WORKER = "/home/ferrule-runner/actions-runner/bin/Runner.Worker"


def job_records():
    spec = importlib.util.spec_from_file_location(
        "job_evidence", Path(__file__).with_name("job-evidence.py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def require(condition, message):
    if not condition:
        raise ValueError(message)


def hook_ancestry():
    ancestors = set()
    pid = os.getpid()
    worker_seen = False
    while pid:
        ancestors.add(pid)
        process = Path("/proc") / str(pid)
        # Container init/runuser ancestors belong to root and their executable
        # links may be unreadable. Only this account can be the job worker;
        # account-owned ancestors must remain fully inspectable.
        if process.stat().st_uid == os.getuid():
            executable = os.path.realpath(process / "exe")
            worker_seen |= executable == WORKER
        fields = (process / "stat").read_text().rsplit(")", 1)[1].split()
        pid = int(fields[1])
    require(worker_seen, "Evidence compaction requires the actual pre-job hook")
    return ancestors


def producer_closure(*, processes=None):
    """Require the dedicated account to have only the current hook's ancestry.

    This is a census of *all* account processes, not merely open evidence files.
    No earlier producer can survive to reopen a file after the census.
    """
    ancestors = hook_ancestry()
    processes = Path("/proc").iterdir() if processes is None else processes
    for process in processes:
        if not process.name.isdecimal() or int(process.name) in ancestors:
            continue
        try:
            require(process.stat().st_uid != os.getuid(),
                    f"Earlier runner process is still alive: {process.name}")
        except FileNotFoundError:
            pass


def signature(entry):
    return (entry.st_dev, entry.st_ino, entry.st_mode, entry.st_uid, entry.st_gid,
            entry.st_size, entry.st_mtime_ns, entry.st_ctime_ns, entry.st_nlink)


def owned_directory(path, *, parent=None):
    fd = os.open(path, DIRECTORY_FLAGS, dir_fd=parent)
    entry = os.fstat(fd)
    if entry.st_uid != os.getuid() or entry.st_mode & 0o022:
        os.close(fd)
        raise ValueError("Evidence directory has unsafe ownership or permissions")
    return fd


def reject_mounts(root, *, mountinfo=None):
    # Device checks alone miss bind mounts on the same filesystem.
    path = os.readlink(f"/proc/self/fd/{root}")
    mountinfo = (Path("/proc/self/mountinfo").read_text()
                 if mountinfo is None else mountinfo)
    for line in mountinfo.splitlines():
        point = line.split()[4]
        point = re.sub(r"\\([0-7]{3})", lambda match: chr(int(match[1], 8)), point)
        require(point != path and not point.startswith(path + "/"),
                "Mounted evidence requires inspection")


def inventory(root):
    """Snapshot a finite tree through directory descriptors without following links."""
    entries = {".": signature(os.fstat(root))}
    reject_mounts(root)
    device = os.fstat(root).st_dev

    def visit(directory, prefix):
        for name in sorted(os.listdir(directory)):
            path = prefix + name
            entry = os.stat(name, dir_fd=directory, follow_symlinks=False)
            require(entry.st_dev == device and entry.st_uid == os.getuid()
                    and (stat.S_ISLNK(entry.st_mode) or not entry.st_mode & 0o022),
                    "Evidence entry has foreign ownership, permissions or filesystem")
            require(stat.S_ISREG(entry.st_mode) or stat.S_ISDIR(entry.st_mode)
                    or stat.S_ISLNK(entry.st_mode), "Special evidence file requires inspection")
            # External hard links could permit a writer outside this tree.
            require(not stat.S_ISREG(entry.st_mode) or entry.st_nlink == 1,
                    "Hard-linked evidence requires inspection")
            entries[path] = signature(entry)
            require(len(entries) <= 100000, "Evidence inventory exceeds its bound")
            if stat.S_ISDIR(entry.st_mode):
                child = os.open(name, DIRECTORY_FLAGS, dir_fd=directory)
                try:
                    require(signature(os.fstat(child)) == signature(entry),
                            "Evidence directory changed during inventory")
                    visit(child, path + "/")
                finally:
                    os.close(child)
    visit(root, "")
    return entries


@contextlib.contextmanager
def parent_directory(root, path):
    fd = os.dup(root)
    try:
        for name in path.split("/")[:-1]:
            child = os.open(name, DIRECTORY_FLAGS, dir_fd=fd)
            os.close(fd)
            fd = child
        yield fd
    finally:
        os.close(fd)


class HashedReader:
    def __init__(self, source):
        self.source = source
        self.digest = hashlib.sha256()

    def read(self, size):
        data = self.source.read(size)
        self.digest.update(data)
        return data


def write_archive(root, entries, destination):
    records = {}
    with tarfile.open(fileobj=destination, mode="w:gz", compresslevel=1,
                      dereference=True) as archive:
        for path, expected in entries.items():
            with parent_directory(root, path) as parent:
                name = path.split("/")[-1]
                entry = os.stat(name, dir_fd=parent, follow_symlinks=False)
                require(signature(entry) == expected, "Evidence changed before archiving")
                info = tarfile.TarInfo(path)
                info.uid, info.gid = entry.st_uid, entry.st_gid
                info.mode = stat.S_IMODE(entry.st_mode)
                info.mtime = entry.st_mtime_ns / 10**9
                record = {"mode": info.mode, "uid": info.uid, "gid": info.gid,
                          "mtime_ns": entry.st_mtime_ns}
                if stat.S_ISDIR(entry.st_mode):
                    info.type = tarfile.DIRTYPE
                    record["kind"] = "directory"
                    archive.addfile(info)
                elif stat.S_ISLNK(entry.st_mode):
                    info.type = tarfile.SYMTYPE
                    info.linkname = os.readlink(name, dir_fd=parent)
                    record.update(kind="symlink", link=info.linkname)
                    archive.addfile(info)
                else:
                    record.update(kind="file", size=entry.st_size)
                    info.size = entry.st_size
                    with os.fdopen(os.open(name, READ_FLAGS, dir_fd=parent), "rb") as source:
                        require(signature(os.fstat(source.fileno())) == expected,
                                "Evidence file was redirected")
                        hashed = HashedReader(source)
                        archive.addfile(info, hashed)
                        require(signature(os.fstat(source.fileno())) == expected,
                                "Evidence changed while archiving")
                        record["sha256"] = hashed.digest.hexdigest()
                records[path] = record
    return records


def verify_archive(source, records):
    seen = set()
    with tarfile.open(fileobj=source, mode="r:gz") as archive:
        for member in archive:
            require(member.name in records and member.name not in seen,
                    "Archive member inventory mismatch")
            seen.add(member.name)
            expected = records[member.name]
            require((member.mode, member.uid, member.gid) ==
                    (expected["mode"], expected["uid"], expected["gid"])
                    and member.mtime == expected["mtime_ns"] / 10**9,
                    "Archive metadata mismatch")
            if expected["kind"] == "file":
                require(member.isfile() and member.size == expected["size"],
                        "Archive file metadata mismatch")
                digest = hashlib.sha256()
                with archive.extractfile(member) as body:
                    while data := body.read(1024 * 1024):
                        digest.update(data)
                require(digest.hexdigest() == expected["sha256"], "Archive body mismatch")
            elif expected["kind"] == "directory":
                require(member.isdir(), "Archive directory mismatch")
            else:
                require(member.issym() and member.linkname == expected["link"],
                        "Archive symlink mismatch")
    require(seen == set(records), "Archive is incomplete")


def compact_directory(scratch, archives, name, *, closed=producer_closure,
                      verify=verify_archive, jobs=False):
    require(re.fullmatch(r"[A-Za-z0-9_.-]{1,240}", name) is not None
            and name not in (".", ".."), "Unsafe evidence directory name")
    require(jobs or re.fullmatch(r"ferrule[-_][A-Za-z0-9_.-]{1,220}", name) is not None,
            "Only Ferrule test directories are eligible")
    require(shutil.rmtree.avoids_symlink_attacks, "Descriptor-safe cleanup unavailable")
    closed()
    with contextlib.ExitStack() as held:
        root = owned_directory(name, parent=scratch)
        held.callback(os.close, root)
        identity = os.fstat(root)
        require(identity.st_dev == os.fstat(scratch).st_dev,
                "Evidence directory uses another filesystem")
        records_helper = job_records() if jobs else None
        preparation = records_helper.retained_job(root, name) if jobs else None
        entries = inventory(root)
        # Keep archive filenames below NAME_MAX even for long test directory names.
        label = "ferrule-job-" + name if jobs else name
        stem = f"{label[:180]}-{time.time_ns()}-{secrets.token_hex(4)}"
        pending = stem + ".pending"
        destination = stem + ".tar.gz"
        flags = os.O_RDWR | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC
        fd = os.open(pending, flags, 0o600, dir_fd=archives)
        try:
            with os.fdopen(fd, "w+b") as output:
                records = write_archive(root, entries, output)
                output.flush()
                os.fsync(output.fileno())
                output.seek(0)
                verify(output, records)
                output.seek(0)
                digest = hashlib.file_digest(output, "sha256").hexdigest()
            # Recheck all producers and the exact original tree after full readback.
            closed()
            current = os.stat(name, dir_fd=scratch, follow_symlinks=False)
            require((current.st_dev, current.st_ino) == (identity.st_dev, identity.st_ino),
                    "Evidence root was replaced")
            require(inventory(root) == entries, "Evidence changed after archive readback")
            if jobs:
                require(records_helper.retained_job(root, name) == preparation,
                        "Preparation receipt changed after archive readback")
        except BaseException:
            os.unlink(pending, dir_fd=archives)
            raise
        os.rename(pending, destination, src_dir_fd=archives, dst_dir_fd=archives)
        original = "jobs/" + name if jobs else name
        manifest = {"schema": "ferrule-ci-local-archive-v1", "original_directory": original,
                    "archive": destination, "archive_sha256": digest, "members": records}
        metadata = os.open(stem + ".json", flags, 0o600, dir_fd=archives)
        with os.fdopen(metadata, "w") as output:
            json.dump(manifest, output, sort_keys=True)
            output.flush()
            os.fsync(output.fileno())
        os.fsync(archives)
        shutil.rmtree(name, dir_fd=scratch)
        os.fsync(scratch)
        print(json.dumps({"compacted_evidence": original, "archive": destination,
                          "members": len(records), "archive_sha256": digest}), flush=True)
        return manifest


def compact(home, goal_gib, *, available, closed=producer_closure):
    home = Path(home)
    require(home.is_absolute() and home.resolve(strict=True) == home,
            "Runner home must be canonical")
    require(goal_gib in (12, 20), "Unknown evidence compaction goal")
    if available(home) >= goal_gib * GIB:
        return []
    closed()
    with contextlib.ExitStack() as held:
        home_fd = owned_directory(home)
        held.callback(os.close, home_fd)
        scratch = owned_directory("ci-tmp", parent=home_fd)
        held.callback(os.close, scratch)
        try:
            os.mkdir("ci-evidence", 0o700, dir_fd=home_fd)
        except FileExistsError:
            pass
        archives = owned_directory("ci-evidence", parent=home_fd)
        held.callback(os.close, archives)
        require(os.fstat(archives).st_dev == os.fstat(scratch).st_dev,
                "Archives use another filesystem")
        flags = os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW | os.O_CLOEXEC
        lock = os.open(".compact-lock", flags, 0o600, dir_fd=archives)
        held.callback(os.close, lock)
        entry = os.fstat(lock)
        require(stat.S_ISREG(entry.st_mode) and entry.st_uid == os.getuid()
                and entry.st_nlink == 1 and not entry.st_mode & 0o022,
                "Unsafe evidence compaction lock")
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        candidates = []
        roots = [(scratch, False)]
        if "jobs" in os.listdir(scratch):
            jobs = owned_directory("jobs", parent=scratch)
            held.callback(os.close, jobs)
            require(stat.S_IMODE(os.fstat(jobs).st_mode) == 0o700,
                    "Unsafe jobs parent")
            require(os.fstat(jobs).st_dev == os.fstat(scratch).st_dev,
                    "Jobs parent uses another filesystem")
            reject_mounts(jobs)
            roots.append((jobs, True))
        records_helper = job_records() if len(roots) > 1 else None
        for parent, jobs in roots:
            for name in os.listdir(parent):
                if not jobs and re.fullmatch(r"ferrule[-_][A-Za-z0-9_.-]{1,220}", name) is None:
                    continue
                entry = os.stat(name, dir_fd=parent, follow_symlinks=False)
                if not stat.S_ISDIR(entry.st_mode):
                    continue
                try:
                    candidate = owned_directory(name, parent=parent)
                    try:
                        if jobs:
                            records_helper.retained_job(candidate, name)
                        size = sum(details[5] for details in inventory(candidate).values()
                                   if stat.S_ISREG(details[2]))
                    finally:
                        os.close(candidate)
                except (OSError, ValueError) as error:
                    # Some test fixtures deliberately contain FIFOs or other
                    # unusual entries. Preserve those trees and try eligible ones.
                    original = "jobs/" + name if jobs else name
                    print(json.dumps({"preserved_evidence": original, "reason": str(error)}),
                          flush=True)
                    continue
                # Tiny records stay directly accessible. Start with the largest
                # retained test trees so a pressure event needs few archives.
                if size >= 16 * 1024**2:
                    candidates.append((-size, entry.st_mtime_ns, name, parent, jobs))
        results = []
        for _, _, name, parent, jobs in sorted(candidates):
            if available(home) >= goal_gib * GIB:
                break
            closed()
            results.append(compact_directory(parent, archives, name, closed=closed, jobs=jobs))
        return results
