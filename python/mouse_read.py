"""Read a raw mouse log from python/mouse_log.py and measure each flick of a KovaaK's run from it (see "Raw mouse log"
in python/README.md).

Counts become degrees with 360 / (cm360 / 2.54 * dpi). Positive x is right; raw input's positive y is down. In run
mode the dpi and cm/360 come from the stats file when it uses cm/360; --dpi and --cm360 override it.

Summary mode (a log only) prints the duration, the event rate (the median interval and the rate it implies, and the
busiest 100 ms), the total travel and the left-button presses.

Run mode (--stats) matches the run's kills with the left-button presses. It searches one clock offset between the
stats file and the log, within 1 s, that puts the most kills within 10 ms of a press, and takes each kill's press as
the nearest one within 10 ms of it. Presses in the run that killed nothing are misses. Then, for each kill, from the
previous kill's press to this one, it measures the speed on a 0.25 ms grid, each over a centered window (4 ms by
default; never past the click):
  start   when the mouse starts moving: the first time the speed rises to --start (30 deg/s) or more. If it was
          still moving that fast at the previous press, the start is when it rises again after dropping under
          --stop, or that press if it never dropped that low.
  peak    the highest speed after the start.
  stop    when the mouse stops: the first time after the peak that the speed stays under --stop (10 deg/s) for
          --hold (5 ms), or until the click if that comes sooner.
  still   how long the speed stayed under --stop right before the click: the time the crosshair sat still on
          the target. 0 when the click came while moving.
It prints the median, p10 and p90 of each and writes them per kill to <log name>.kills.json next to the log.
Between events the position is interpolated, so a report never counts as all in or all out of a window.

Precision: a 4 ms window holds about 32 reports at 8000 Hz, and at 1600 dpi and 70 cm/360 one count in it is about
2 deg/s. On the self-test's synthetic runs (8000 Hz, 0.02 to 0.15 ms of handling delay) the start, stop, still and
click times come out within 1 ms, and the peak speed within 5%: it reads a few percent high, because the delay's
jitter moves the window's edges and the highest of many noisy speeds wins.

The app measures logs with the core's port of this (src/mouse.rs, checked equal to it).

Usage: python python/mouse_read.py <log.bin> [--stats "<stats csv>"] [--dpi N] [--cm360 N]
                                [--window MS] [--start DEG_S] [--stop DEG_S] [--hold MS]
       python python/mouse_read.py --selftest    (builds a synthetic run in test_out/mouse/selftest and checks it)"""
import argparse
import bisect
import json
import math
import random
import re
import statistics as st
import sys
from collections import Counter
from datetime import datetime, timedelta
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import mouse_log as ml  # noqa: E402

DT = 0.00025          # the analysis grid, s
MATCH_TOL = 0.010     # a kill matches a press within 10 ms (after the offset)
SEARCH = 1.0          # the clock offset is searched within 1 s


def deg_per_count(dpi, cm360):
    """The degrees the view turns for one count of the mouse, from its dpi and the cm a 360 takes."""
    return 360 / (cm360 / 2.54 * dpi)


def local(wall):
    """A wall-clock time (s since the epoch) as this computer's local time of day, to the microsecond."""
    return datetime.fromtimestamp(wall).strftime("%H:%M:%S.%f")


