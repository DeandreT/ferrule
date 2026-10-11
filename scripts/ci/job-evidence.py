#!/usr/bin/env python3
"""Create per-job scratch and durable unarchived metadata; never retire evidence."""
import contextlib
import json
import os
from pathlib import Path
import pwd
import re
import secrets
import stat
import sys
import time

DIRECTORY_FLAGS = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC
FILE_FLAGS = os.O_NOFOLLOW | os.O_CLOEXEC
MAX_RECORD = 16384
SCHEMA = "ferrule-ci-unarchived-v1"


def require(condition, message):
    if not condition:
        raise ValueError(message)


def identity(environment):
    values = {key: environment.get(key, "") for key in
              ("GITHUB_REPOSITORY", "GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT",
               "GITHUB_JOB", "GITHUB_SHA")}
    require(values["GITHUB_REPOSITORY"] == "DeandreT/ferrule", "Unexpected repository")
    for key in ("GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT"):
        require(re.fullmatch(r"[1-9][0-9]{0,19}", values[key]) is not None,
                "Invalid run identity")
    require(re.fullmatch(r"[A-Za-z0-9_-]{1,80}", values["GITHUB_JOB"]) is not None,
            "Invalid job identity")
    require(re.fullmatch(r"[0-9a-f]{40}", values["GITHUB_SHA"]) is not None,
            "Invalid source identity")
    profile = environment.get("FERRULE_CI_SUITE", "native")
    suite = environment.get("FERRULE_CI_EVIDENCE_SUITE", "native")
    require((profile, suite) in (("native", "native"),
            ("codegen", "codegen-crates"), ("codegen", "codegen-mappings")),
            "Invalid evidence suite")
    return {"repository": values["GITHUB_REPOSITORY"],
            "run_id": values["GITHUB_RUN_ID"], "attempt": values["GITHUB_RUN_ATTEMPT"],
            "job": values["GITHUB_JOB"], "source": values["GITHUB_SHA"], "suite": suite}


def directory_identity(fd):
    current = os.fstat(fd)
    require(stat.S_ISDIR(current.st_mode) and current.st_uid == os.getuid()
            and not current.st_mode & 0o022, "Unsafe owned directory")
    return {"device": current.st_dev, "inode": current.st_ino}


@contextlib.contextmanager
def owned_directory(path):
    path = Path(path)
    require(path.is_absolute() and str(path) == os.path.normpath(str(path))
            and path.resolve(strict=True) == path, "Noncanonical home")
    fd = os.open(path, DIRECTORY_FLAGS)
    try:
        directory_identity(fd)
        yield fd
    finally:
        os.close(fd)


@contextlib.contextmanager
def child_directory(parent, name, *, fresh=False):
    require(re.fullmatch(r"[A-Za-z0-9_.-]{1,240}", name) is not None
            and name not in (".", ".."), "Unsafe child name")
    if fresh:
        os.mkdir(name, 0o700, dir_fd=parent)
        os.fsync(parent)
    else:
        try:
            os.mkdir(name, 0o700, dir_fd=parent)
            os.fsync(parent)
        except FileExistsError:
            pass
    fd = os.open(name, DIRECTORY_FLAGS, dir_fd=parent)
    try:
        directory_identity(fd)
        # The existing ci-tmp parent may be 0755; all new jobs are private.
        if name != "ci-tmp":
            require(stat.S_IMODE(os.fstat(fd).st_mode) == 0o700,
                    "Job directories must be private")
        yield fd
    finally:
        os.close(fd)


def publish(directory, name, record):
    data = (json.dumps(record, ensure_ascii=True, sort_keys=True, indent=2) + "\n").encode("ascii")
    require(len(data) <= MAX_RECORD, "Metadata too large")
    fd = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | FILE_FLAGS,
                 0o600, dir_fd=directory)
    try:
        view = memoryview(data)
        while view:
            amount = os.write(fd, view)
            require(amount > 0, "Metadata write made no progress")
            view = view[amount:]
        os.fsync(fd)
        os.fchmod(fd, 0o444)
        os.fsync(fd)
    finally:
        os.close(fd)
    os.fsync(directory)


def read_record(directory, name):
    fd = os.open(name, os.O_RDONLY | os.O_NONBLOCK | FILE_FLAGS, dir_fd=directory)
    try:
        before = os.fstat(fd)
        require(stat.S_ISREG(before.st_mode) and before.st_uid == os.getuid()
                and before.st_nlink == 1 and stat.S_IMODE(before.st_mode) == 0o444
                and before.st_size <= MAX_RECORD, "Unsafe metadata record")
        data = bytearray()
        while True:
            chunk = os.read(fd, MAX_RECORD + 1 - len(data))
            if not chunk:
                break
            data.extend(chunk)
            require(len(data) <= MAX_RECORD, "Metadata exceeds bound")
        after = os.fstat(fd)
        fields = ("st_dev", "st_ino", "st_mode", "st_uid", "st_gid", "st_nlink",
                  "st_size", "st_mtime_ns", "st_ctime_ns")
        require(all(getattr(after, field) == getattr(before, field) for field in fields)
                and len(data) == before.st_size,
                "Metadata changed while reading")
        return json.loads(data)
    finally:
        os.close(fd)


