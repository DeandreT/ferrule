#!/usr/bin/env python3
"""Restore build headroom using only idle, reproducible Cargo target caches."""
import contextlib
import fcntl
import json
import os
from pathlib import Path
import pwd
import re
import stat
import sys

GIB = 1024**3
DIR_FLAGS = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC
FILE_FLAGS = os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW | os.O_CLOEXEC
TARGETS = ("generated-host-target", "workspace-target")
PROFILES = ("debug", "release")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def checked_directory(path):
    fd = os.open(path, DIR_FLAGS)
    current = os.fstat(fd)
    if (current.st_uid != os.getuid() or current.st_mode & 0o022):
        os.close(fd)
        raise ValueError("Cache directory has unsafe ownership or permissions")
    return fd


def mount_identity(directory):
    lines = Path(f"/proc/self/fdinfo/{directory}").read_text().splitlines()
    values = [line.removeprefix("mnt_id:").strip()
              for line in lines if line.startswith("mnt_id:")]
    require(len(values) == 1 and re.fullmatch(r"[0-9]+", values[0]) is not None,
            "Cache descriptor mount identity is unavailable")
    return int(values[0])


def locked_file(parent, name, mount):
    fd = os.open(name, FILE_FLAGS, 0o600, dir_fd=parent)
    try:
        current = os.fstat(fd)
        require(stat.S_ISREG(current.st_mode) and current.st_nlink == 1
                and current.st_uid == os.getuid() and not current.st_mode & 0o022,
                "Unsafe cache lock")
        require(mount_identity(fd) == mount, "Mounted cache tree requires inspection")
        fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        return fd
    except BaseException:
        os.close(fd)
        raise


def live_cache_users(roots, *, pids=None):
    """Veto executing hosts and open files as well as Cargo's build leases."""
    users = []
    processes = (Path("/proc").iterdir() if pids is None
                 else (Path("/proc") / str(pid) for pid in pids))
    for process in processes:
        if not process.name.isdecimal() or int(process.name) == os.getpid():
            continue
        try:
            if process.stat().st_uid != os.getuid():
                continue
            paths = []
            for leaf in ("cwd", "exe"):
                try:
                    paths.append(os.readlink(process / leaf))
                except FileNotFoundError:
                    pass
            for leaf in (process / "fd").iterdir():
                try:
                    paths.append(os.readlink(leaf))
                except FileNotFoundError:
                    pass
            paths.extend(line.split(maxsplit=5)[-1].strip()
                         for line in (process / "maps").read_text().splitlines()
                         if len(line.split(maxsplit=5)) == 6)
            if any(path == str(root) or path.startswith(str(root) + "/")
                   for root in roots for path in paths):
                users.append(int(process.name))
        except (FileNotFoundError, ProcessLookupError):
            continue
        # An unreadable live process makes retirement unsafe.
    require(not users, "Build caches have live users: " + ",".join(map(str, users)))


def headroom(home):
    space = os.statvfs(home)
    return space.f_bavail * space.f_frsize


def decode_mount_path(encoded):
    # mountinfo quotes these characters in its root and mount-point fields.
    require(encoded.startswith("/") and "\x00" not in encoded
            and "\\" not in re.sub(r"\\(?:040|011|012|134)", "", encoded),
            "Mount inventory is malformed")
    return re.sub(r"\\([0-7]{3})", lambda match: chr(int(match[1], 8)), encoded)


def check_mount_root(encoded, filesystem):
    # Namespace bind mounts have a kernel namespace/inode label, not a path.
    # Their absolute mount points still participate in every cache veto below.
    if filesystem == "nsfs" and re.fullmatch(
            r"(?:cgroup|ipc|mnt|net|pid|time|user|uts):\[[0-9]+\]", encoded):
        return
    decode_mount_path(encoded)