class Motion:
    """The log's relative motion as cumulative degrees after each event (absolute events are skipped). Between events
    the position is interpolated: each event's motion is spread evenly over the time since the event before it, but
    over at most cap (1 ms, or twice the median interval if that is longer), so a report never counts as all in or all
    out of a speed window."""

    def __init__(self, log, k):
        """log: as mouse_log.read_log gives it. k: degrees a count. Up is positive y here."""
        self.t, self.x, self.y = [], [], []
        x = y = 0.0
        for t, dx, dy, fl in zip(log["t"], log["dx"], log["dy"], log["flags"]):
            if fl & ml.MOUSE_MOVE_ABSOLUTE:
                continue
            x += dx * k
            y -= dy * k
            self.t.append(t)
            self.x.append(x)
            self.y.append(y)
        gaps = [b - a for a, b in zip(self.t, self.t[1:])]
        self.cap = max(0.001, 2 * st.median(gaps)) if gaps else 0.001

    def pos(self, i):
        """The position (deg) after the first i events: (0, 0) before any."""
        return (self.x[i - 1], self.y[i - 1]) if i else (0.0, 0.0)

    def at(self, t, i=None):
        """Position at time t; i is the index just past the last event at or before t, if known."""
        if i is None:
            i = bisect.bisect_right(self.t, t)
        x0, y0 = self.pos(i)
        if i >= len(self.t):
            return x0, y0
        t1 = self.t[i]
        s = max(self.t[i - 1], t1 - self.cap) if i else t1 - self.cap
        if t <= s:
            return x0, y0
        f = (t - s) / (t1 - s)
        return x0 + f * (self.x[i] - x0), y0 + f * (self.y[i] - y0)

    def speeds(self, a, c, w):
        """Grid times from a to c (ending exactly at c) and the speed in deg/s at each, over [t - w/2, t + w/2]
        cut at c."""
        n = int((c - a) / DT)
        ts, vs, i0, i1 = [], [], 0, 0
        for g in range(n + 1):
            t = c - (n - g) * DT
            lo, hi = t - w / 2, min(t + w / 2, c)
            i0 = bisect.bisect_right(self.t, lo, i0)
            i1 = bisect.bisect_right(self.t, hi, i1)
            (x0, y0), (x1, y1) = self.at(lo, i0), self.at(hi, i1)
            ts.append(t)
            vs.append(math.hypot(x1 - x0, y1 - y0) / (hi - lo))
        return ts, vs


def cross(ts, vs, i, thr):
    """When the speed crosses thr between grid points i - 1 and i (linear), so times are not rounded to the grid."""
    if i == 0 or vs[i] == vs[i - 1]:
        return ts[i]
    f = min(1.0, max(0.0, (thr - vs[i - 1]) / (vs[i] - vs[i - 1])))
    return ts[i - 1] + f * (ts[i] - ts[i - 1])


def find(ts, vs, dt, o):
    """The start time, the peak's index, the stop time and the settle time (the start of the final still stretch)
    in a speed profile that ends at the click; None where there is none. Shared by the reader and the self-test's
    ground truth."""
    n = len(vs)
    if vs[0] >= o.start:                     # still moving from the last flick: a new start needs a stop first
        j = next((i for i in range(n) if vs[i] < o.stop), None)
        s = 0 if j is None else next((i for i in range(j, n) if vs[i] >= o.start), None)
    else:
        s = next((i for i in range(n) if vs[i] >= o.start), None)
    p = max(range(s or 0, n), key=vs.__getitem__)
    hold = max(1, round(o.hold / 1000 / dt))
    stop, below = None, [v < o.stop for v in vs]
    left = [0] * (n + 1)                     # left[i]: grid points in a row under the stop speed from i on
    for i in range(n - 1, -1, -1):
        left[i] = left[i + 1] + 1 if below[i] else 0
    for i in range(p, n):
        if below[i] and left[i] >= min(hold + 1, n - i):
            stop = i
            break
    settle = None
    if below[-1]:
        settle = n - 1
        while settle > 0 and below[settle - 1]:
            settle -= 1
    t = lambda i, thr: cross(ts, vs, i, thr) if i is not None else None
    return t(s, o.start), p, t(stop, o.stop), t(settle, o.stop)


def measure(m, a, c, o):
    """One kill's measures from the motion `m` between the previous press `a` and this kill's press `c` (s on the
    log's scale), with the options `o`: the times found (s), and reaction, flick, peak, stop to click and still (ms,
    deg/s), the speed at the click, the corrections after the stop and the distance moved (deg)."""
    ts, vs =m.speeds(a, c, o.window / 1000)
    s, p, stop, settle = find(ts, vs, DT, o)
    ms = lambda x: round(x * 1000, 3)
    (x0, y0), (x1, y1) = m.at(a), m.at(c)
    return dict(start_s=s, stop_s=stop, settle_s=settle,
                reaction_ms=ms(s - a) if s is not None else None,
                flick_ms=ms(stop - s) if s is not None and stop is not None else None,
                peak_dps=round(vs[p], 1), peak_ms=ms(ts[p] - a),
                stop_to_click_ms=ms(c - stop) if stop is not None else None,
                still_ms=ms(c - settle) if settle is not None else 0.0,
                click_dps=round(vs[-1], 1),
                corrections=sum(1 for i in range(1, len(vs)) if ts[i] > stop and vs[i] >= o.stop > vs[i - 1])
                if stop is not None else None,
                dist_deg=round(math.hypot(x1 - x0, y1 - y0), 3))


