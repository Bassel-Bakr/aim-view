"""Python's results for the mouse log reader's parity test (tests/mouse_parity.rs), in test_out/parity/mouse/<case>/:
the log (log.bin), the run's stats file, and want.json with what python/mouse_read.py prints and writes for them.

The cases: mouse_read.py's self-test run (the copy in test_out/mouse/selftest/, and new ones with other seeds), the
throttled copy of it, variations on it (no stop pair, a second device with absolute events, a kill with no press, a
stats file without its sensitivity or challenge start, other options, a run over midnight, a log that does not cover
the run, a file that is not a log), and copies of the real logs in test_out/mouse/. The originals are only read.

Usage: python tests/mouse_fixtures.py"""
import argparse
import contextlib
import io
import os
import json
import math
import random
import re
import statistics as st
import sys
import tempfile
from datetime import datetime, timedelta
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "python"))
import mouse_log as ml  # noqa: E402
import mouse_read as mr  # noqa: E402

OUT = ROOT / "test_out" / "parity" / "mouse"
MINE = ROOT / "test_out" / "mouse"
DEFAULTS = dict(dpi=None, cm360=None, window=4.0, start=30.0, stop=10.0, hold=5.0)


def opts(**kw):
    return argparse.Namespace(**{**DEFAULTS, **kw})


class NoJitter(random.Random):
    """The self-test's random source with a fixed handling delay (85 us)."""

    def uniform(self, a, b):
        return 85e-6 if a == 20e-6 else super().uniform(a, b)


def selftest_run(seed, source=random.Random):
    """mouse_read.py's self-test run with its random source seeded: (log bytes, stats name, stats bytes)."""
    keep_random, keep_file = mr.random.Random, mr.__file__
    with tempfile.TemporaryDirectory() as tmp:
        mr.random.Random = lambda _s: source(seed)
        mr.__file__ = str(Path(tmp) / "a" / "b.py")      # the self-test writes into <tmp>/test_out/mouse/selftest
        try:
            with contextlib.redirect_stdout(io.StringIO()):
                mr.selftest(opts())
        finally:
            mr.random.Random, mr.__file__ = keep_random, keep_file
        made = Path(tmp) / "test_out" / "mouse" / "selftest"
        stats = next(made.glob("*Stats.csv"))
        return (made / "synthetic.bin").read_bytes(), stats.name, stats.read_bytes()


def records(log):
    return [log[i:i + 24] for i in range(ml.HEADER.size, len(log) - (len(log) - ml.HEADER.size) % 24, 24)]


def without_stop(log):
    return log[:ml.HEADER.size] + b"".join(r for r in records(log) if ml.REC.unpack(r)[0] != ml.KIND_STOP)


def with_second_device(log):
    """A second device from a third of the way in: 40 events, every fourth one absolute."""
    recs = records(log)
    at = len(recs) // 3
    q = ml.REC.unpack(recs[at])[0]
    extra = [ml.DEV.pack(ml.KIND_DEVICE, 0x5678, 1, 0)]
    for i in range(40):
        flags = ml.MOUSE_MOVE_ABSOLUTE if i % 4 == 0 else 0
        extra.append(ml.REC.pack(q + i, 30000 + i if flags else (i % 3) - 1, 20000 if flags else 1, flags, 0, 0, 1))
    return log[:ml.HEADER.size] + b"".join(recs[:at] + extra + recs[at:])


def shift_wall(log, seconds):
    """The log with its wall clock pairs moved by whole seconds."""
    magic, version, size, freq, q0, ns0 = ml.HEADER.unpack_from(log, 0)
    out = [ml.HEADER.pack(magic, version, size, freq, q0, ns0 + seconds * 10 ** 9)]
    for r in records(log):
        k = ml.REC.unpack(r)[0]
        out.append(ml.STOP.pack(k, *ml.STOP.unpack(r)[1:2], ml.STOP.unpack(r)[2] + seconds * 10 ** 9)
                   if k == ml.KIND_STOP else r)
    return b"".join(out)


STAMP = re.compile(r"(\d{4})\.(\d{2})\.(\d{2})-(\d{2})\.(\d{2})\.(\d{2})")
CLOCK = re.compile(r"\b(\d{2}):(\d{2}):(\d{2})\.(\d{3})\b")


def shift_stats(name, data, seconds):
    """A stats file with its file name's time and every clock time in it moved by seconds (local time)."""
    end = datetime(*map(int, STAMP.search(name).groups()))
    new_end = end + timedelta(seconds=seconds)
    new_name = STAMP.sub(f"{new_end:%Y.%m.%d-%H.%M.%S}", name)

    def move(m):
        t = datetime.combine(end.date(), datetime.strptime(m.group(0), "%H:%M:%S.%f").time())
        return (t + timedelta(seconds=seconds)).strftime("%H:%M:%S.%f")[:-3]

    return new_name, CLOCK.sub(move, data.decode("utf-8")).encode("utf-8")


def edit_stats(data, drop=(), kill_shift=None):
    """A stats file without the "Key:," lines named in drop; with kill_shift (kill number, s) one kill's time moved."""
    lines = data.decode("utf-8").split("\n")
    out = []
    for line in lines:
        if any(line.startswith(k + ":,") for k in drop):
            continue
        if kill_shift and line.startswith(f"{kill_shift[0]},"):
            cells = line.split(",")
            t = datetime.strptime(cells[1], "%H:%M:%S.%f") + timedelta(seconds=kill_shift[1])
            cells[1] = t.strftime("%H:%M:%S.%f")[:-3]
            line = ",".join(cells)
        out.append(line)
    return "\n".join(out).encode("utf-8")


