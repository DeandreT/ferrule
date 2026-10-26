#!/usr/bin/env python3
"""Self-authored inputs for the mapping-path desktop qualification.

Never supplies runtime nodes: those must be created and wired in Ferrule.
See docs/qualification/mapping-path-desktop-2026-10-25.md for the gestures.
"""

import argparse
import ctypes
import json
import os
from pathlib import Path
import signal
import shutil
import subprocess
import time


def schema(name, fields):
    return {
        "name": name,
        "repeating": False,
        "kind": {
            "kind": "group",
            "children": [
                {"name": field, "repeating": False,
                 "kind": {"kind": "scalar", "ty": "string"}}
                for field in fields
            ],
        },
    }


def empty_scope():
    return {"target_field": "", "source": None, "filter": None,
            "bindings": [], "children": []}


def format_options():
    return {"lenient_segments": False, "delimiter": None, "has_header_row": None}


def seed_project():
    target = schema("Output", ["Active", "Main", "Function", "Keep"])
    project = {
        "source": schema("Input", ["Keep"]),
        "target": target,
        "source_path": "input.json",
        "target_path": "primary.json",
        "source_options": format_options(),
        "target_options": format_options(),
        "extra_sources": [],
        "graph": {"nodes": {
            "0": {"kind": "source_field", "path": ["Keep"]},
            "1": {"kind": "user_function_call", "function": 7, "args": []},
            "2": {"kind": "const", "value": "independent audit target"},
        }},
        "root": empty_scope(),
        "extra_targets": [{
            "name": "audit", "path": "audit.json", "schema": target,
            "options": format_options(), "root": empty_scope(),
        }],
        "user_functions": {"7": {
            "library": "local", "name": "runtime_paths", "parameters": [],
            "output_name": "paths", "output_type": "string",
            "body": {"nodes": {
                "0": {"kind": "unconnected"},
                "1": {"kind": "const", "value": " | "},
                "2": {"kind": "unconnected"},
                "3": {"kind": "call", "function": "concat", "args": [0, 1, 2]},
            }},
            "output": 3,
        }},
    }
    return project


def prepare(directory):
    directory.mkdir(parents=True, exist_ok=False, mode=0o700)
    project = seed_project()
    (directory / "seed.json").write_text(json.dumps(project, indent=2) + "\n")
    (directory / "input.json").write_text('{"Keep":"self-authored desktop 108"}\n')


def verify(directory):
    """Compare complete originals, not just screenshots or selected fields."""
    if not __debug__:
        raise RuntimeError("Run the qualification without Python's -O option")
    expected = seed_project()
    for node, value in ((3, "mapping_file_path"), (4, "main_mapping_file_path"),
                        (5, "mapping_file_path"), (6, "main_mapping_file_path")):
        expected["graph"]["nodes"][str(node)] = {"kind": "runtime_value", "value": value}
    for scope, active, main, keep in (
        (expected["root"], 3, 4, 0), (expected["extra_targets"][0]["root"], 5, 6, 2),
    ):
        scope["bindings"] = [
            {"target_field": "Active", "node": active},
            {"target_field": "Main", "node": main},
            {"target_field": "Keep", "node": keep},
            {"target_field": "Function", "node": 1},
        ]
    body = expected["user_functions"]["7"]["body"]["nodes"]
    del body["0"], body["2"]
    body["3"]["args"] = [4, 1, 5]
    body["4"] = {"kind": "runtime_value", "value": "mapping_file_path"}
    body["5"] = {"kind": "runtime_value", "value": "main_mapping_file_path"}
    read = lambda name: json.loads((directory / name).read_text())
    assert read("saved.json") == expected, "Complete saved project differs"
    assert read("before-reopen.json") == expected, "Pre-reopen project differs"
    assert read("after-reopen.json") == expected, "Reopened project differs"
    for suffix in (".json", ".layout.json"):
        assert ((directory / ("before-reopen" + suffix)).read_bytes()
                == (directory / ("after-reopen" + suffix)).read_bytes()), suffix
    for stage in ("history-main", "history-audit", "history-function"):
        layout = lambda phase: read(stage + "-" + phase + ".layout.json")
        assert layout("before") == layout("undo"), stage + " undo"
        assert layout("after") == layout("redo"), stage + " redo"
        assert layout("before") != layout("after"), stage + " moved"
        assert len({(directory / (stage + "-" + phase + ".json")).read_bytes()
                    for phase in ("before", "undo", "redo")}) == 1, stage + " identity/wires"
    for canvas in ("main", "audit", "function"):
        for suffix in (".json", ".layout.json"):
            assert (read("lock-" + canvas + "-before" + suffix)
                    == read("lock-" + canvas + "-after" + suffix)), canvas + " lock" + suffix
    saved = str((directory / "saved.json").resolve())
    for target, keep in (("main", "self-authored desktop 108"),
                         ("audit", "independent audit target")):
        assert read(target + "-preview.json") == {
            "Active": saved, "Main": saved, "Function": saved + " | " + saved,
            "Keep": keep,
        }, target + " complete Preview"
    assert (directory / "desktop.exit").read_text().strip() == "0", "Desktop exit"
    remaining = [row for row in read("owned-processes.json")
                 if Path(f"/proc/{row['pid']}").exists()]
    assert not remaining, f"Owned processes still present: {remaining}"
    print("Complete project, history, locks, reopen, Preview, and process closure match.")


