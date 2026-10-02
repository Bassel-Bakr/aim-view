"""Log the raw mouse stream to a file, to time flicks and clicks in a KovaaK's run (see "Raw mouse log" in
python/README.md).

The logger registers a message-only window for raw mouse input with RIDEV_INPUTSINK, so it keeps receiving input
while KovaaK's has focus. It never sends input and never moves the cursor. Each WM_INPUT becomes one 24-byte record:
the QueryPerformanceCounter (QPC) time when the message was handled, the counts moved (x right, y down), usFlags,
usButtonFlags, usButtonData and the device. The file starts with a header holding the QPC frequency and a
(QPC, time.time_ns()) pair, and a second pair is appended at stop. python/mouse_read.py reads the file.

File format (little endian):
  header  "<4sHHqqq"  b"FFML", version 1, record size 24, QPC frequency, start QPC, start time_ns
  event   "<qiiHHHH"  QPC, dx, dy, usFlags, usButtonFlags, usButtonData, device index
  device  "<qQII"     -1, device handle, device index, 0 (written before the first event of each device)
  stop    "<qqq"      -2, stop QPC, stop time_ns

Windows 11 throttles raw input to background programs to about 125 Hz. Unless the user turns that off
(HKCU\\Control Panel\\Mouse, RawMouseThrottleEnabled = 0, then sign out and in), the logger gets the motion summed
into about one message every 8 ms while KovaaK's has focus. The summary warns when the log looks throttled.

Usage: python python/mouse_log.py [--out FILE] [--seconds N]
       python python/mouse_log.py --bench     (times the per-event code path; reads no real input)"""
import argparse
import ctypes as C
import signal
import struct
import sys
import time
from ctypes import wintypes as W
from datetime import datetime
from pathlib import Path

MAGIC, VERSION = b"FFML", 1
HEADER = struct.Struct("<4sHHqqq")
REC = struct.Struct("<qiiHHHH")
DEV = struct.Struct("<qQII")
STOP = struct.Struct("<qqq")
KIND_DEVICE, KIND_STOP = -1, -2
MOUSE_MOVE_ABSOLUTE = 0x01
RI_MOUSE_LEFT_BUTTON_DOWN, RI_MOUSE_LEFT_BUTTON_UP = 0x0001, 0x0002
assert REC.size == DEV.size == STOP.size == 24 and HEADER.size == 32


# Raw Input structures, 64-bit layouts.
class RAWINPUTHEADER(C.Structure):
    _fields_ = [("dwType", W.DWORD), ("dwSize", W.DWORD), ("hDevice", W.HANDLE), ("wParam", W.WPARAM)]


class _BUTTONS(C.Structure):
    _fields_ = [("usButtonFlags", W.USHORT), ("usButtonData", W.USHORT)]


class _BUTTONS_UNION(C.Union):
    _fields_ = [("ulButtons", W.ULONG), ("s", _BUTTONS)]


class RAWMOUSE(C.Structure):
    _fields_ = [("usFlags", W.USHORT), ("u", _BUTTONS_UNION), ("ulRawButtons", W.ULONG), ("lLastX", W.LONG),
                ("lLastY", W.LONG), ("ulExtraInformation", W.ULONG)]


class RAWINPUT(C.Structure):              # the data union is as large as RAWMOUSE, so only the mouse is declared
    _fields_ = [("header", RAWINPUTHEADER), ("mouse", RAWMOUSE)]


class RAWINPUTDEVICE(C.Structure):
    _fields_ = [("usUsagePage", W.USHORT), ("usUsage", W.USHORT), ("dwFlags", W.DWORD), ("hwndTarget", W.HWND)]


assert C.sizeof(C.c_void_p) == 8, "needs 64-bit Python"
assert (C.sizeof(RAWINPUTHEADER), C.sizeof(RAWMOUSE), C.sizeof(RAWINPUT)) == (24, 24, 48)
# One unpack reads what the logger keeps from a RAWINPUT: dwType, hDevice, usFlags, usButtonFlags, usButtonData,
# lLastX, lLastY. Check its offsets against the ctypes layout.
RAW = struct.Struct("<I4xQ8xH2xHH4xii")
_M = RAWINPUT.mouse.offset
assert (RAWINPUTHEADER.hDevice.offset, _M + RAWMOUSE.u.offset, _M + RAWMOUSE.lLastX.offset,
        _M + RAWMOUSE.lLastY.offset, RAW.size) == (8, 28, 36, 40, 44)

WM_INPUT, PM_REMOVE, RID_INPUT, RIM_TYPEMOUSE, RIM_INPUT = 0x00FF, 0x0001, 0x10000003, 0, 0
RIDEV_INPUTSINK, RIDI_DEVICENAME, HWND_MESSAGE = 0x00000100, 0x20000007, -3
QS_ALLINPUT, MWMO_INPUTAVAILABLE, THREAD_PRIORITY_HIGHEST = 0x04FF, 0x0004, 2
FAIL = 0xFFFFFFFF