def presses_of(log):
    """The times (s) of the log's left-button presses."""
    return [t for t, b in zip(log["t"], log["bflags"]) if b & ml.RI_MOUSE_LEFT_BUTTON_DOWN]


def rate_line(log):
    """(the median interval between events in s, or None, a line on the event rate with a warning when the log
    looks throttled)."""
    t = log["t"]
    gaps = [b - a for a, b in zip(t, t[1:])]
    med = st.median(gaps) if gaps else None
    text = (f"median interval {med * 1000:.3f} ms ({1 / med if med else float('inf'):.0f} Hz), busiest 100 ms "
            f"{ml.busiest_rate(t):.0f} Hz" if med is not None else "too few events for a rate")
    if med is not None and len(t) >= 200 and med > 0.002:
        text += ("\nwarning: events are far apart; Windows probably throttled the logger (about 8 ms when "
                 "throttled), so times are only good to about the interval")
    return med, text


def head(log):
    """Prints the log's file, times, event count, clock drift and devices, and how many absolute events it skips."""
    t0, dur =log["wall0"], log["duration"]
    drift = f"clock drift {log['drift_ms']:.3f} ms" if log["drift_ms"] is not None else "no stop pair (killed?)"
    print(f"log: {log['path']}\n  {local(t0)} to {local(t0 + dur)}, {dur:.2f} s, {len(log['t'])} events, {drift}")
    for d, h in enumerate(log["devices"]):
        print(f"  device {d}: handle {h:#x}, {log['dev'].count(d)} events")
    n_abs = sum(1 for f in log["flags"] if f & ml.MOUSE_MOVE_ABSOLUTE)
    if n_abs:
        print(f"  {n_abs} absolute events (MOUSE_MOVE_ABSOLUTE) skipped")


def summary(path, o):
    """Summary mode: prints the log's head, its event rate, its travel (deg, summed over 1 ms steps) and its
    left-button presses. Without --dpi and --cm360, 1600 dpi and 70 cm/360."""
    log = ml.read_log(path)
    k = deg_per_count(o.dpi or 1600, o.cm360 or 70)
    head(log)
    _, text = rate_line(log)
    print("  " + text.replace("\n", "\n  "))
    bins = {}
    for t, dx, dy, f in zip(log["t"], log["dx"], log["dy"], log["flags"]):
        if not f & ml.MOUSE_MOVE_ABSOLUTE:
            b = bins.setdefault(int(t * 1000), [0, 0])
            b[0] += dx
            b[1] += dy
    travel = sum(math.hypot(x, y) for x, y in bins.values()) * k
    print(f"  travel {travel:.1f} deg (1 ms steps, {k:.6f} deg per count), left-button presses "
          f"{len(presses_of(log))}")


def read_stats(path):
    """A KovaaK's stats file: its kills (number, local time, epoch time, shots), the run's start and end (epoch s),
    its total shots, its (dpi, cm/360) when it uses cm/360 (else None) and its scenario. Stops when the file's name
    has no date."""
    L =Path(path).read_text(encoding="utf-8", errors="replace").splitlines()
    meta = dict(l.split(":,", 1) for l in L if ":," in l)
    m = re.search(r"(\d{4})\.(\d{2})\.(\d{2})-(\d{2})\.(\d{2})\.(\d{2})", Path(path).name)
    if not m:
        sys.exit("the stats file name holds no date (expected '... - 2026.09.30-04.55.23 Stats.csv')")
    end = datetime(*map(int, m.groups()))                 # when the stats were written, about the run's end

    def epoch(hms):
        """A time of day from the file ("HH:MM:SS.fff") as epoch seconds, on the day the file was written (the day
        before, when that puts it more than an hour after the file)."""
        d =datetime.combine(end.date(), datetime.strptime(hms.strip(), "%H:%M:%S.%f").time())
        return (d - timedelta(days=1) if d > end + timedelta(hours=1) else d).timestamp()   # a run over midnight

    rows = []
    for l in L[1:]:
        if not l.strip():
            break
        r = l.split(",")
        rows.append(dict(n=int(r[0]), local=r[1], t=epoch(r[1]), shots=int(r[5])))
    w = next((i for i, l in enumerate(L) if l.startswith("Weapon,Shots")), None)
    shots = 0
    for l in L[w + 1:] if w is not None else []:
        if not l.strip():
            break
        shots += int(float(l.split(",")[1]))
    sens = None
    if meta.get("Sens Scale", "").strip() == "cm/360":
        sens = (float(meta["DPI"]), float(meta["Horiz Sens"]))
    start = epoch(meta["Challenge Start"]) if "Challenge Start" in meta else rows[0]["t"] - 1
    return dict(kills=rows, start=start, end=end.timestamp() + 1, shots=shots, sens=sens,
                scenario=meta.get("Scenario", "").strip())


