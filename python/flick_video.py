"""A short manim video about flicks (step 4 of python/README.md), first made on 2026-10-01 from the user's 143 run in
1w4ts Voltaic.

Every path is a real flick from <VOD_DIR>/flicks.json and measures.json, except the one panel marked as an
illustration. The kill numbers shown (43, 42, 13) and the title's run are that run's; pick new ones for another VOD.
    VOD_DIR=test_out/vod test_out/manim_venv/Scripts/manim -qh --media_dir test_out/vod/media python/flick_video.py FlickVideo
"""
import json
import math
import os
from pathlib import Path

import numpy as np
from manim import *

HERE = Path(os.environ.get("VOD_DIR", "test_out/vod"))
FL = {f["n"]: f for f in json.load(open(HERE / "flicks.json"))}
MS = {m["n"]: m for m in json.load(open(HERE / "measures.json"))}
R = 0.43                       # the target's radius, deg
FPS = 120.0
WALL, TARGET, CROSS, TRAIL = "#c9ccd2", "#111111", "#e0302a", "#e0302a"
BG = "#1d1f24"
PHASES = {"React": "#8ab4f8", "Main flick": "#f6c26b", "Stopped short": "#b0b0b0", "Second push": "#f28b82",
          "On target": "#81c995"}
config.background_color = BG
Text.set_default(font="Segoe UI")


def track(n, rotate=False):
    """Times (s) and crosshair positions relative to the target (deg), smoothed over the capture's uneven frames.
    rotate: turn the path so the flick runs left to right."""
    tr = FL[n]["traj"]
    f0 = tr[0][0]
    t = np.array([(p[0] - f0) / FPS for p in tr])
    c = -np.array([[p[1], p[2]] for p in tr])
    s = c.copy()
    s[1:-1] = (c[:-2] + 2 * c[1:-1] + c[2:]) / 4
    if rotate:
        a = -math.atan2(-s[0][1], -s[0][0])
        rot = np.array([[math.cos(a), -math.sin(a)], [math.sin(a), math.cos(a)]])
        s = s @ rot.T
    return t, s


def at(t, s, x):
    return np.array([np.interp(x, t, s[:, 0]), np.interp(x, t, s[:, 1])])


def speeds(t, s):
    v = np.zeros(len(t))
    v[1:-1] = np.linalg.norm(s[2:] - s[:-2], axis=1) / (t[2:] - t[:-2])
    return v


class View(VGroup):
    """A patch of the wall: the target at world (0, 0), degrees mapped into the panel."""

    def __init__(self, center, size, world_center, scale, label=None):
        super().__init__()
        self.c, self.wc, self.k = np.array([*center, 0.0][:3], float), np.array(world_center), scale
        self.panel = RoundedRectangle(width=size[0], height=size[1], corner_radius=0.08, fill_color=WALL,
                                      fill_opacity=1, stroke_width=0).move_to(self.c)
        self.target = Circle(radius=R * scale, fill_color=TARGET, fill_opacity=1, stroke_width=0).move_to(self.p((0, 0)))
        self.add(self.panel, self.target)
        if label:
            self.add(Text(label, font_size=22, color=WHITE).next_to(self.panel, UP, buff=0.12).align_to(self.panel, LEFT))

    def p(self, w):
        return np.array([*(self.c[:2] + self.k * (np.array(w) - self.wc)), 0.0])


def crosshair(view, t, s, clock, t_from=0.0):
    dot = Dot(radius=0.07, color=CROSS).move_to(view.p(at(t, s, t_from)))
    dot.add_updater(lambda m: m.move_to(view.p(at(t, s, clock.get_value()))))
    trail = TracedPath(dot.get_center, stroke_color=TRAIL, stroke_width=2.5, stroke_opacity=0.55)
    return dot, trail


def click_mark(view, pos, hit=True):
    if hit:
        return Circle(radius=0.28, color=WHITE, stroke_width=4).move_to(view.p(pos))
    return Cross(Square(0.3), stroke_color=YELLOW, stroke_width=6).move_to(view.p(pos))