def begin(home, environment, *, nonce=None):
    facts = identity(environment)  # Validate before creating any directory.
    nonce = secrets.token_hex(16) if nonce is None else nonce
    require(re.fullmatch(r"[0-9a-f]{32}", nonce) is not None, "Invalid nonce")
    job_id = "-".join((facts["run_id"], facts["attempt"], facts["job"], facts["suite"], nonce))
    root = Path(home) / "ci-tmp" / "jobs" / job_id
    with owned_directory(home) as home_fd, child_directory(home_fd, "ci-tmp") as tmp_parent, \
            child_directory(tmp_parent, "jobs") as jobs, \
            child_directory(jobs, job_id, fresh=True) as job:
        with child_directory(job, "tmp", fresh=True):
            pass
        with child_directory(job, "receipts", fresh=True) as receipts:
            base = {"schema": SCHEMA, "state": "UNARCHIVED", "closure": "UNQUALIFIED",
                    "archive_verified": False, "producer_tmpdir_compliance": "UNQUALIFIED",
                    "created_ns": time.time_ns(),
                    "identity": facts, "job_id": job_id, "nonce": nonce,
                    "job_directory": directory_identity(job),
                    "preparation": {"pid": os.getpid(), "uid": os.getuid(),
                                    "meaning": "preparation identity only; no producer closure"}}
            publish(receipts, "UNARCHIVED.json", base)
            # Only this whitelist can enter the optional provider canary.
            publish(receipts, "evidence-canary.json",
                    {key: base[key] for key in ("schema", "state", "closure", "archive_verified",
                     "created_ns", "identity", "job_id", "producer_tmpdir_compliance")} |
                    {"scope": "metadata-canary-only; not complete job evidence"})
    return {"TMPDIR": str(root / "tmp"), "FERRULE_CI_EVIDENCE_ROOT": str(root),
            "FERRULE_CI_EVIDENCE_ID": job_id,
            "FERRULE_CI_EVIDENCE_CANARY": str(root / "receipts" / "evidence-canary.json")}


def observe(home, environment):
    facts = identity(environment)
    job_id = environment.get("FERRULE_CI_EVIDENCE_ID", "")
    prefix = "-".join((facts["run_id"], facts["attempt"], facts["job"], facts["suite"])) + "-"
    require(job_id.startswith(prefix) and re.fullmatch(r"[0-9a-f]{32}", job_id[len(prefix):])
            is not None, "Mismatched evidence identity")
    expected_root = Path(home) / "ci-tmp" / "jobs" / job_id
    require(environment.get("FERRULE_CI_EVIDENCE_ROOT") == str(expected_root),
            "Mismatched evidence root")
    state = environment.get("FERRULE_CI_OBSERVED_JOB_STATE", "")
    canary = environment.get("FERRULE_CI_CANARY_OUTCOME", "") or "not_requested"
    require(state in ("success", "failure", "cancelled", "skipped"), "Invalid observed state")
    require(canary in ("success", "failure", "cancelled", "skipped", "not_requested"),
            "Invalid canary outcome")
    # Opening a missing root never creates a replacement job.
    with owned_directory(home) as home_fd:
        tmp_parent = os.open("ci-tmp", DIRECTORY_FLAGS, dir_fd=home_fd)
        try:
            jobs = os.open("jobs", DIRECTORY_FLAGS, dir_fd=tmp_parent)
            try:
                job = os.open(job_id, DIRECTORY_FLAGS, dir_fd=jobs)
                try:
                    directory_identity(tmp_parent)
                    require(stat.S_IMODE(os.fstat(jobs).st_mode) == 0o700,
                            "Unsafe jobs parent")
                    directory_identity(jobs)
                    job_pin = directory_identity(job)
                    require(stat.S_IMODE(os.fstat(job).st_mode) == 0o700, "Unsafe job root")
                    receipts = os.open("receipts", DIRECTORY_FLAGS, dir_fd=job)
                    try:
                        directory_identity(receipts)
                        require(stat.S_IMODE(os.fstat(receipts).st_mode) == 0o700,
                                "Unsafe receipts directory")
                        initial = read_record(receipts, "UNARCHIVED.json")
                        require(initial["schema"] == SCHEMA and initial["identity"] == facts
                                and initial["job_id"] == job_id
                                and initial["job_directory"] == job_pin
                                and initial["state"] == "UNARCHIVED"
                                and initial["archive_verified"] is False
                                and initial["closure"] == "UNQUALIFIED"
                                and initial["producer_tmpdir_compliance"] == "UNQUALIFIED",
                                "Changed preparation record")
                        publish(receipts, "observation-" + secrets.token_hex(16) + ".json",
                                {"schema": SCHEMA, "state": "UNARCHIVED", "closure": "UNQUALIFIED",
                                 "archive_verified": False, "producer_tmpdir_compliance": "UNQUALIFIED",
                                 "identity": facts, "job_id": job_id,
                                 "observed_ns": time.time_ns(), "job_state": state,
                                 "canary_outcome": canary,
                                 "meaning": "workflow observation; no descendant closure or archive proof"})
                    finally:
                        os.close(receipts)
                finally:
                    os.close(job)
            finally:
                os.close(jobs)
        finally:
            os.close(tmp_parent)


def main():
    require(len(sys.argv) == 2 and sys.argv[1] in ("begin", "observe"), "Expected begin or observe")
    require(os.environ.get("RUNNER_ENVIRONMENT") == "self-hosted"
            and pwd.getpwuid(os.getuid()).pw_name == "ferrule-runner"
            and os.environ.get("HOME") == "/home/ferrule-runner", "Unexpected runner account")
    if sys.argv[1] == "begin":
        settings = begin(Path("/home/ferrule-runner"), os.environ)
        for key, value in settings.items():
            print(key + "=" + value)
    else:
        observe(Path("/home/ferrule-runner"), os.environ)


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, KeyError, json.JSONDecodeError) as error:
        print("CI evidence preparation/observation refused: " + str(error), file=sys.stderr)
        sys.exit(1)