def match(presses, kills):
    """The offset (log time minus stats time) that puts the most kills within MATCH_TOL of a press, and each kill's
    press index (None when none is within MATCH_TOL)."""
    d = []
    for ki, k in enumerate(kills):
        i, j = bisect.bisect_left(presses, k - SEARCH), bisect.bisect_right(presses, k + SEARCH)
        d += [(presses[x] - k, ki) for x in range(i, j)]
    d.sort()
    cnt, j, best = Counter(), 0, (0, 0, -1)
    for i in range(len(d)):
        cnt[d[i][1]] += 1
        while d[i][0] - d[j][0] > MATCH_TOL:
            cnt[d[j][1]] -= 1
            if not cnt[d[j][1]]:
                del cnt[d[j][1]]
            j += 1
        if len(cnt) > best[0]:
            best = (len(cnt), j, i)
    if not best[0]:
        return None, [None] * len(kills)
    off = st.median(x[0] for x in d[best[1]:best[2] + 1])
    idx = []
    for k in kills:
        near = range(bisect.bisect_left(presses, k + off - MATCH_TOL),
                     bisect.bisect_right(presses, k + off + MATCH_TOL))
        idx.append(min(near, key=lambda x: abs(presses[x] - k - off)) if near else None)
    return off, idx


def q(vals, p):
    """The values' p-quantile for p a tenth from 0.1 to 0.9 (the one value when there is only one)."""
    return st.quantiles(vals, n=10, method="inclusive")[round(p * 10) - 1] if len(vals) > 1 else vals[0]


def show(name, vals, fmt="{:7.1f}"):
    """Prints one measure's count, p10, median and p90 over the kills, Nones left out; nothing when none is left."""
    vals =[v for v in vals if v is not None]
    if vals:
        print(f"  {name:34s} n {len(vals):3d}  p10 {fmt.format(q(vals, .1))}  median "
              f"{fmt.format(st.median(vals))}  p90 {fmt.format(q(vals, .9))}")