def budget():
    """Each kill split at its own moments: moving (react), main flick over, on the target, settled, click. Returns
    the average ms of each part and the number of kills."""
    parts = []
    for f in FL.values():
        m = MS.get(f["n"])
        tr = f["traj"]
        if not m or len(tr) < 6 or None in (m["react"], m["flick"], m["arrive"]):
            continue
        fr = [p[0] for p in tr]
        d = [(p[1], p[2]) for p in tr]
        ds = [d[0]] + [((d[i - 1][0] + d[i][0] + d[i + 1][0]) / 3, (d[i - 1][1] + d[i][1] + d[i + 1][1]) / 3)
                       for i in range(1, len(d) - 1)] + [d[-1]]
        sp = [0.0] + [math.dist(ds[i], ds[i - 1]) * FPS for i in range(1, len(ds))]
        k = len(d) - 1
        arr = next(i for i in range(len(d)) if math.hypot(*d[i]) < R)
        settle = next((i for i in range(arr, k + 1) if all(v < 10 for v in sp[i:k])), k)
        total = (fr[k] - fr[0]) / FPS
        b = sorted([m["react"], min(m["react"] + m["flick"], m["arrive"]), m["arrive"], (fr[settle] - fr[0]) / FPS])
        b = [0.0] + [min(x, total) for x in b] + [total]
        parts.append([1000 * (b[i + 1] - b[i]) for i in range(5)])
    return [sum(p[i] for p in parts) / len(parts) for i in range(5)], len(parts)


