"""Aim View's settings for this checkout and this computer, as src/local_config.rs reads them: aimview.defaults.json
(in git: the project's layout, and where KovaaK keeps its folders under Steam's) under aimview.json (out of git,
optional: this computer's own, such as the recordings' folder), both at the repo's root. No folder is written in the
scripts: they ask here. Steam's folder, unless named, is where Steam records it (the registry on Windows, else under
the home folder). Relative paths start at the repo's root; a setting that is null gives None.
"""
import json
import os
import sys
from functools import lru_cache
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DEFAULTS = ROOT / "aimview.defaults.json"
LOCAL = ROOT / "aimview.json"


@lru_cache(maxsize=1)
def settings():
    """The defaults with this computer's settings over them (each top-level key replaced whole)."""
    value = json.loads(DEFAULTS.read_text(encoding="utf-8"))
    if LOCAL.is_file():
        value.update(json.loads(LOCAL.read_text(encoding="utf-8")))
    return value


def folder(key):
    """A folder the settings name (data, models, ui, ffmpeg, vods), or None."""
    path = settings().get(key)
    return None if path is None else ROOT / path


def tool_port(name):
    """The port a Python tool listens on at 127.0.0.1 (tool_ports: detector_api, web_demo, label_check)."""
    return settings()["tool_ports"][name]


def repo_relative(path):
    """A path as a report records it: from the repo's root, with forward slashes, when it lies under the root (as the
    data folder's paths do); else as given."""
    full = Path(path)
    return full.relative_to(ROOT).as_posix() if full.is_absolute() and full.is_relative_to(ROOT) else str(path)


def steam():
    """Steam's folder: as named, else where Steam records it; None when neither has one."""
    named = folder("steam")
    if named is not None:
        return named
    found = settings()["steam_found"]
    if sys.platform == "win32":
        import winreg
        hive, _, key = found["registry_key"].partition("\\")
        try:
            with winreg.OpenKey(getattr(winreg, {"HKCU": "HKEY_CURRENT_USER", "HKLM": "HKEY_LOCAL_MACHINE"}[hive]),
                                key) as opened:
                return Path(winreg.QueryValueEx(opened, found["registry_value"])[0])
        except OSError:
            return None
    home = Path(os.path.expanduser("~")) / found["under_home"]
    return home if home.is_dir() else None


def kovaak(name):
    """One of KovaaK's folders (game, workshop, stats, scenarios, crosshairs), or None without Steam's folder."""
    base, layout = steam(), settings()["kovaak"]
    if base is None:
        return None
    if name in ("game", "workshop"):
        return base / layout[name]
    return base / layout["game"] / layout[name]


def required(path, what):
    """`path`, or a clear stop when the settings give none."""
    if path is None:
        sys.exit(f"no {what}: name it in {LOCAL.name} at the repo's root (see {DEFAULTS.name})")
    return path