def run(path, stats_path, o):
    """Run mode: matches the stats file's kills to the log's presses, measures each matched kill, prints the summary
    and writes <log>.kills.json. Returns the offset (s), the matched count, the misses' times and the kills' rows.
    Stops when the log does not cover the run or no press is near a kill."""
    log = ml.read_log(path)
    stats = read_stats(stats_path)
    dpi, cm360 = (o.dpi or (stats["sens"] or (1600, 70))[0]), (o.cm360 or (stats["sens"] or (1600, 70))[1])
    k = deg_per_count(dpi, cm360)
    src = "options" if o.dpi or o.cm360 else ("the stats file" if stats["sens"] else "defaults")
    head(log)
    med, text = rate_line(log)
    print("  " + text.replace("\n", "\n  "))
    if med is not None and len(log["t"]) >= 200 and 2 * med * 1000 > o.window:
        o.window = round(2 * med * 1000, 2)
        print(f"  speed window widened to {o.window} ms (twice the median interval)")
    print(f"stats: {Path(stats_path).name}\n  {dpi:g} dpi, {cm360:g} cm/360 (from {src}), {k:.6f} deg per count")
    w0 = log["wall0"]
    kills = [r["t"] - w0 for r in stats["kills"]]          # stats times on the log's time scale
    if not kills or kills[-1] < -SEARCH or kills[0] > log["duration"] + SEARCH:
        sys.exit(f"the log ({local(w0)} to {local(w0 + log['duration'])}) does not cover this run "
                 f"({stats['kills'][0]['local'] if kills else '?'} on)")
    presses = presses_of(log)
    off, idx = match(presses, kills)
    if off is None:
        sys.exit("no left-button press lies within 1 s of any kill")
    gaps = [abs(presses[i] - k - off) * 1000 for i, k in zip(idx, kills) if i is not None]
    print(f"  clock offset (press minus stats kill time) {off * 1000:+.1f} ms; {len(gaps)} of {len(kills)} kills "
          f"matched within {MATCH_TOL * 1000:.0f} ms (gap p90 {q(gaps, .9) if gaps else 0:.1f} ms)")
    lo, hi = stats["start"] - w0 + off, stats["end"] - w0 + off
    used = set(i for i in idx if i is not None)
    in_run = [i for i, t in enumerate(presses) if lo <= t <= hi]
    misses = [presses[i] for i in in_run if i not in used]
    print(f"  presses in the run {len(in_run)} (stats shots {stats['shots']}), misses {len(misses)}")
    m = Motion(log, k)
    out, a = [], stats["start"] - w0 + off
    for r, kt, i in zip(stats["kills"], kills, idx):
        if i is None:
            a = kt + off
            continue
        c = presses[i]
        row = dict(n=r["n"], kill_local=r["local"], press_local=local(w0 + c), press_s=round(c, 6),
                   gap_ms=round((c - kt - off) * 1000, 3), shots=r["shots"], **measure(m, a, c, o))
        out.append(row)
        a = c
    print(f"{len(out)} kills measured (times from the previous kill's press)")
    show("reaction: start (ms)", [r["reaction_ms"] for r in out])
    show("flick: start to stop (ms)", [r["flick_ms"] for r in out])
    show("peak speed (deg/s)", [r["peak_dps"] for r in out], "{:7.0f}")
    show("stop to click (ms)", [r["stop_to_click_ms"] for r in out])
    show("still before the click (ms)", [r["still_ms"] for r in out])
    show("speed at the click (deg/s)", [r["click_dps"] for r in out])
    show("distance (deg)", [r["dist_deg"] for r in out])
    print(f"  clicked while moving: {sum(1 for r in out if r['still_ms'] == 0)} of {len(out)}; no stop before the "
          f"click: {sum(1 for r in out if r['stop_s'] is None)}; with corrections: "
          f"{sum(1 for r in out if r['corrections'])}")
    js = Path(path).with_suffix(".kills.json")
    js.write_text(json.dumps(dict(log=str(path), stats=str(stats_path), offset_ms=round(off * 1000, 3), dpi=dpi,
                                  cm360=cm360, window_ms=o.window, start_dps=o.start, stop_dps=o.stop, hold_ms=o.hold,
                                  misses_s=[round(t, 6) for t in misses], kills=out), indent=1))
    print(f"wrote {js}")
    return dict(offset=off, matched=len(gaps), misses=misses, kills=out)


# ---- self-test: a synthetic run with known flicks ----