def reject_mounts(directory, *, mountinfo=None, descendants=True):
    """Veto mounted roots and descendants, including same-device bind mounts."""
    root = os.readlink(f"/proc/self/fd/{directory}")
    require(root.startswith("/") and not root.endswith(" (deleted)"),
            "Cache descriptor path is unavailable")
    if mountinfo is None:
        mountinfo = Path("/proc/self/mountinfo").read_text()
    require(isinstance(mountinfo, str) and mountinfo.strip(),
            "Mount inventory is unavailable")
    for line in mountinfo.splitlines():
        fields = line.split()
        require(len(fields) >= 10 and "-" in fields[6:],
                "Mount inventory is malformed")
        separator = fields.index("-", 6)
        require(len(fields) == separator + 4
                and re.fullmatch(r"[0-9]+", fields[0]) is not None
                and re.fullmatch(r"[0-9]+", fields[1]) is not None
                and re.fullmatch(r"[0-9]+:[0-9]+", fields[2]) is not None,
                "Mount inventory is malformed")
        check_mount_root(fields[3], fields[separator + 1])
        point = decode_mount_path(fields[4])
        require(point != root and not (descendants and point.startswith(root + "/")),
                "Mounted cache tree requires inspection")


def check_tree(directory, device, mount):
    """Refuse mounted/foreign directories and special files before any deletion."""
    require(mount_identity(directory) == mount, "Mounted cache tree requires inspection")
    for leaf in os.listdir(directory):
        entry = os.stat(leaf, dir_fd=directory, follow_symlinks=False)
        require(entry.st_dev == device and entry.st_uid == os.getuid(),
                "Cache entry has foreign ownership or filesystem")
        if stat.S_ISDIR(entry.st_mode):
            child = os.open(leaf, DIR_FLAGS, dir_fd=directory)
            try:
                current = os.fstat(child)
                require(current.st_dev == entry.st_dev and current.st_ino == entry.st_ino
                        and current.st_uid == os.getuid(),
                        "Cache directory changed during admission")
                check_tree(child, device, mount)
            finally:
                os.close(child)
        else:
            require(stat.S_ISREG(entry.st_mode) or stat.S_ISLNK(entry.st_mode),
                    "Special cache file requires inspection")


def remove_entry(directory, leaf, device, mount):
    """Visit only opened descendants on the held cache descriptor's mount."""
    require(mount_identity(directory) == mount, "Mounted cache tree requires inspection")
    entry = os.stat(leaf, dir_fd=directory, follow_symlinks=False)
    require(entry.st_dev == device and entry.st_uid == os.getuid(),
            "Cache entry has foreign ownership or filesystem")
    if stat.S_ISDIR(entry.st_mode):
        child = os.open(leaf, DIR_FLAGS, dir_fd=directory)
        try:
            current = os.fstat(child)
            require(current.st_dev == entry.st_dev and current.st_ino == entry.st_ino
                    and current.st_uid == os.getuid(),
                    "Cache directory changed during deletion")
            require(mount_identity(child) == mount, "Mounted cache tree requires inspection")
            for nested in sorted(os.listdir(child)):
                remove_entry(child, nested, device, mount)
        finally:
            os.close(child)
        os.rmdir(leaf, dir_fd=directory)
    else:
        require(stat.S_ISREG(entry.st_mode) or stat.S_ISLNK(entry.st_mode),
                "Special cache file requires inspection")
        # Unlink discards a symlink itself and refuses a mounted file.
        os.unlink(leaf, dir_fd=directory)