def win():
    """Load the Windows functions with their argument types (handles are 64-bit)."""
    u32 = C.WinDLL("user32", use_last_error=True)
    k32 = C.WinDLL("kernel32", use_last_error=True)
    sig = {
        (u32, "CreateWindowExW"): (W.HWND, [W.DWORD, W.LPCWSTR, W.LPCWSTR, W.DWORD, C.c_int, C.c_int, C.c_int,
                                            C.c_int, W.HWND, W.HMENU, W.HINSTANCE, W.LPVOID]),
        (u32, "DestroyWindow"): (W.BOOL, [W.HWND]),
        (u32, "RegisterRawInputDevices"): (W.BOOL, [C.POINTER(RAWINPUTDEVICE), W.UINT, W.UINT]),
        (u32, "GetRawInputData"): (W.UINT, [W.LPARAM, W.UINT, W.LPVOID, C.POINTER(W.UINT), W.UINT]),
        (u32, "GetRawInputDeviceInfoW"): (W.UINT, [W.HANDLE, W.UINT, W.LPVOID, C.POINTER(W.UINT)]),
        (u32, "PeekMessageW"): (W.BOOL, [C.POINTER(W.MSG), W.HWND, W.UINT, W.UINT, W.UINT]),
        (u32, "PostMessageW"): (W.BOOL, [W.HWND, W.UINT, W.WPARAM, W.LPARAM]),
        (u32, "TranslateMessage"): (W.BOOL, [C.POINTER(W.MSG)]),
        (u32, "DispatchMessageW"): (W.LPARAM, [C.POINTER(W.MSG)]),
        (u32, "DefWindowProcW"): (W.LPARAM, [W.HWND, W.UINT, W.WPARAM, W.LPARAM]),
        (u32, "MsgWaitForMultipleObjectsEx"): (W.DWORD, [W.DWORD, W.LPVOID, W.DWORD, W.DWORD, W.DWORD]),
        (k32, "QueryPerformanceCounter"): (W.BOOL, [C.POINTER(C.c_int64)]),
        (k32, "QueryPerformanceFrequency"): (W.BOOL, [C.POINTER(C.c_int64)]),
        (k32, "GetCurrentThread"): (W.HANDLE, []),
        (k32, "SetThreadPriority"): (W.BOOL, [W.HANDLE, C.c_int]),
    }
    for (dll, name), (res, args) in sig.items():
        f = getattr(dll, name)
        f.restype, f.argtypes = res, args
    return u32, k32