def desktop(directory, binary):
    """Run only on a newly allocated private X server and private D-Bus."""
    env = dict(os.environ)
    for key in (
        "DISPLAY", "WAYLAND_DISPLAY", "WAYLAND_SOCKET", "XDG_RUNTIME_DIR",
        "XDG_SESSION_TYPE", "DBUS_SESSION_BUS_ADDRESS",
    ):
        env.pop(key, None)
    for key, name in (
        ("XDG_CONFIG_HOME", "config"), ("XDG_CACHE_HOME", "cache"),
        ("XDG_DATA_HOME", "data"),
    ):
        path = directory / name
        path.mkdir(exist_ok=True, mode=0o700)
        env[key] = str(path)
    read_fd, write_fd = os.pipe()
    with (directory / "xvfb.log").open("wb") as log:
        server = subprocess.Popen(
            ["Xvfb", "-displayfd", str(write_fd), "-screen", "0", "1440x1000x24",
             "-nolisten", "tcp"],
            env=env, pass_fds=(write_fd,), stdout=log, stderr=log,
        )
    os.close(write_fd)
    try:
        with os.fdopen(read_fd) as stream:
            number = stream.readline().strip()
        if not number.isdecimal():
            raise RuntimeError("Private Xvfb did not allocate a display")
        env.update(DISPLAY=":" + number, GDK_BACKEND="x11")
        with (directory / "gui.log").open("wb") as log:
            app = subprocess.Popen(
                ["dbus-run-session", "--", str(binary)], cwd=directory,
                env=env, stdout=log, stderr=log, start_new_session=True,
            )
            (directory / "owner.json").write_text(json.dumps({
                "display": env["DISPLAY"], "xvfb": server.pid,
                "session": app.pid, "binary": str(binary),
            }, indent=2))
            try:
                result = app.wait()
            except BaseException:
                os.killpg(app.pid, signal.SIGTERM)
                app.wait(timeout=10)
                raise
    finally:
        server.terminate()
        server.wait(timeout=10)
    (directory / "desktop.exit").write_text(str(result) + "\n")
    return result