def reclaim(home, reserve_gib, *, available=headroom, active=live_cache_users,
            mounts=reject_mounts):
    home = Path(home)
    require(home.is_absolute() and home.resolve(strict=True) == home,
            "Runner home must be canonical")
    require(reserve_gib in (8, 16), "Unknown storage reserve")
    # Four GiB of headroom prevents admission directly on the hard floor.
    goal = (reserve_gib + 4) * GIB
    before = available(home)
    report = {"before_bytes": before, "floor_bytes": reserve_gib * GIB,
              "goal_bytes": goal, "reclaimed_targets": []}
    if before >= goal:
        report["after_bytes"] = before
        print(json.dumps(report), flush=True)
        return report
    cache = home / "ci-cache"
    require(cache.resolve(strict=True) == cache, "Cache root must be canonical")
    with contextlib.ExitStack() as held:
        cache_fd = checked_directory(cache)
        held.callback(os.close, cache_fd)
        cache_mount = mount_identity(cache_fd)
        mounts(cache_fd, descendants=False)
        maintenance = locked_file(cache_fd, ".reclaim-lock", cache_mount)
        held.callback(os.close, maintenance)
        plans = []
        roots = []
        # Acquire every lease before deleting anything. Preserve profile/lock
        # inodes so a waiting Cargo process cannot lock a replacement inode.
        for name in TARGETS:
            root = cache / name
            if not root.exists() and not root.is_symlink():
                continue
            require(root.resolve(strict=True) == root, "Target path is redirected")
            target_fd = checked_directory(root)
            held.callback(os.close, target_fd)
            require(os.fstat(target_fd).st_dev == os.fstat(cache_fd).st_dev,
                    "Target cache uses another filesystem")
            require(mount_identity(target_fd) == cache_mount,
                    "Mounted cache tree requires inspection")
            # A bind-mounted target can share both device and ownership. Veto
            # every selected root/subtree before creating its Cargo leases.
            mounts(target_fd)
            # Cargo and test harnesses may also retain tmp/timing/report roots.
            # Only known profile caches are eligible; every other entry stays.
            untouched = sorted(set(os.listdir(target_fd)) -
                               {*PROFILES, ".rustc_info.json", "CACHEDIR.TAG"})
            print(json.dumps({"target": name, "preserved_entries": untouched}), flush=True)
            roots.append(root)
            profiles = []
            for profile in PROFILES:
                if profile not in os.listdir(target_fd):
                    continue
                profile_fd = os.open(profile, DIR_FLAGS, dir_fd=target_fd)
                held.callback(os.close, profile_fd)
                current = os.fstat(profile_fd)
                require(current.st_uid == os.getuid() and not current.st_mode & 0o022,
                        "Unsafe Cargo profile")
                require(mount_identity(profile_fd) == cache_mount,
                        "Mounted cache tree requires inspection")
                lease = locked_file(profile_fd, ".cargo-lock", cache_mount)
                held.callback(os.close, lease)
                profiles.append(profile_fd)
            plans.append((name, target_fd, profiles))
        for _, target_fd, profiles in plans:
            mounts(target_fd)
            for profile_fd in profiles:
                check_tree(profile_fd, os.fstat(target_fd).st_dev, cache_mount)
        active(roots)
        # Admit all targets again after the process census, before trimming any.
        for _, target_fd, _ in plans:
            mounts(target_fd)
        for name, target_fd, profiles in plans:
            if available(home) >= goal:
                break
            mounts(target_fd)
            for profile_fd in profiles:
                for leaf in sorted(os.listdir(profile_fd)):
                    if leaf == ".cargo-lock":
                        continue
                    remove_entry(profile_fd, leaf, os.fstat(target_fd).st_dev, cache_mount)
                os.fsync(profile_fd)
            for leaf in (".rustc_info.json", "CACHEDIR.TAG"):
                try:
                    os.unlink(leaf, dir_fd=target_fd)
                except FileNotFoundError:
                    pass
            os.fsync(target_fd)
            report["reclaimed_targets"].append(name)
            print(json.dumps({"reclaimed_target": name,
                              "free_bytes": available(home)}), flush=True)
    report["after_bytes"] = available(home)
    print(json.dumps(report), flush=True)
    require(report["after_bytes"] >= reserve_gib * GIB,
            "Insufficient space after build-cache reclamation; retained evidence is preserved")
    return report


def main():
    require(len(sys.argv) == 2 and sys.argv[1] in ("8", "16"), "Expected storage reserve")
    require(os.environ.get("RUNNER_ENVIRONMENT") == "self-hosted"
            and pwd.getpwuid(os.getuid()).pw_name == "ferrule-runner"
            and os.environ.get("HOME") == "/home/ferrule-runner", "Unexpected runner account")
    reclaim(Path("/home/ferrule-runner"), int(sys.argv[1]))


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError) as error:
        print("Build-cache space admission refused: " + str(error), file=sys.stderr)
        sys.exit(1)
