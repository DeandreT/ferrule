# Linux desktop metadata

Ferrule provides desktop-entry and AppStream metadata for the existing native
GUI executable. These files are metadata only: they do not produce a binary
release, install system packages, register file associations, or change the
desktop environment.

## Build and validate

From the repository root, build the GUI using the pinned toolchain:

```sh
cargo build --locked --release -p gui
```

Validate the checked-in metadata with the standard desktop and AppStream
validators:

```sh
desktop-file-validate packaging/linux/io.github.DeandreT.Ferrule.desktop
appstreamcli validate --pedantic packaging/linux/io.github.DeandreT.Ferrule.metainfo.xml
```

The desktop entry uses `ferrule-gui`, the executable name declared by the GUI
crate. It intentionally declares no `MimeType`, desktop actions, or command
line arguments: the GUI does not currently expose a file-opening CLI contract.
The command-line tool is a separate executable.

## Manual user-local staging

The following commands install files only in the current user's home directory.
They do not use a package manager, modify system files, alter shell profiles, or
change default applications. They require Python 3 for desktop-entry escaping.
Run them from the repository root after building:

```sh
install -Dm755 target/release/ferrule-gui "$HOME/.local/bin/ferrule-gui"
install -Dm644 packaging/linux/io.github.DeandreT.Ferrule.svg "$HOME/.local/share/icons/hicolor/scalable/apps/io.github.DeandreT.Ferrule.svg"
install -Dm644 packaging/linux/io.github.DeandreT.Ferrule.metainfo.xml "$HOME/.local/share/metainfo/io.github.DeandreT.Ferrule.metainfo.xml"
desktop_entry=$(mktemp)
trap 'rm -f "$desktop_entry"' EXIT
python3 - "$desktop_entry" <<'PY'
import os
import pathlib
import sys

def desktop_string(value):
    # Escape the desktop-entry string layer (also used by TryExec).
    return (value.replace("\\", "\\\\").replace("\n", "\\n")
            .replace("\r", "\\r").replace("\t", "\\t").replace(" ", "\\s"))

executable = os.path.join(os.environ["HOME"], ".local/bin/ferrule-gui")
if not os.path.isabs(executable):
    raise SystemExit("HOME must be an absolute path")
# Exec has an additional quoting layer and literal percent signs must be doubled.
quoted = "".join("\\" + char if char in '\\"`$' else char for char in executable)
# GLib checks the executable token before expanding %% field escapes. Use env
# as that token so a literal percent in HOME still works on GLib desktops.
exec_value = '/usr/bin/env -- ' + desktop_string('"' + quoted.replace("%", "%%") + '"')
source = pathlib.Path("packaging/linux/io.github.DeandreT.Ferrule.desktop")
entry = source.read_text(encoding="utf-8")
entry = entry.replace("Exec=ferrule-gui\n", "Exec=" + exec_value + "\n", 1)
entry = entry.replace("TryExec=ferrule-gui\n", "TryExec=" + desktop_string(executable) + "\n", 1)
pathlib.Path(sys.argv[1]).write_text(entry, encoding="utf-8")
PY
install -Dm644 "$desktop_entry" "$HOME/.local/share/applications/io.github.DeandreT.Ferrule.desktop"
desktop-file-validate "$HOME/.local/share/applications/io.github.DeandreT.Ferrule.desktop"
rm "$desktop_entry"
trap - EXIT
```

The staged desktop entry uses the absolute path to the user-local executable in
both `Exec` and `TryExec`, so visibility and launching do not depend on the desktop
session's `PATH`. Paths containing spaces, quotes, backslashes, percent signs,
and other desktop-entry metacharacters are escaped. `Exec` invokes the absolute
path through `/usr/bin/env --` (required on the Linux host), avoiding GLib's
pre-expansion executable lookup for paths containing literal percent signs.
The SVG embeds its Lucide
copyright and ISC license notice, retaining them in the installed icon. The temporary
file is uniquely created and removed after installation. To uninstall this
manual staging, remove only the four listed files from `~/.local/bin`,
`~/.local/share/icons/hicolor/scalable/apps`,
`~/.local/share/metainfo`, and `~/.local/share/applications` respectively.