def action(directory, arguments):
    owner = json.loads((directory / "owner.json").read_text())
    # Refuse an old owner file rather than directing input at a reused display.
    if not Path(f"/proc/{owner['xvfb']}").exists():
        raise RuntimeError("The owned desktop is no longer running")
    env = dict(os.environ, DISPLAY=owner["display"])
    if arguments[0] == "close":
        retain_process_inventory(directory, owner)
        close_window(owner["display"], int(arguments[1], 0))
    elif arguments[0] == "snapshot":
        name = arguments[1]
        if Path(name).name != name:
            raise ValueError("Snapshot name must be a filename, not a path")
        for suffix in (".json", ".layout.json"):
            shutil.copyfile(directory / ("saved" + suffix), directory / (name + suffix))
        subprocess.run(["import", "-window", "root", str(directory / (name + ".png"))],
                       env=env, check=True)
    elif arguments[0] == "capture":
        destination = directory / (arguments[1] + ".png")
        subprocess.run(["import", "-window", "root", str(destination)], env=env, check=True)
    elif arguments[0] == "clipboard":
        data = subprocess.check_output(["xclip", "-selection", "clipboard", "-o"], env=env)
        (directory / arguments[1]).write_bytes(data)
    else:
        subprocess.run(["xdotool", *arguments], env=env, check=True)
        time.sleep(0.4)
    with (directory / "actions.jsonl").open("a") as log:
        log.write(json.dumps(arguments) + "\n")


def retain_process_inventory(directory, owner):
    rows = []
    for line in subprocess.check_output(
        ["ps", "-eo", "pid,ppid,pgid,comm"], text=True
    ).splitlines()[1:]:
        pid, parent, group, command = line.split(maxsplit=3)
        rows.append({"pid": int(pid), "ppid": int(parent),
                     "pgid": int(group), "command": command})
    owned = [row for row in rows
             if row["pgid"] == owner["session"] or row["pid"] == owner["xvfb"]]
    ids = {row["pid"] for row in owned}
    # Include helpers that start a new process group beneath our own session.
    while children := [row for row in rows if row["ppid"] in ids and row["pid"] not in ids]:
        owned.extend(children)
        ids.update(row["pid"] for row in children)
    (directory / "owned-processes.json").write_text(json.dumps(owned, indent=2) + "\n")


def close_window(display_name, window):
    """Send the ordinary WM close request; do not destroy the X window."""
    class ClientMessage(ctypes.Structure):
        _fields_ = [
            ("type", ctypes.c_int), ("serial", ctypes.c_ulong),
            ("send_event", ctypes.c_int), ("display", ctypes.c_void_p),
            ("window", ctypes.c_ulong), ("message_type", ctypes.c_ulong),
            ("format", ctypes.c_int), ("data", ctypes.c_long * 5),
        ]

    class Event(ctypes.Union):
        _fields_ = [("client", ClientMessage), ("padding", ctypes.c_long * 24)]

    x11 = ctypes.CDLL("libX11.so.6")
    x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
    x11.XOpenDisplay.restype = ctypes.c_void_p
    x11.XInternAtom.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_int]
    x11.XInternAtom.restype = ctypes.c_ulong
    x11.XSendEvent.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int,
                               ctypes.c_long, ctypes.POINTER(Event)]
    x11.XFlush.argtypes = [ctypes.c_void_p]
    x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
    display = x11.XOpenDisplay(display_name.encode())
    if not display:
        raise RuntimeError("Private X display is unavailable")
    try:
        event = Event()
        event.client.type = 33  # ClientMessage
        event.client.display = display
        event.client.window = window
        event.client.message_type = x11.XInternAtom(display, b"WM_PROTOCOLS", 0)
        event.client.format = 32
        event.client.data[0] = x11.XInternAtom(display, b"WM_DELETE_WINDOW", 0)
        if not x11.XSendEvent(display, window, 0, 0, ctypes.byref(event)):
            raise RuntimeError("Could not send the ordinary window-close request")
        x11.XFlush(display)
    finally:
        x11.XCloseDisplay(display)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("prepare", "desktop", "action", "verify"))
    parser.add_argument("directory", type=Path)
    parser.add_argument("arguments", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    directory = args.directory.resolve()
    if args.command == "desktop" and len(args.arguments) != 1:
        parser.error("desktop requires exactly one GUI executable path")
    if args.command == "action" and not args.arguments:
        parser.error("action requires an operation or xdotool arguments")
    if args.command == "prepare":
        prepare(directory)
    elif args.command == "desktop":
        raise SystemExit(desktop(directory, Path(args.arguments[0]).resolve()))
    elif args.command == "action":
        action(directory, args.arguments)
    else:
        verify(directory)


if __name__ == "__main__":
    main()