def selftest(o):
    """Builds a synthetic run (20 minimum-jerk flicks logged at 8000 Hz with a handling delay, a clock drift, a stats
    file whose clock is off and one miss), reads it with run, and checks every measure against the same rules on the
    exact speed. Prints the errors; returns 0 when every one is within its limit, else 1."""
    rng = random.Random(7)
    folder = Path(__file__).resolve().parent.parent / "test_out" / "mouse" / "selftest"
    folder.mkdir(parents=True, exist_ok=True)
    k = deg_per_count(1600, 70)
    FREQ, Q0, REPORT = 10_000_000, 5_000_000_000, 1 / 8000
    NS0 = int(datetime(2026, 9, 30, 4, 54, 20).timestamp()) * 10 ** 9
    DRIFT, OFFSET = 20e-6, -0.0373          # wall clock 20 ppm fast against QPC; stats clock 37.3 ms behind the log
    snap = lambda t: round(t / REPORT) * REPORT
    wall = lambda t: NS0 / 1e9 + t * (1 + DRIFT)            # QPC seconds since Q0 as wall-clock seconds

    segs, clicks, plan, a = [], [], [], 1.0                 # min-jerk moves (t0, T, dx, dy); x right, y down
    cs = a
    for j in range(1, 21):
        R, T, D, th = rng.uniform(.12, .22), rng.uniform(.10, .16), rng.uniform(6, 30), rng.uniform(0, 2 * math.pi)
        t0 = a + R
        segs.append((t0, T, D * math.cos(th), D * math.sin(th)))
        end = t0 + T
        if j % 5 == 3:                                       # a 1 deg correction after the flick
            th = rng.uniform(0, 2 * math.pi)
            segs.append((end + .03, .05, math.cos(th), math.sin(th)))
            end += .08
        if j % 5 == 4:                                       # clicks while still moving
            c = snap(t0 + .75 * T)
        else:
            h = rng.uniform(.03, .12)
            c = snap(end + h)
            if j == 7:
                clicks.append((snap(end + h / 2), False))    # a miss while settled
        clicks.append((c, True))
        plan.append((a, c))
        a = c
    t_end = a + .5
    t0s = [s[0] for s in segs]
    cum = [(0.0, 0.0)]
    for s in segs:
        cum.append((cum[-1][0] + s[2], cum[-1][1] + s[3]))

    def state(t):
        """Position and velocity (deg, deg/s) at t; the moves never overlap."""
        j = bisect.bisect_right(t0s, t) - 1
        if j < 0:
            return 0.0, 0.0, 0.0, 0.0
        t0, T, dx, dy = segs[j]
        u = min(1.0, (t - t0) / T)
        f, v = 10 * u ** 3 - 15 * u ** 4 + 6 * u ** 5, 30 * u * u * (1 - u) ** 2 / T
        return cum[j][0] + dx * f, cum[j][1] + dy * f, dx * v, dy * v

    buttons = {}
    for c, _ in clicks:
        buttons[round(c / REPORT)] = ml.RI_MOUSE_LEFT_BUTTON_DOWN
        buttons[round((c + .07) / REPORT)] = ml.RI_MOUSE_LEFT_BUTTON_UP
    body, cx, cy, tx, ty, last_q, logged = bytearray(ml.DEV.pack(ml.KIND_DEVICE, 0xABC, 0, 0)), 0, 0, 0, 0, 0, {}
    for i in range(round(.5 / REPORT), round(t_end / REPORT)):
        r = i * REPORT
        x, y, vx, vy = state(r)
        if math.hypot(vx, vy) < 1 and rng.random() < .01:   # tremor at rest: a stray count now and then
            if rng.random() < .5:
                tx += rng.choice((-1, 1))
            else:
                ty += rng.choice((-1, 1))
        nx, ny = math.floor(x / k) + tx, math.floor(y / k) + ty
        b = buttons.get(i, 0)
        if nx != cx or ny != cy or b:
            qv = max(last_q, Q0 + round((r + rng.uniform(20e-6, 150e-6)) * FREQ))   # handling delay
            body += ml.REC.pack(qv, nx - cx, ny - cy, 0, b, 0, 0)
            cx, cy, last_q = nx, ny, qv
            if b & ml.RI_MOUSE_LEFT_BUTTON_DOWN:
                logged[i] = (qv - Q0) / FREQ
    log_path, q1 = folder / "synthetic.bin", Q0 + round(t_end * FREQ)
    log_path.write_bytes(ml.HEADER.pack(ml.MAGIC, ml.VERSION, ml.REC.size, FREQ, Q0, NS0) + body +
                         ml.STOP.pack(ml.KIND_STOP, q1, NS0 + round(t_end * (1 + DRIFT) * 1e9)))

    stamp = lambda w: datetime.fromtimestamp(w).strftime("%H:%M:%S.%f")[:-3]
    run_end = datetime.fromtimestamp(wall(a + .3) + OFFSET)
    lines = ["Kill #,Timestamp,Bot,Weapon,TTK,Shots,Hits,Accuracy,Damage Done,Damage Possible,Efficiency,Cheated,"
             "OverShots"]
    shots, n = 1, 0
    for c, kill in clicks:
        if not kill:
            shots += 1
            continue
        n += 1
        lines.append(f"{n},{stamp(wall(c) + OFFSET + rng.uniform(.0005, .0025))},target,BB Gun,0.000000s,{shots},1,"
                     f"1.0,1.0,1.0,1.0,0,0")
        shots = 1
    lines += ["", "Weapon,Shots,Hits,Damage Done,Damage Possible,,Sens Scale,Horiz Sens,Vert Sens,FOV",
              f"BB Gun,{len(clicks)},20,20.0,{len(clicks)}.0,", "", "Kills:,20", "Scenario:,Selftest",
              f"Challenge Start:,{stamp(wall(cs) + OFFSET)}", "Sens Scale:,cm/360", "Horiz Sens:,70.0",
              "DPI:,1600"]
    stats_path = folder / f"Selftest - Challenge - {run_end:%Y.%m.%d-%H.%M.%S} Stats.csv"
    stats_path.write_text("\n".join(lines) + "\n", encoding="utf-8")

    summary(log_path, o)
    res = run(log_path, stats_path, o)

    # Ground truth: the same rules on the exact speed, on a 10 us grid, with times on the log's wall scale.
    fine, scale, errs, fails = 1e-5, 1 + DRIFT, {"start": [], "stop": [], "still": [], "click": [], "peak": []}, []
    for (a, c), got in zip(plan, res["kills"]):
        ts = [a + g * fine for g in range(int((c - a) / fine) + 1)]
        vs = [math.hypot(*state(t)[2:]) for t in ts]
        s, p, stop, settle = find(ts, vs, fine, o)
        click = logged[round(c / REPORT)]
        pairs = [("start", s, got["start_s"]), ("stop", stop, got["stop_s"]),
                 ("still", c - settle if settle is not None else 0.0, got["still_ms"] / 1000),
                 ("click", c, got["press_s"])]
        for name, want, have in pairs:
            if (want is None) != (have is None):
                fails.append(f"kill {got['n']}: {name} expected {want}, got {have}")
            elif want is not None:
                want = want * scale if name != "still" else want
                errs[name].append(abs(have - want) * 1000)
        errs["peak"].append(abs(got["peak_dps"] / vs[p] - 1) * 100)
        if abs(got["press_s"] - click * scale) > 1e-6:
            fails.append(f"kill {got['n']}: matched the wrong press")
    want_off = -OFFSET * 1000 - 1.5
    print(f"selftest: offset {res['offset'] * 1000:.2f} ms (built in: {-OFFSET * 1000:.1f} ms less a 0.5-2.5 ms "
          f"kill delay); {res['matched']} of 20 kills matched; {len(res['misses'])} miss (1 built in)")
    for name, e in errs.items():
        print(f"  {name:5s} max error {max(e):.3f} {'%' if name == 'peak' else 'ms'} over {len(e)} kills")
    limit = {"start": 1, "stop": 1, "still": 1, "click": 1, "peak": 5}       # ms, and % for the peak
    fails += [f"{name} error {max(e):.3f} over the limit {limit[name]}" for name, e in errs.items()
              if e and max(e) > limit[name]]
    if abs(res["offset"] * 1000 - want_off) > 2 or res["matched"] != 20 or len(res["misses"]) != 1:
        fails.append("offset, matches or misses wrong")
    print("selftest " + ("passed" if not fails else "FAILED:\n  " + "\n  ".join(fails)))
    return 0 if not fails else 1


def main():
    """Runs the self-test, run mode (with --stats) or summary mode."""
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("log", nargs="?", help="a log from python/mouse_log.py")
    ap.add_argument("--stats", help="the run's KovaaK's stats csv")
    ap.add_argument("--dpi", type=float, help="mouse dpi (default: the stats file, else 1600)")
    ap.add_argument("--cm360", type=float, help="cm per 360 deg (default: the stats file, else 70)")
    ap.add_argument("--window", type=float, default=4.0, help="speed window, ms (default 4)")
    ap.add_argument("--start", type=float, default=30.0, help="moving above this speed, deg/s (default 30)")
    ap.add_argument("--stop", type=float, default=10.0, help="still below this speed, deg/s (default 10)")
    ap.add_argument("--hold", type=float, default=5.0, help="a stop lasts at least this long, ms (default 5)")
    ap.add_argument("--selftest", action="store_true", help="check the reader on a synthetic run")
    o = ap.parse_args()
    if o.selftest:
        return selftest(o)
    if not o.log:
        ap.error("give a log, or --selftest")
    if o.stats:
        run(o.log, o.stats, o)
    else:
        summary(o.log, o)


if __name__ == "__main__":
    sys.exit(main())