class Logger:
    """A message-only window that turns WM_INPUT messages into records in self.out."""

    def __init__(self):
        self.u32, self.k32 = u32, k32 = win()
        self.hwnd = u32.CreateWindowExW(0, "STATIC", "flowfix mouse_log", 0, 0, 0, 0, 0, HWND_MESSAGE, None, None,
                                        None)
        if not self.hwnd:
            raise OSError(C.get_last_error(), "CreateWindowExW failed")
        f = C.c_int64()
        k32.QueryPerformanceFrequency(C.byref(f))
        self.freq = f.value
        self.out = bytearray()
        self.devices = {}                     # handle -> index
        self.names = []
        self.bad = 0
        msg, raw, size, qpc = W.MSG(), RAWINPUT(), W.UINT(), C.c_int64()
        pmsg, praw, psize, pq = C.byref(msg), C.byref(raw), C.byref(size), C.byref(qpc)
        peek, getraw, now = u32.PeekMessageW, u32.GetRawInputData, k32.QueryPerformanceCounter
        defproc, translate, dispatch = u32.DefWindowProcW, u32.TranslateMessage, u32.DispatchMessageW
        unpack, pack, extend, devices = RAW.unpack_from, REC.pack, self.out.extend, self.devices
        rawsize, hdrsize = C.sizeof(raw), C.sizeof(RAWINPUTHEADER)
        self.raw, self.qpc_ref, self.qpc_val = raw, pq, qpc

        def record(t):
            """Decode the RAWINPUT in raw and append one record."""
            typ, h, fl, bf, bd, x, y = unpack(raw)
            if typ != RIM_TYPEMOUSE:
                return
            d = devices.get(h)
            if d is None:
                d = self.add_device(h)
            extend(pack(t, x, y, fl, bf, bd, d))

        def drain():
            """Handle every queued message; WM_INPUT is read here, not in a window procedure."""
            while peek(pmsg, None, 0, 0, PM_REMOVE):
                if msg.message == WM_INPUT:
                    now(pq)
                    t = qpc.value
                    size.value = rawsize
                    if getraw(msg.lParam, RID_INPUT, praw, psize, hdrsize) == FAIL:
                        self.bad += 1
                        continue
                    if msg.wParam == RIM_INPUT:   # input while this window is foreground: let the system clean up
                        defproc(msg.hWnd, WM_INPUT, msg.wParam, msg.lParam)
                    record(t)
                else:
                    translate(pmsg)
                    dispatch(pmsg)

        self.record, self.drain = record, drain

    def add_device(self, h):
        d = self.devices[h] = len(self.devices)
        self.out.extend(DEV.pack(KIND_DEVICE, h, d, 0))
        name = "(no device handle: injected or synthetic input)"
        n = W.UINT()
        if h and self.u32.GetRawInputDeviceInfoW(h, RIDI_DEVICENAME, None, C.byref(n)) == 0 and n.value:
            buf = C.create_unicode_buffer(n.value)
            if self.u32.GetRawInputDeviceInfoW(h, RIDI_DEVICENAME, buf, C.byref(n)) != FAIL:
                name = buf.value
        self.names.append(name)
        return d

    def register(self):
        rid = RAWINPUTDEVICE(1, 2, RIDEV_INPUTSINK, self.hwnd)      # usage page 1 (generic desktop), usage 2 (mouse)
        if not self.u32.RegisterRawInputDevices(C.byref(rid), 1, C.sizeof(rid)):
            raise OSError(C.get_last_error(), "RegisterRawInputDevices failed")

    def qpc(self):
        self.k32.QueryPerformanceCounter(self.qpc_ref)
        return self.qpc_val.value

    def clock_pair(self):
        """A (QPC, time_ns) pair: the tightest of 20 tries, with time_ns read between two QPC reads."""
        best = None
        for _ in range(20):
            a = self.qpc()
            ns = time.time_ns()
            b = self.qpc()
            if best is None or b - a < best[0]:
                best = (b - a, (a + b) // 2, ns)
        return best[1], best[2]

    def wait(self, ms):
        self.u32.MsgWaitForMultipleObjectsEx(0, None, ms, QS_ALLINPUT, MWMO_INPUTAVAILABLE)

    def close(self):
        self.u32.DestroyWindow(self.hwnd)


def throttle_setting():
    """The background raw input throttle values under HKCU\\Control Panel\\Mouse, read only."""
    import winreg
    found = {}
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, r"Control Panel\Mouse") as k:
            i = 0
            while True:
                try:
                    name, value, _ = winreg.EnumValue(k, i)
                except OSError:
                    break
                if name.startswith("RawMouseThrottl"):
                    found[name] = value
                i += 1
    except OSError:
        pass
    if not found:
        return "background throttle not set (Windows 11 default: about 125 Hz while KovaaK's has focus)"
    return "background throttle: " + ", ".join(f"{k} = {v}" for k, v in sorted(found.items()))