def python_results(folder, stats_name, o):
    """What mouse_read.py prints and writes for the case (run in its folder, so paths print as log.bin)."""
    try:
        log = ml.read_log("log.bin")
    except ValueError as e:
        return dict(stats=stats_name, options=vars(o), utc_offset=0, log_error=str(e))
    w0 = log["wall0"]
    want = dict(stats=stats_name, options=vars(o),
                utc_offset=round(datetime.fromtimestamp(w0).astimezone().utcoffset().total_seconds()))
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        mr.summary("log.bin", argparse.Namespace(**vars(o)))
    k = mr.deg_per_count(o.dpi or 1600, o.cm360 or 70)
    bins = {}
    for t, dx, dy, f in zip(log["t"], log["dx"], log["dy"], log["flags"]):
        if not f & ml.MOUSE_MOVE_ABSOLUTE:
            b = bins.setdefault(int(t * 1000), [0, 0])
            b[0] += dx
            b[1] += dy
    gaps = [b - a for a, b in zip(log["t"], log["t"][1:])]
    want["summary_text"] = buf.getvalue()
    want["summary"] = dict(travel_deg=sum(math.hypot(x, y) for x, y in bins.values()) * k, deg_per_count=k,
                           duration=log["duration"], drift_ms=log["drift_ms"], wall0=w0, events=len(log["t"]),
                           busiest_hz=ml.busiest_rate(log["t"]), median_interval=st.median(gaps) if gaps else None,
                           presses=len(mr.presses_of(log)))
    if stats_name is None:
        return want
    buf, run_o = io.StringIO(), argparse.Namespace(**vars(o))
    try:
        with contextlib.redirect_stdout(buf):
            res = mr.run("log.bin", stats_name, run_o)
    except SystemExit as e:
        want["error"] = str(e.code)
        return want
    text = buf.getvalue()
    want["run_text"] = text[:text.rindex("wrote ")]
    js = json.loads((folder / "log.kills.json").read_text(encoding="utf-8"))
    out = js["kills"]
    keys = ["reaction_ms", "flick_ms", "peak_dps", "stop_to_click_ms", "still_ms", "click_dps", "dist_deg"]
    spreads = []
    for key in keys:
        vals = [r[key] for r in out if r[key] is not None]
        if vals:
            spreads.append(dict(key=key, n=len(vals), p10=mr.q(vals, .1), median=st.median(vals), p90=mr.q(vals, .9)))
    want["run"] = dict(kills_json=js, offset_s=res["offset"], matched=res["matched"], spreads=spreads,
                       moving_clicks=sum(1 for r in out if r["still_ms"] == 0),
                       no_stop=sum(1 for r in out if r["stop_s"] is None),
                       corrected=sum(1 for r in out if r["corrections"]))
    return want


def write_case(name, log, stats=None, o=None):
    folder = OUT / name
    folder.mkdir(parents=True, exist_ok=True)
    (folder / "log.bin").write_bytes(log)
    if stats:
        (folder / stats[0]).write_bytes(stats[1])
    here = Path.cwd()
    os.chdir(folder)
    try:
        want = python_results(folder, stats[0] if stats else None, o or opts())
    finally:
        os.chdir(here)
    (folder / "want.json").write_text(json.dumps(want, indent=1), encoding="utf-8")
    kills = f"{len(want['run']['kills_json']['kills'])} kills" if "run" in want else "log only"
    print(f"{name}: {want.get('log_error') or want.get('error') or kills}")


def main():
    sel = MINE / "selftest"
    stats_name = "Selftest - Challenge - 2026.09.30-04.54.28 Stats.csv"
    base_log, base_stats = (sel / "synthetic.bin").read_bytes(), (sel / stats_name).read_bytes()
    base = (stats_name, base_stats)
    write_case("selftest", base_log, base)
    write_case("selftest_log_only", base_log)
    write_case("throttled", (sel / "throttled.bin").read_bytes(), base)
    for seed in (1, 2, 3, 4):
        log, name, data = selftest_run(seed)
        write_case(f"seed{seed}", log, (name, data))
    log, name, data = selftest_run(0, NoJitter)
    write_case("nojitter", log, (name, data))
    write_case("killed", without_stop(base_log), base)
    write_case("devices", with_second_device(base_log), base)
    write_case("devices_log_only", with_second_device(base_log))
    write_case("unmatched", base_log, (stats_name, edit_stats(base_stats, kill_shift=(5, 0.3))))
    write_case("nosens", base_log, (stats_name, edit_stats(base_stats, drop=("Sens Scale", "Challenge Start"))))
    write_case("options", base_log, base, opts(dpi=800.0, cm360=35.0, window=3.0, start=40.0, stop=8.0, hold=4.0))
    write_case("options_log_only", base_log, None, opts(dpi=400.0, cm360=50.0))
    # the run moved to start just before local midnight
    start = datetime(2026, 9, 30, 4, 54, 20)
    move = round(datetime(2026, 9, 29, 23, 59, 58).timestamp() - start.timestamp())
    write_case("midnight", shift_wall(base_log, move), shift_stats(stats_name, base_stats, move))
    write_case("uncovered", base_log, shift_stats(stats_name, base_stats, 7200))
    empty = b"Kill #,Timestamp,Bot\n\nChallenge Start:,04:54:21.000\n"
    write_case("no_kills", base_log, (stats_name, empty))
    write_case("not_a_log", base_stats, base)
    for real in sorted(MINE.glob("*.bin")):
        write_case(f"real_{real.stem}", real.read_bytes())
        write_case(f"real_{real.stem}_run", real.read_bytes(), base)


if __name__ == "__main__":
    main()