class FlickVideo(Scene):
    def construct(self):
        self.title_card()
        self.anatomy()
        self.gallery()
        self.time_budget()

    # ---- 1 ------------------------------------------------------------------------------------------------------
    def title_card(self):
        t1 = Text("Anatomy of a flick", font_size=64, weight=BOLD)
        t2 = Text("Measured from your 143 run in 1w4ts Voltaic", font_size=30, color=GREY_B)
        t3 = Text("139 flicks tracked frame by frame at 120 fps", font_size=26, color=GREY_C)
        g = VGroup(t1, t2, t3).arrange(DOWN, buff=0.3)
        self.play(FadeIn(t1, shift=UP * 0.2), run_time=0.8)
        self.play(FadeIn(t2), FadeIn(t3), run_time=0.8)
        self.wait(1.4)
        self.play(FadeOut(g), run_time=0.5)

    # ---- 2 ------------------------------------------------------------------------------------------------------
    def anatomy(self):
        n, slow = 43, 10.0
        m = MS[n]
        t, s = track(n)
        head = Text("One flick, ten times slower", font_size=36, weight=BOLD).to_edge(UP, buff=0.35)
        sub = Text(f"Kill {n}: {m['D0']:.1f}° to the right, 0.40 s from the last kill to this one",
                   font_size=22, color=GREY_B).next_to(head, DOWN, buff=0.12)
        view = View((-3.35, -0.55), (7.0, 4.6), (-6.5, -1.4), 0.46)
        self.play(FadeIn(head), FadeIn(sub), FadeIn(view), run_time=0.7)

        ax = Axes(x_range=[0, 420, 100], y_range=[0, 180, 60], x_length=5.4, y_length=2.6, tips=False,
                  axis_config={"color": GREY_B, "include_numbers": False}).move_to((3.85, 0.55, 0))
        xl = VGroup(*[Text(str(v), font_size=16, color=GREY_B).next_to(ax.c2p(v, 0), DOWN, buff=0.1)
                      for v in (0, 100, 200, 300, 400)])
        yl = VGroup(*[Text(str(v), font_size=16, color=GREY_B).next_to(ax.c2p(0, v), LEFT, buff=0.1)
                      for v in (60, 120, 180)])
        xt = Text("ms since the last kill", font_size=18, color=GREY_B).next_to(ax, DOWN, buff=0.4)
        yt = Text("crosshair speed, °/s", font_size=18, color=GREY_B).next_to(ax, UP, buff=0.1).align_to(ax, LEFT)
        self.play(Create(ax), FadeIn(xl), FadeIn(yl), FadeIn(xt), FadeIn(yt), run_time=0.6)

        v = speeds(t, s)
        clock = ValueTracker(0.0)
        curve = always_redraw(lambda: self._curve(ax, t, v, clock.get_value()))
        dot, trail = crosshair(view, t, s, clock)
        self.add(trail, dot, curve)

        stop_i = int(np.argmin([np.linalg.norm(p) if 0.24 <= x <= 0.29 else 9 for x, p in zip(t, s)]))
        phases = [("React", 0.0, m["react"]), ("Main flick", m["react"], m["react"] + m["flick"]),
                  ("Stopped short", m["react"] + m["flick"], 0.283), ("Second push", 0.283, m["arrive"]),
                  ("On target", m["arrive"], m["total"])]
        rows = VGroup()
        for name, a, b in phases:
            row = VGroup(Square(0.18, fill_color=PHASES[name], fill_opacity=1, stroke_width=0),
                         Text(name, font_size=22), Text(f"{1000 * (b - a):.0f} ms", font_size=22, color=GREY_B))
            row[1].next_to(row[0], RIGHT, buff=0.15)
            row[2].next_to(row[0], RIGHT, buff=2.4)
            rows.add(row)
        rows.arrange(DOWN, aligned_edge=LEFT, buff=0.16).next_to(xt, DOWN, buff=0.35).align_to(ax, LEFT)
        for (name, a, b), row in zip(phases, rows):
            band = Rectangle(width=max(0.02, ax.c2p(1000 * b, 0)[0] - ax.c2p(1000 * a, 0)[0]),
                             height=ax.y_length, fill_color=PHASES[name], fill_opacity=0.22, stroke_width=0)
            band.move_to(((ax.c2p(1000 * a, 0)[0] + ax.c2p(1000 * b, 0)[0]) / 2, ax.c2p(0, 90)[1], 0))
            self.add(band)
            self.bring_to_front(curve)
            self.play(FadeIn(row, shift=RIGHT * 0.1), clock.animate.set_value(b), run_time=max(0.35, (b - a) * slow),
                      rate_func=linear)
        self.play(Create(click_mark(view, at(t, s, m["total"]))), run_time=0.3)
        note = Text("Click", font_size=22, color=WHITE).next_to(view.p(at(t, s, m["total"])), UP, buff=0.35)
        self.play(FadeIn(note), run_time=0.3)
        self.wait(1.2)

        # replay the end, zoomed on the target
        zc, zk = (-0.7, 0.05), 1.75
        zoom = View((-3.35, -0.55), (7.0, 4.6), zc, zk)
        half = 7.0 / 2 / zk - 0.15
        t_in = float(t[int(np.argmax((s[:, 0] > zc[0] - half) & (t > 0.05)))])
        ztitle = Text("The end of it, zoomed in", font_size=22, color=BLACK)
        ztitle.align_to(zoom.panel, LEFT).align_to(zoom.panel, UP).shift(RIGHT * 0.25 + DOWN * 0.18)
        self.play(FadeOut(VGroup(dot, trail, note)), *[FadeOut(x) for x in self.mobjects if isinstance(x, Circle)
                                                       and x is not view.target], run_time=0.3)
        self.play(ReplacementTransform(view, zoom), run_time=0.6)
        self.play(FadeIn(ztitle), run_time=0.3)
        clock2 = ValueTracker(t_in)
        dot2, trail2 = crosshair(zoom, t, s, clock2, t_in)
        self.add(trail2, dot2)
        self.play(clock2.animate.set_value(0.283), run_time=(0.283 - t_in) * slow * 1.5, rate_func=linear)
        stop = at(t, s, 0.26)
        short = Text(f"stopped {np.linalg.norm(stop):.1f}° from the centre, outside the target",
                     font_size=20, color=BLACK).move_to(zoom.p((stop[0] + 0.25, -1.0)))
        arrow = Arrow(short.get_top(), zoom.p(stop) + DOWN * 0.1, buff=0.08, color=BLACK, stroke_width=3,
                      max_tip_length_to_length_ratio=0.2)
        self.play(FadeIn(short), GrowArrow(arrow), run_time=0.4)
        self.wait(0.8)
        self.play(clock2.animate.set_value(m["total"]), run_time=(m["total"] - 0.283) * slow * 1.5, rate_func=linear)
        self.play(Create(click_mark(zoom, at(t, s, m["total"]))), run_time=0.3)
        self.wait(1.2)
        self.play(*[FadeOut(x) for x in self.mobjects], run_time=0.5)

    def _curve(self, ax, t, v, now):
        k = int(np.searchsorted(t, now, side="right"))
        pts = [ax.c2p(1000 * t[i], min(180, v[i])) for i in range(max(2, k))]
        return VMobject(stroke_color=WHITE, stroke_width=3).set_points_as_corners(pts)

    # ---- 3 ------------------------------------------------------------------------------------------------------
    def gallery(self):
        head = Text("Four ways a flick goes wrong", font_size=36, weight=BOLD).to_edge(UP, buff=0.35)
        self.play(FadeIn(head), run_time=0.5)
        spots = [(-3.55, 1.25), (3.55, 1.25), (-3.55, -2.15), (3.55, -2.15)]
        size, wc, k = (6.6, 2.2), (-1.6, 0.0), 1.25
        items = [
            (13, "Overflick", "Kill 13: went {past:.1f}° past the centre, then came back"),
            (43, "Stopping short", "Kill 43: stopped {short:.1f}° before the centre, then a second push"),
            (42, "Waiting on target", "Kill 42: on the target {wait:.0f} ms before the click"),
            (None, "Clicking before it stops", "Illustration, not your run: the shot goes early and misses"),
        ]
        for (n, name, cap), spot in zip(items, spots):
            view = View(spot, size, wc, k, label=name)
            if n is None:
                t = np.linspace(0, 0.25, 61)
                x = -4.0 + 4.0 * (1 - (1 - t / 0.25) ** 2.4)
                s = np.stack([x, 0.08 * np.sin(t * 9)], axis=1)
                t_click, hit = float(np.interp(-0.62, x, t)), False
                t0 = 0.0
            else:
                t, s = track(n, rotate=True)
                t_click, hit = float(t[-1]), True
                t0 = float(t[int(np.argmax(s[:, 0] > wc[0] - size[0] / 2 / k + 0.1))])
            t42, s42 = track(42)
            vals = dict(past=MS[13]["past"], short=float(np.linalg.norm(at(*track(43), 0.26))),
                        wait=1000 * (t42[-1] - t42[int(np.argmax(np.linalg.norm(s42, axis=1) < R))]))
            caption = Text(cap.format(**vals), font_size=18, color=GREY_B)
            if caption.width > size[0] - 0.2:
                caption.scale_to_fit_width(size[0] - 0.2)
            caption.next_to(view.panel, DOWN, buff=0.1)
            self.play(FadeIn(view), FadeIn(caption), run_time=0.4)
            clock = ValueTracker(t0)
            dot, trail = crosshair(view, t, s, clock, t0)
            self.add(trail, dot)
            if n == 42:                              # count the time on the target while it plays
                on_t = float(t[int(np.argmax(np.linalg.norm(s, axis=1) < R))])
                timer = always_redraw(lambda: Text(
                    f"on target: {max(0.0, 1000 * (clock.get_value() - on_t)):.0f} ms", font_size=22,
                    color=BLACK).align_to(view.panel, LEFT).align_to(view.panel, UP).shift(RIGHT * 0.25 + DOWN * 0.15))
                self.add(timer)
            self.play(clock.animate.set_value(t_click), run_time=(t_click - t0) * 8, rate_func=linear)
            if n == 42:
                timer.clear_updaters()
            self.play(Create(click_mark(view, at(t, s, t_click), hit)), run_time=0.3)
            if not hit:
                self.play(clock.animate.set_value(t[-1]), run_time=(t[-1] - t_click) * 8, rate_func=linear)
            self.wait(0.5)
        self.wait(1.5)
        self.play(*[FadeOut(x) for x in self.mobjects], run_time=0.5)

    # ---- 4 ------------------------------------------------------------------------------------------------------
    def time_budget(self):
        head = Text("Where a kill's time goes", font_size=36, weight=BOLD).to_edge(UP, buff=0.5)
        avg, n = budget()
        total = sum(avg)
        sub = Text(f"Average of each part over {n} of your kills; together they make the average kill, "
                   f"{total:.0f} ms", font_size=22, color=GREY_B).next_to(head, DOWN, buff=0.15)
        self.play(FadeIn(head), FadeIn(sub), run_time=0.5)
        parts = [(name, round(a), col) for (name, col), a in zip(
            (("React", "#8ab4f8"), ("Main flick", "#f6c26b"), ("Onto the target", "#f28b82"),
             ("Settle", "#c58af9"), ("Still on target", "#81c995")), avg)]
        unit = 11.5 / sum(p[1] for p in parts)
        x = -11.5 / 2
        bars, labels = VGroup(), VGroup()
        for i, (name, ms, col) in enumerate(parts):
            w = ms * unit
            b = Rectangle(width=w, height=1.0, fill_color=col, fill_opacity=0.9, stroke_color=BG, stroke_width=3)
            b.move_to((x + w / 2, 0.3, 0))
            lab = VGroup(Text(name, font_size=20), Text(f"{ms} ms", font_size=20, color=GREY_B)).arrange(DOWN, buff=0.08)
            lab.next_to(b, DOWN if i % 2 == 0 else UP, buff=0.2)
            bars.add(b)
            labels.add(lab)
            x += w
        for b, lab in zip(bars, labels):
            self.play(GrowFromEdge(b, LEFT), FadeIn(lab), run_time=0.45)
        box = SurroundingRectangle(bars[-1], color=WHITE, buff=0.06, stroke_width=4)
        self.play(Create(box), run_time=0.4)
        l1 = Text(f"{100 * avg[-1] / total:.0f}% of every kill is spent still, on the target, before the click.",
                  font_size=26)
        l2 = Text("Every 10 ms off each kill is about 3 to 4 more kills a run.", font_size=26, color="#81c995")
        l3 = Text("Next: click as the crosshair stops.", font_size=30, weight=BOLD)
        g = VGroup(l1, l2, l3).arrange(DOWN, buff=0.22).to_edge(DOWN, buff=0.55)
        for line in g:
            self.play(FadeIn(line, shift=UP * 0.1), run_time=0.5)
            self.wait(0.6)
        self.wait(2.0)
        self.play(*[FadeOut(x) for x in self.mobjects], run_time=0.6)