def read_log(path):
    """Parse a log into columns. Times t are seconds since the start pair, on the wall clock's scale (the QPC time is
    stretched by the drift between the two pairs). Without a stop pair (the logger was killed) the scale is 1."""
    data = Path(path).read_bytes()
    magic, version, recsize, freq, q0, ns0 = HEADER.unpack_from(data, 0)
    if magic != MAGIC or version != VERSION or recsize != REC.size:
        raise ValueError(f"{path}: not a version {VERSION} mouse log")
    body = memoryview(data)[HEADER.size:HEADER.size + (len(data) - HEADER.size) // REC.size * REC.size]
    qpc = [r[0] for r in struct.iter_unpack("<q16x", body)]
    control = [i for i, q in enumerate(qpc) if q < 0]
    devices, stop = [], None
    for i in control:
        if qpc[i] == KIND_DEVICE:
            devices.append(DEV.unpack_from(body, i * REC.size)[1])
        elif qpc[i] == KIND_STOP:
            stop = STOP.unpack_from(body, i * REC.size)[1:]
    keep = set(control)
    rows = [r for i, r in enumerate(REC.iter_unpack(body)) if i not in keep] if keep else list(REC.iter_unpack(body))
    scale, drift_ms = 1.0, None
    if stop and stop[0] > q0:
        span = (stop[0] - q0) / freq
        wall = (stop[1] - ns0) / 1e9
        scale, drift_ms = wall / span, (wall - span) * 1e3
    k = scale / freq
    return dict(path=str(path), freq=freq, wall0=ns0 / 1e9, stop=stop, drift_ms=drift_ms,
                duration=((stop[0] - q0) * k) if stop else ((rows[-1][0] - q0) * k if rows else 0.0),
                devices=devices, t=[(r[0] - q0) * k for r in rows], dx=[r[1] for r in rows], dy=[r[2] for r in rows],
                flags=[r[3] for r in rows], bflags=[r[4] for r in rows], bdata=[r[5] for r in rows],
                dev=[r[6] for r in rows])


def busiest_rate(t, span=0.1):
    """Events a second in the busiest window of span seconds."""
    best, j = 0, 0
    for i in range(len(t)):
        while t[i] - t[j] > span:
            j += 1
        best = max(best, i - j + 1)
    return best / span


def summary(path, names=None):
    log = read_log(path)
    n, dur = len(log["t"]), log["duration"]
    peak = busiest_rate(log["t"])
    print(f"mouse_log: {n} events in {dur:.2f} s ({n / dur if dur else 0:.1f} Hz mean, busiest 100 ms "
          f"{peak:.0f} Hz); wrote {path}")
    for d, h in enumerate(log["devices"]):
        name = f" {names[d]}" if names and d < len(names) else ""
        print(f"  device {d}: handle {h:#x}, {log['dev'].count(d)} events{name}")
    if n >= 200 and peak <= 300:
        print("  warning: events came at most about 125 a second. Windows probably throttled the logger, so the "
              "times are only good to about 8 ms. See python/README.md, \"Raw mouse log\".")
    return log


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--out", help="output file (default test_out/mouse/mouse_<date>_<time>.bin)")
    ap.add_argument("--seconds", type=float, help="stop after this many seconds (default: run until Ctrl+C)")
    ap.add_argument("--bench", action="store_true", help="time the per-event code path and exit")
    a = ap.parse_args()
    if a.bench:
        return bench()
    out = Path(a.out) if a.out else (Path(__file__).resolve().parent.parent / "test_out" / "mouse" /
                                     f"mouse_{datetime.now():%Y-%m-%d_%H-%M-%S}.bin")
    out.parent.mkdir(parents=True, exist_ok=True)
    lg = Logger()
    lg.register()
    lg.k32.SetThreadPriority(lg.k32.GetCurrentThread(), THREAD_PRIORITY_HIGHEST)
    stopping = []
    for s in (signal.SIGINT, getattr(signal, "SIGBREAK", None)):
        if s is not None:
            signal.signal(s, lambda *_: stopping.append(1))
    with open(out, "wb", buffering=0) as f:
        q0, ns0 = lg.clock_pair()
        f.write(HEADER.pack(MAGIC, VERSION, REC.size, lg.freq, q0, ns0))
        end = q0 + int(a.seconds * lg.freq) if a.seconds else None
        print(f"mouse_log: logging to {out} ({f'for {a.seconds:g} s' if a.seconds else 'Ctrl+C stops'}; "
              f"QPC {lg.freq} Hz; {throttle_setting()})", flush=True)
        last_flush = time.perf_counter()
        try:
            while not stopping:
                lg.wait(100)
                lg.drain()
                if len(lg.out) >= 1 << 16 or time.perf_counter() - last_flush > 0.25:
                    f.write(lg.out)               # every quarter second, so a killed logger loses little
                    del lg.out[:]
                    last_flush = time.perf_counter()
                if end is not None and lg.qpc() >= end:
                    break
        finally:
            lg.drain()
            q1, ns1 = lg.clock_pair()
            f.write(lg.out + STOP.pack(KIND_STOP, q1, ns1))
            lg.close()
    if lg.bad:
        print(f"  {lg.bad} WM_INPUT messages could not be read")
    summary(out, lg.names)


def bench(n_msgs=5000, n_records=200_000):
    """Time the per-event path without real input: (1) PeekMessage, the QPC read and the GetRawInputData call, on
    WM_INPUT messages posted to the logger's own window (their handle is invalid, so the read fails as fast as the
    system rejects it); (2) decoding a filled RAWINPUT and appending its record. No input is sent to the system."""
    lg = Logger()
    lg.raw.header.dwType, lg.raw.header.hDevice = RIM_TYPEMOUSE, 0x1234
    lg.raw.mouse.lLastX, lg.raw.mouse.lLastY = 3, -2
    lg.add_device(0x1234)
    per_msg = []
    for _ in range(5):
        for _ in range(n_msgs):
            lg.u32.PostMessageW(lg.hwnd, WM_INPUT, 1, 0)
        t0 = time.perf_counter()
        lg.drain()
        per_msg.append((time.perf_counter() - t0) / n_msgs)
    bad = lg.bad
    del lg.out[:]
    record = lg.record
    t0 = time.perf_counter()
    for i in range(n_records):
        record(i)
    per_rec = (time.perf_counter() - t0) / n_records
    lg.close()
    total = min(per_msg) + per_rec
    print(f"message + QPC + GetRawInputData: {min(per_msg) * 1e6:.2f} us ({bad} posted messages read, all rejected "
          f"as expected); decode + record: {per_rec * 1e6:.2f} us; total {total * 1e6:.2f} us per event, about "
          f"{1 / total:,.0f} events a second (an 8000 Hz mouse needs 8,000)")


if __name__ == "__main__":
    sys.exit(main())
