// VOD review page: lists the recordings from python/server.py, runs the review, shows the report, and plays any flick
// slowed down with the tracked target drawn over the video.
const $ = (s) => document.querySelector(s);
const ms = (v) => (v == null ? "–" : `${Math.round(1000 * v)} ms`);
// 13278 as 13,278 (and 13278.5 as 13,278.5); text that is not a number stays as it is
const num = (v) => (v == null || v === "" || Number.isNaN(+v) ? v : (+v).toLocaleString("en-US", { maximumFractionDigits: 2 }));
const pct = (v) => (v == null ? "–" : `${Math.round(100 * v)}%`);
const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
const ARROWS = { right: "→", "up-right": "↗", up: "↑", "up-left": "↖", left: "←", "down-left": "↙", down: "↓",
  "down-right": "↘" };
const PARTS = [["React", "--series-1"], ["Main flick", "--series-2"], ["Onto the target", "--series-3"],
  ["Settle", "--series-4"], ["Still on the target", "--series-5"]];

let vods = [], current = null, report = null, flick = null, rate = 0.25, stopAt = null;
let tracks = null, orderCache = null, fitts = null, orders = null, lastKill = Infinity, firstStart = 0;   // the path overlay
let faint = null;                                      // the faint-target cut-off (faintScores)
const video = $("#video"), overlay = $("#overlay");

async function api(path, opts) {
  const r = await fetch(path, opts);
  const j = await r.json();
  if (!r.ok) throw new Error(j && j.error ? j.error : r.statusText);
  return j;
}

// ---- recordings list ------------------------------------------------------------------------------------------------
async function loadVods() {
  vods = await api("/api/vods");
  renderVods();
  const want = new URLSearchParams(location.search).get("id");
  if (want && vods.find((v) => v.id === want)) select(want);
}

function renderVods() {
  const f = $("#search").value.trim().toLowerCase();
  $("#vods").innerHTML = vods.filter((v) => !f || v.scenario.toLowerCase().includes(f)).slice(0, 400).map((v) => `
    <div class="vod${v.id === current ? " sel" : ""}${v.stats ? "" : " nostats"}" data-id="${esc(v.id)}" role="option">
      <div class="name" title="${esc(v.scenario)}">${esc(v.scenario)}</div>
      <div class="meta">${v.score != null ? `<span>${num(v.score)}</span>` : ""}<span>${v.stamp.slice(0, 10).replaceAll(".", "-")} ${v.stamp.slice(11, 16).replace(".", ":")}</span>
        ${v.uploaded ? '<span class="badge up">uploaded</span>' : ""}${busy.has(v.id) ? '<span class="badge busy">analysing</span>' : v.analysed ? '<span class="badge done">reviewed</span>' : ""}${v.stats ? "" : '<span class="badge">no stats</span>'}${v.not_aim ? '<span class="badge">not an aim trainer</span>' : ""}</div>
    </div>`).join("");
}

$("#search").addEventListener("input", renderVods);
$("#vods").addEventListener("click", (e) => {
  const row = e.target.closest(".vod");
  if (row) select(row.dataset.id);
});

async function select(id) {
  if (excluding) stopExcluding();
  current = id;
  report = null;
  flick = null;
  tracks = faint = tl = null;
  faintOpen = false;
  $("#run-box").hidden = true;
  $("#runbtn").classList.remove("on");
  $("#faint-box").hidden = true;
  $("#cutoff").classList.remove("on");
  $("#tl-box").hidden = true;
  orderCache = null;
  orders = null;
  const v = vods.find((x) => x.id === id);
  history.replaceState(null, "", `?id=${encodeURIComponent(id)}`);
  renderVods();
  $("#empty").hidden = true;
  $("#run").hidden = false;
  $("#run-title").textContent = v.scenario;
  $("#run-sub").textContent = `${v.score != null ? `Score ${num(v.score)} · ` : ""}${v.stamp} · ${(v.size / 1e6).toFixed(0)} MB` +
    (v.stats || v.analysed ? "" : " · no stats file: the review reads KovaaK's session HUD in the video, or finds the kills in the video alone");
  $("#analyse").textContent = v.analysed ? "Review again" : "Analyse";
  $("#analyse").classList.toggle("primary", !v.analysed);
  $("#source").textContent = "";
  $("#review-by").textContent = "";
  $("#report").innerHTML = "";
  $("#flick-scroll").innerHTML = "";
  $("#kill-marks").innerHTML = "";
  $("#flick-panel").hidden = true;
  $("#speed-wrap").classList.add("idle");
  $("#flick-label").textContent = "";
  video.src = `/video?id=${encodeURIComponent(id)}`;
  video.playbackRate = rate;
  if (v.analysed) await showReport();
  else watchJob();
}

// ---- analysis -------------------------------------------------------------------------------------------------------
$("#analyse").addEventListener("click", async () => {
  const v = vods.find((x) => x.id === current);
  try {
    await api(`/api/analyse?id=${encodeURIComponent(current)}${v.analysed ? "&again=1" : ""}`, { method: "POST" });
    watchJob();
  } catch (e) {
    showProgress(`Could not start: ${e.message}`, 0);
  }
});

function showProgress(text, frac) {
  $("#progress").hidden = false;
  $("#bar").style.width = `${Math.round(100 * frac)}%`;
  $("#progress-text").textContent = text;
}

const busy = new Set();
async function watchJob(id = current) {
  if (busy.has(id)) return;                            // already watched: that loop updates the page too
  busy.add(id);
  renderVods();
  try {
    for (;;) {
      const j = await api(`/api/job?id=${encodeURIComponent(id)}`);
      const here = id === current;
      if (j.stage === "none") { if (here) $("#progress").hidden = true; return; }
      if (j.stage === "error") { if (here) showProgress(`Error: ${j.error}`, 0); return; }
      if (j.stage === "done") {
        vods.find((x) => x.id === id).analysed = true;
        if (here) {
          tracks = orders = orderCache = faint = null;    // a new review wrote new tracks: never draw the old ones
          showProgress(`Reviewed in ${j.seconds} s`, 1);
          $("#analyse").textContent = "Review again";
          $("#analyse").classList.remove("primary");
          $("#analyse").disabled = false;
          showReport();
        }
        return;
      }
      if (here) {
        const label = { starting: "Starting", looking: "Looking at the key frames", tracking: "Tracking the targets",
          linking: "Linking the tracks", "reading the HUD": "Reading the session HUD", measuring: "Measuring the flicks",
          camera: "Reading the camera's turn" }[j.stage] || j.stage;
        showProgress(`${label}… ${j.stage === "tracking" || j.stage === "camera" ? `${j.done} / ${j.total} frames` : ""}`,
          j.done / Math.max(1, j.total));
        $("#analyse").disabled = true;
      }
      await new Promise((r) => setTimeout(r, 500));
    }
  } finally {
    busy.delete(id);
    renderVods();
  }
}

// ---- report ---------------------------------------------------------------------------------------------------------
async function showReport() {
  report = await api(`/api/report?id=${encodeURIComponent(current)}`);
  if (!report) return;
  if (!models) await loadModels().catch(() => {});
  const by = report.review_model, mine = models && by === models.chosen;
  $("#review-by").textContent = by == null ? "The model behind this review was not recorded (it is from before reviews kept it)"
    : `Reviewed with ${modelName(by)}${mine || !models ? "" : `, not ${modelName(models.chosen)} (the model in use)`}`;
  if (models) $("#analyse").textContent = mine ? "Review again" : `Review with ${modelName(models.chosen)}`;
  const s = report.summary, info = s.info || {};
  $("#source").textContent = { stats: "Stats file", hud: "Session HUD", aimlab: "Aim Lab HUD", video: "Video only" }[info.source] || "Stats file";
  $("#source").title = { stats: "Kills, shots and score from KovaaK's stats file", hud: "Kills and shots read from KovaaK's session HUD in the video",
    aimlab: "Hits, misses and score read from Aim Lab's POINTS in the video",
    video: "Kills found in the video alone: no shots, misses or score" }[info.source] || "";
  if (report.mode === "track") { showTracking(s); return; }
  $("#report").innerHTML = `
    <div id="tiles"></div>
    <p class="note">${info.source === "video"
      ? `No stats file and no readable session HUD: ${info.matched} kills found in the video alone (in tests ` +
        "between 1 in 3 and 9 in 10 are right, depending on the run). These are kills, not shots: a target that takes two hits is one kill, and score, misses, " +
        "accuracy and shot counts are not available"
      : info.source === "hud"
        ? `No stats file: kills, shots and hits read from KovaaK's session HUD in the video (${info.matched} of ` +
          `${info.kills_stats} kills matched to a target); the score is the file name's`
        : info.source === "aimlab"
          ? `No stats file: hits and misses read from Aim Lab's POINTS in the video (a hit adds points, a miss takes some ` +
            `off; every hit is counted as a kill), ${info.matched} of ${info.kills_stats} matched to a target; the score is ` +
            "the last POINTS"
          : `${info.matched} of ${info.kills_stats} kills matched with the stats file`} · ${s.measured} flicks measured ·
      target radius ${s.radius.toFixed(2)}°${s.sens ? ` · ${esc(s.sens)}` : ""} · the checks below are provisional until the
      issue list is settled</p>

    <div id="budget-box"></div>

    <h3>What to look at</h3>
    <div class="issues" id="issues"></div>

    <div class="split">
      <div><h3>By distance</h3><table><thead><tr><th>Distance</th><th>Flicks</th><th>Median kill</th><th>Reaction</th>
        <th>Stopped short</th><th>Went past</th><th>Still</th></tr></thead><tbody>
        ${s.by_distance.map((b) => `<tr><td>${b.hi === 90 ? `${b.lo}° and over` : `${b.lo}–${b.hi}°`}</td><td>${b.n}</td><td>${ms(b.interval)}</td>
          <td>${ms(b.react)}</td><td>${pct(b.short)}</td><td>${pct(b.past)}</td><td>${ms(b.still)}</td></tr>`).join("")}</tbody></table></div>
      <div><h3>By direction</h3><table><thead><tr><th>Toward</th><th>Flicks</th><th>Median kill</th><th>Distance</th>
        <th title="Median time beyond what the distance predicts (Fitts' law fitted to the run)">For its distance</th>
        <th>Stopped short</th><th>Went past</th></tr></thead><tbody>
        ${s.by_direction.map((b) => `<tr><td>${ARROWS[b.name]} ${b.name}</td><td>${b.n}</td><td>${ms(b.interval)}</td>
          <td>${b.distance.toFixed(1)}°</td><td>${b.beyond == null ? "–" : `${b.beyond >= 0 ? "+" : "−"}${ms(Math.abs(b.beyond))}`}</td>
          <td>${pct(b.short)}</td><td>${pct(b.past)}</td></tr>`).join("")}</tbody></table></div>
    </div>`;
  // the flick list sits beside the video, so a flick can be picked while watching
  $("#flick-scroll").innerHTML = `
    <table id="flicks"><thead><tr><th>Kill</th><th>Distance</th><th>Toward</th><th>Kill time</th><th>Main flick ended</th>
      <th>Still</th><th>Peak</th><th>Click speed</th><th>Shots</th><th title="Time the pick of this target cost against the fastest pick">Path cost</th></tr></thead><tbody>
      ${report.flicks.map((m) => `<tr data-n="${m.n}"><td>${m.n}</td><td>${m.D0.toFixed(1)}°</td><td>${arrow(m.dir)}</td>
        <td>${ms(m.total)}</td><td>${ended(m, s.radius)}</td><td>${ms(m.still)}</td><td>${Math.round(m.peak)} °/s</td>
        <td>${m.click_speed.toFixed(0)} °/s</td><td class="${m.shots > 1 ? "miss" : ""}">${m.shots ?? "–"}</td>
        <td class="oc" data-n="${m.n}">${orderText(m.n)}</td></tr>`).join("")}</tbody></table>`;
  $("#flick-panel").hidden = false;
  renderTiles(null);
  renderIssues();
  killMarks();
  for (const b of [orderBox, mineBox, followBox]) b.closest("label").hidden = false;
  $("#speed-wrap").hidden = false;
  fitts = fitFitts();
  lastKill = Math.max(...report.flicks.map((m) => m.kill_frame));
  // the run's start: the stats file gives it; from the HUD or the video alone, the first kill less two median kill
  // intervals (the countdown before it shows the targets too)
  const first = report.flicks.reduce((a, b) => (b.kill_frame < a.kill_frame ? b : a));
  firstStart = (report.summary.info || {}).source === "stats" ? first.start_frame
    : Math.max(first.start_frame, first.kill_frame - Math.round(2 * (report.summary.median_interval || 0.5) * report.fps));
  if (report.run?.start != null) firstStart = Math.round(report.run.start * report.fps);    // the user's marks
  if (report.run?.end != null) lastKill = Math.round(report.run.end * report.fps);
  loadTracks();
  $("#flicks").addEventListener("click", (e) => {
    const tr = e.target.closest("tr[data-n]");
    if (tr) playFlick(+tr.dataset.n);
  });
}

// A tracking run (review.track_summary): the bots never die, so there are no flicks; the time on the target instead,
// with its timeline, second by second.
function showTracking(s) {
  fitts = null;                                         // no flicks: no path overlay
  report.flicks = [];
  $("#flick-panel").hidden = true;
  killMarks();
  $("#tl-legend").innerHTML = `<b>On target over the run</b>
    <span><i style="background:var(--series-3)"></i>on target</span>
    <span><i style="background:var(--series-2)"></i>off target</span>${s.bots > 0 ? `
    <span><i style="background:var(--text-muted)"></i>switching after a death</span>
    <span><i style="background:#fff;width:2px"></i>a death</span>` : ""}
    <span><i style="background:var(--series-2);opacity:0.5"></i>how far off its edge</span>
    <span class="muted">click or drag to go there</span>`;
  $("#tl-box").hidden = false;
  $("#speed-wrap").hidden = true;                       // a flick's speed chart: no flicks here
  loadTracks();                                         // the highlight of the tracked bot on the video
  for (const b of [orderBox, mineBox, followBox]) b.closest("label").hidden = true;   // no flicks: no paths
  const deg = (v) => (v == null ? "–" : `${v.toFixed(2)}°`);
  const sec = (v) => (v == null ? "–" : v < 1 ? `${Math.round(1000 * v)} ms` : `${v.toFixed(2)} s`);   // under 1 s in ms
  const bots = s.bots > 0;
  const tiles = [["Score", num(s.score) ?? "–", "The run's score, from the stats file or the file name."],
    [bots ? "On target while tracking" : "On target", pct(s.on_target),
      "How much of the time your crosshair was on the bot." +
      (bots ? " The time after a bot dies, until you are on the next one, is left out." : "")],
    ...(bots ? [["On target, whole run", pct(s.on_all),
      "The same with that switching time counted too, as the game's accuracy counts it."]] : []),
    ["Accuracy (stats file)", pct(s.accuracy),
      "The game's own number: hits ÷ (hits + misses) while you fired. A check on On target."],
    ["Distance from the center", deg(s.error),
      "How far your crosshair usually was from the bot's middle (a capsule's middle line), when on or near it."],
    ["Lost the bot, per second", s.lost == null ? "–" : s.lost.toFixed(2),
      "How often you lost the bot: came off it for more than 0.1 s, per second of tracking." +
      (s.lost_cost == null ? "" : ` The time off it in those drops cost you ${pct(s.lost_cost)} accuracy` +
        `${s.slip_cost != null ? `; slips shorter than 0.1 s cost ${pct(s.slip_cost)} more` : ""}.`),
      s.lost_cost == null ? "" : `cost ${pct(s.lost_cost)} accuracy`],
    ["Time to get back", sec(s.back), "How long those drops usually lasted before you were back on."],
    ["Longest off", sec(s.longest_off), "The longest single drop off the bot."],
    ...(bots ? [["Bots killed", num(s.bots), "Bots that died in the run (from the stats file or the HUD's kill count)."],
      ["Time to the next bot", sec(s.to_next), "Usual time from a bot's death until you were on a bot again."],
      ["Waiting for a spawn", sec(s.waiting), "The part of that with no bot on screen yet."],
      ["Getting onto it", sec(s.onto), "The part from the next bot showing until you were on it."],
      ["Switching", pct(s.switching), "How much of the run went on getting from a dead bot to the next."]] : []),
    ["Game FPS", s.fps_avg ? Math.round(s.fps_avg) : "–", "Your average frame rate, from the stats file."]];
  $("#report").innerHTML = `
    <div class="tiles">${tiles.map(([k, v, why, sub]) => tvTile(k, v, sub, why)).join("")}</div>
    <p class="note">Tracking: the review measures the time the crosshair spent on the target, from the tracked
      targets. On target means the crosshair lay inside the target's box. Distance from the center is
      the median distance from the target's center line (a sphere's center, a capsule's long axis) over the frames on
      or near it. Lost the bot counts the stretches off the target longer than 0.1 s, per second of tracking, with the
      accuracy they cost (their time as a share of the tracking time); time to get back is their median length. The stats file's accuracy is the game's own
      measure of the same thing.${bots ? ` Bots die here: from a death until your crosshair is on a target again is
      switching, not tracking, so it is left out of "while tracking", the lost stretches and the measures below, and
      measured on its own. It splits into waiting for a spawn (no target on screen) and getting onto it. "Whole run"
      counts switching too, as the accuracy does.` : ""}${s.faint ? ` The faint-target cut-off is on: ${s.faint.tracks}
      tracks scoring under ${s.faint.cut} are left out of every measure here.` : ""}</p>
    ${motionHtml(s.motion, deg)}
    ${whatIfHtml(s)}`;
}

// The review's estimates of how much the accuracy would rise if one thing changed (review.what_if).
function whatIfHtml(s) {
  const w = s.what_if || [];
  if (!w.length) return "";
  return `<h3>What would raise your accuracy</h3>
    <p class="note">Each line is how much more of the time you would have been on the bot if that one thing had been
      different and everything else the same: the off-target time it accounts for. They overlap, so they do not add
      up; each is the most that change could give.</p>
    <table class="what-if"><thead><tr><th>Change</th><th>Accuracy</th><th>Why</th></tr></thead><tbody>
      ${w.map((r) => `<tr><td>${esc(r.what)}</td><td class="gain">+${(100 * r.gain).toFixed(1)}%</td>
        <td class="muted">${esc(r.how)}</td></tr>`).join("")}</tbody></table>`;
}

// A tracking run's timeline (the user asked for a better one than bars per second, 2026-10-02): per moment, how far
// outside the bot's edge the crosshair was (the bot the overlay boxes: nearest the crosshair by its center line; 0
// while on it), and a strip of on target, off target and switching; a playhead that follows the video, and
// click or drag to seek. Built from the tracks the page loads for the overlay.
let tl = null;
function renderTimeline() {
  const box = $("#track-tl");
  if (!box || !tracks || report?.mode !== "track") return;
  const s = report.summary, fps = report.fps, a = s.start ?? 0, b = Math.min(s.end ?? tracks.frames.length, tracks.frames.length);
  const n = b - a, sw = s.switches || [];
  const state = new Int8Array(n), dist = new Float32Array(n).fill(NaN);
  for (let i = a; i < b; i++) {
    const f = tracks.frames[i];
    if (!f) continue;
    let best = null, inside = false;
    f.t.forEach(([, x, y], k) => {
      const [w, h] = f.wh ? f.wh[k] : [0.6, 0.6];
      const d = Math.hypot(Math.max(0, Math.abs(x) - Math.max(0, w - h) / 2), Math.max(0, Math.abs(y) - Math.max(0, h - w) / 2));
      inside ||= Math.abs(x) <= w / 2 + 0.05 && Math.abs(y) <= h / 2 + 0.05;
      if (!best || d < best[0]) best = [d, Math.min(w, h) / 2];
    });
    const switching = sw.some(([x, y]) => i >= x && i < y);
    state[i - a] = switching ? 3 : inside ? 1 : best ? 2 : 0;      // 0 no bot seen, 1 on, 2 off, 3 switching
    if (best && !switching) dist[i - a] = inside ? 0 : Math.max(0, best[0] - best[1]);   // outside the bot's edge
  }
  const off = [...dist].filter((v) => v > 0).sort((x, y) => x - y);
  const cap = Math.min(5, Math.max(0.5, off.length ? off[Math.floor(0.98 * (off.length - 1))] : 0.5));
  tl = { a, n, fps, state, dist, cap, deaths: sw.map(([d]) => d - a) };
  drawTimeline();
  if (!box.dataset.wired) {
    box.dataset.wired = "1";
    new ResizeObserver(() => drawTimeline()).observe(box);
    const at = (e) => {
      const r = box.getBoundingClientRect();
      return Math.min(tl.n - 1, Math.max(0, Math.floor((e.clientX - r.left) / r.width * tl.n)));
    };
    let dragging = false;
    const seekTo = (e) => { video.pause(); video.currentTime = (tl.a + at(e) + 0.5) / tl.fps; };
    box.addEventListener("mousedown", (e) => { dragging = true; seekTo(e); });
    window.addEventListener("mouseup", () => (dragging = false));
    box.addEventListener("mousemove", (e) => {
      if (dragging) seekTo(e);
      const k = at(e), tip = box.querySelector(".tl-tip"), r = box.getBoundingClientRect();
      const what = ["no bot seen", "on target", "off target", "switching after a death"][tl.state[k]];
      const d = tl.dist[k];
      tip.textContent = `${clock((tl.a + k) / tl.fps)} · ${what}${Number.isNaN(d) || d === 0 ? "" : ` · ${d.toFixed(2)}° outside its edge`}`;
      tip.hidden = false;
      tip.style.left = `${Math.min(r.width - tip.offsetWidth, Math.max(0, e.clientX - r.left + 10))}px`;
    });
    box.addEventListener("mouseleave", () => (box.querySelector(".tl-tip").hidden = true));
  }
}

function drawTimeline() {
  const box = $("#track-tl");
  if (!box || !tl) return;
  const cv = box.querySelector("canvas"), W = box.clientWidth, H = box.clientHeight, dpr = devicePixelRatio || 1;
  if (!W) return;
  cv.width = W * dpr; cv.height = H * dpr;
  const c = cv.getContext("2d");
  c.setTransform(dpr, 0, 0, dpr, 0, 0);
  const css = getComputedStyle(document.documentElement), col = (v) => css.getPropertyValue(v).trim();
  const top = 6, chartH = H - 52, stripY = top + chartH + 8, stripH = 14;
  const y = (d) => top + chartH * (1 - Math.min(d, tl.cap) / tl.cap);
  c.strokeStyle = col("--grid");
  for (const g of [0, 0.5, 1]) {
    c.setLineDash(g ? [3, 4] : []);
    c.beginPath(); c.moveTo(0, Math.round(y(g * tl.cap)) + 0.5); c.lineTo(W, Math.round(y(g * tl.cap)) + 0.5); c.stroke();
  }
  c.setLineDash([]);
  // per pixel column: how far outside the bot's edge, the furthest (light) and the average (dark)
  const mean = [], max = [];
  for (let px = 0; px < W; px++) {
    const k0 = Math.floor(px * tl.n / W), k1 = Math.max(k0 + 1, Math.floor((px + 1) * tl.n / W));
    let sum = 0, cnt = 0, hi = 0;
    for (let k = k0; k < k1; k++) { const d = tl.dist[k]; if (!Number.isNaN(d)) { sum += d; cnt++; hi = Math.max(hi, d); } }
    mean.push(cnt ? sum / cnt : 0); max.push(hi);
  }
  const area = (vals, alpha) => {
    c.fillStyle = col("--series-2");
    c.globalAlpha = alpha;
    c.beginPath();
    c.moveTo(0, y(0));
    vals.forEach((v, px) => { c.lineTo(px, y(v)); c.lineTo(px + 1, y(v)); });
    c.lineTo(W, y(0));
    c.closePath();
    c.fill();
    c.globalAlpha = 1;
  };
  area(max, 0.35);
  area(mean, 0.9);
  // the strip: on, off, switching, per pixel column (the state most of its frames had)
  const fills = [col("--grid"), col("--series-3"), col("--series-2"), col("--text-muted")];
  for (let px = 0; px < W; px++) {
    const k0 = Math.floor(px * tl.n / W), k1 = Math.max(k0 + 1, Math.floor((px + 1) * tl.n / W));
    const cnt = [0, 0, 0, 0];
    for (let k = k0; k < k1; k++) cnt[tl.state[k]]++;
    c.fillStyle = fills[cnt.indexOf(Math.max(...cnt))];
    c.fillRect(px, stripY, 1, stripH);
  }
  c.fillStyle = "#fff";
  for (const d of tl.deaths) c.fillRect(Math.floor(d / tl.n * W), stripY - 4, 1.5, stripH + 4);
  // labels
  c.fillStyle = col("--text-muted");
  c.font = "11px Segoe UI";
  c.textBaseline = "middle";
  for (const [t, ty] of [[`${tl.cap.toFixed(1)}° off`, top + 7], ["0°: on the bot", top + chartH - 8]]) {
    c.fillStyle = "rgba(0,0,0,0.6)";                    // readable over the spikes
    c.fillRect(2, ty - 7, c.measureText(t).width + 6, 14);
    c.fillStyle = col("--text-secondary");
    c.fillText(t, 5, ty);
  }
  c.textBaseline = "alphabetic";
  const secs = tl.n / tl.fps, step = secs > 90 ? 20 : 10;
  for (let t = 0; t <= secs; t += step) {
    const x = t / secs * W;
    c.textAlign = t === 0 ? "left" : x > W - 20 ? "right" : "center";
    c.fillText(`${t} s`, Math.min(W - 1, x), H - 4);
  }
  c.textAlign = "left";
  tlHead();
}

function tlHead() {
  const head = $("#track-tl .tl-head");
  if (!head || !tl) return;
  const k = shownTime * tl.fps - tl.a;
  head.hidden = k < 0 || k > tl.n;
  head.style.left = `${(100 * k / tl.n).toFixed(3)}%`;
}

// A tracking card: its title (bold white), its value (white), then a detail and its explanation in grey (the user's
// layout, 2026-10-02); the explanation also on hover.
const tvTile = (k, v, sub, why) => `<div class="tile tile-tv" title="${esc(why)}"><div class="t">${k}</div>
  <div class="v2">${esc(String(v))}</div>${sub ? `<div class="k">${esc(sub)}</div>` : ""}<div class="why">${esc(why)}</div></div>`;

// The tracking diagnostics (review.track_motion): the target's own motion and the mouse's, read from the video.
function motionHtml(m, deg) {
  if (!m) return "";
  const head = `<h3>How you followed the target</h3>`;
  const cam = `the camera's turn was read in ${pct(m.camera)} of the frames`;
  if (m.reason) return `${head}<p class="note">Not measured: ${esc(m.reason)} (${(m.seconds ?? 0).toFixed(1)} s); ${cam}.</p>`;
  const side = (v) => (v == null ? "–" : `${Math.abs(v).toFixed(2)}° ${v < 0 ? "behind" : "ahead"}`);
  const rate = (v) => (v == null ? "–" : v.toFixed(2));
  const tiles = [
    ["Behind or ahead", side(m.lag), m.lag_ms == null ? "" : `${Math.abs(Math.round(m.lag_ms))} ms at its speed`,
      "Where your crosshair usually sat along the bot's motion: behind means trailing it, ahead means leading it."],
    ["Off target: behind", pct(m.off_behind), "", "Trailing: the share of the off-target time spent behind the bot."],
    ["Off target: ahead", pct(m.off_ahead), "", "Leading: the share of the off-target time spent ahead of the bot, past its edge."],
    ["Off target: to the side", pct(m.off_side), "",
      "Share of the off-target time spent beside the bot's path (above or below a bot moving sideways)."],
    ["Overshoots a second", rate(m.overshoots), m.overshoot_dist == null ? "" : `${deg(m.overshoot_dist)} past the edge`,
      "How often you went ahead of the bot past its edge, per second of tracking."],
    ["Over-correcting", pct(m.overcorrect), m.corrections == null ? "" : `${m.swing_count} of ${m.corrections} corrections`,
      "Of your corrections (each turn of your crosshair back toward the bot's middle), the share that went too far: " +
      "across the middle to the other side by half the bot's width or more, while the bot kept its direction."],
    ["Reaction to direction changes", m.reaction == null ? "–" : `${Math.round(m.reaction)} ms`, `${m.reversals} changes`,
      "Usual time from the bot turning until your mouse moved the new way."],
    ["Carried past at direction changes", pct(m.reversal_overshoot),
      m.reversal_overshoot_dist == null ? "" : `${deg(m.reversal_overshoot_dist)} past the edge`,
      "How often, when the bot turned, you kept going the old way past its edge."],
    ["Horizontal distance from the line", deg(m.error_h), "",
      "How far left or right of the bot's middle line you usually were, when on or near it."],
    ["Vertical distance from the line", deg(m.error_v), "",
      "How far above or below it you usually were. On a capsule, anywhere along its length counts as 0."],
    ["Target speed", m.target_speed == null ? "–" : `${Math.round(m.target_speed)} °/s`, "its own motion",
      "How fast the bot itself usually moved, apart from your mouse."]];
  const rows = (m.by_direction || []).map((b) => `<tr><td>${ARROWS[b.name]} ${b.name}</td><td>${pct(b.share)}</td>
    <td>${pct(b.on)}</td><td>${deg(b.distance)}</td><td>${side(b.lag)}</td></tr>`).join("");
  return `${head}
    <div class="tiles">${tiles.map(([k, v, sub, why]) => tvTile(k, v, sub, why)).join("")}</div>
    <p class="note">Read from the video alone: the room's slide across the screen gives the camera's turn (your mouse),
      and the target's move on screen less that gives its own motion. Measured while the target moves and your
      crosshair is with it (within 2° of it, ${(m.seconds ?? 0).toFixed(0)} s here); ${cam}. Behind or ahead is the
      median offset along the target's motion. An overshoot is a stretch ahead of the target past its leading edge. A
      swing is the crosshair crossing from behind the target to ahead of it, or back, by half its width or more, while it
      keeps its direction. At a direction change, the reaction is the time until your mouse moves the new way, and
      "carried past" counts the changes after which the crosshair went on the old way past the target's edge. Distances
      are from the target's center line (a sphere's center, a capsule's long axis).</p>
    <h3>By the target's direction</h3>
    <p class="note">How you did while the bot moved each way: the share of the time it moved that way, your time on it,
      your usual distance from its middle line, and whether you trailed or led it.</p>
    <table><thead><tr><th>Target moving</th><th>Time</th><th>On target</th><th>Distance from the line</th>
      <th>Behind or ahead</th></tr></thead><tbody>${rows}</tbody></table>`;
}

// A tracking run's video: the bot the review takes for the target (the track nearest the crosshair, by its center
// line, as review.track_summary does) boxed in green while the crosshair is on it and orange while off, with a dashed
// line from the crosshair and the distance; the other detected targets in thin grey; "switching" after a bot's death
// until the crosshair is on a target again.
function drawTracked(c, scale) {
  tlHead();
  if (!tracks || !$("#show-overlay").checked) return;
  drawFaint(c, scale);                                  // what the cut-off leaves out, dimmed
  const s = report.summary, fi = Math.round(shownTime * report.fps), f = tracks.frames[fi];
  if (!f || (s.start != null && (fi < s.start || fi >= s.end))) return;
  const g = report.geometry, css = getComputedStyle(document.documentElement);
  const green = css.getPropertyValue("--series-3").trim(), orange = css.getPropertyValue("--series-2").trim();
  let best = null;
  const boxes = [];
  f.t.forEach(([id, x, y], k) => {
    const [w, h] = f.wh ? f.wh[k] : [0.6, 0.6];
    const lx = Math.sign(x) * Math.max(0, Math.abs(x) - Math.max(0, w - h) / 2);
    const ly = Math.sign(y) * Math.max(0, Math.abs(y) - Math.max(0, h - w) / 2);
    const d = Math.hypot(lx, ly), inside = Math.abs(x) <= w / 2 + 0.05 && Math.abs(y) <= h / 2 + 0.05;
    const box = { k, x, y, w, h, d, inside, out: Math.max(0, d - Math.min(w, h) / 2) };   // outside its edge
    if (!best || d < best.d) best = box;
    box.corner = [toPx(x - w / 2, y + h / 2, g, scale), toPx(x + w / 2, y - h / 2, g, scale)];
    boxes.push(box);
  });
  for (const box of boxes) {
    const { corner: [[x0, y0], [x1, y1]] } = box, main = box === best, pad = 1.5;   // tight: the edge is the edge
    c.strokeStyle = main ? (best.inside ? green : orange) : "rgba(255,255,255,0.4)";
    c.lineWidth = main ? 2 : 1;
    c.strokeRect(x0 - pad, y0 - pad, x1 - x0 + 2 * pad, y1 - y0 + 2 * pad);
  }
  const switching = (s.switches || []).some(([a, b]) => fi >= a && fi < b);
  const [cx, cy] = toPx(0, 0, g, scale);
  c.font = "11px Segoe UI";
  if (best && !best.inside) {
    const [tx, ty] = toPx(best.x, best.y, g, scale);
    c.strokeStyle = orange;
    c.lineWidth = 1;
    c.setLineDash([4, 4]);
    c.beginPath(); c.moveTo(cx, cy); c.lineTo(tx, ty); c.stroke();
    c.setLineDash([]);
  }
  const label = switching ? "switching" : best ? (best.inside ? "on" : `${best.out.toFixed(2)}° off its edge`) : "";
  if (label) {
    const [x1, y1] = best ? best.corner[1] : [cx, cy];
    c.fillStyle = "rgba(0,0,0,0.6)";
    c.fillRect(x1 + 6, y1 - 4, c.measureText(label).width + 8, 15);
    c.fillStyle = "#fff";
    c.fillText(label, x1 + 10, y1 + 7);
  }
}

// The cards under the speed chart: the whole run, or the picked kill next to the run's medians.
let flickTiles = null;
function renderTiles(m) {
  flickTiles = m;
  const s = report.summary;
  const deg = (v) => (v == null ? "–" : `${Math.round(v)} °/s`);
  const tiles = m
    ? [["Distance", `${m.D0.toFixed(1)}° ${arrow(m.dir)}`], ["Kill time", ms(m.total), ms(s.median_interval)],
      ["Reaction", ms(m.react), ms(s.react)], ["Main flick", ms(m.flick), ms(s.flick)],
      ["Main flick ended", ended(m, s.radius)], ["Still before the click", ms(m.still), ms(s.still)],
      ["Peak speed", deg(m.peak), deg(s.peak)], ["Click speed", deg(m.click_speed), deg(s.click_speed)],
      ["Shots", m.shots ?? "–"], ["Path cost", orderText(m.n)]]
    : [["Score", num(s.score) ?? "–"], ["Kills", num(s.kills)], ["Misses", num(s.misses) ?? "–"], ["Median kill", ms(s.median_interval)],
      ["Still before the click", ms(s.still)], ["Peak speed", deg(s.peak)], ["Game FPS", s.fps_avg ? Math.round(s.fps_avg) : "–"],
      ["Fastest next target", orders ? pct(orders.share) : "…"],
      ["Path cost in all", orders ? ms(orders.total) : "…"],
      [`More ${s.shots == null ? "kills" : "shots"} with the best path`, orders ? `≈ ${extraShots(orders.total).toFixed(1)}` : "…"]];
  $("#tiles").innerHTML = `
    <div class="tiles-head">${m ? `<b>Kill ${m.n}</b><span class="muted">run median below each value</span>
      <button id="whole-run">Whole run</button>` : `<b>Whole run</b><span class="muted">pick a kill for its own numbers</span>`}</div>
    <div class="tiles">${tiles.map(([k, v, med]) => `<div class="tile"><div class="v" title="${esc(v)}">${esc(v)}</div><div class="k">${k}</div>
      <div class="k">${med ? `run ${esc(med)}` : "&nbsp;"}</div></div>`).join("")}</div>`;
  $("#whole-run")?.addEventListener("click", () => renderTiles(null));
  renderBudget(m);
  keepHeight($("#tiles"));
  keepHeight($("#budget-box"));
}

// Grow, never shrink: a box whose content changes with the picked kill keeps its tallest height, so following the
// video never moves the page below it. A new VOD's report starts with new boxes.
function keepHeight(el) {
  el.style.minHeight = `${Math.max(parseFloat(el.style.minHeight) || 0, el.offsetHeight)}px`;
}

// Where the time goes: the run's average kill, or the picked kill with the average under it on the same time scale.
function renderBudget(m) {
  const avg = report.summary.budget;
  if (!avg) { $("#budget-box").innerHTML = ""; return; }
  const sum = (p) => p.reduce((a, b) => a + b, 0);
  const bar = (p, width, cls = "") => `<div class="budget ${cls}" style="width:${width}%">${p.map((v, i) =>
    `<div style="flex:${v};background:var(${PARTS[i][1]})" title="${PARTS[i][0]}: ${ms(v)}">
      ${cls ? "" : v / sum(p) > 0.12 ? `${PARTS[i][0]} ${ms(v)}` : ""}</div>`).join("")}</div>`;
  // hide: keep the "(avg ...)" text but invisible, so the legend wraps as it does for a kill
  const legend = (p, ref, hide = false) => `<div class="legend">${p.map((v, i) => `<span><i style="background:var(${PARTS[i][1]})"></i>
    ${PARTS[i][0]} ${ms(v)}${ref ? ` <span class="muted"${hide ? ' style="visibility:hidden"' : ""}>(avg ${ms(ref[i])})</span>` : ""}</span>`).join("")}</div>`;
  if (!m) {
    // the average bar's row is kept (hidden), so the box is as tall as a kill's
    $("#budget-box").innerHTML = `<h3>Where an average kill's ${ms(sum(avg))} goes</h3>${bar(avg, 100)}
      <div class="budget-ref" style="visibility:hidden"><span class="muted">Average kill</span>${bar(avg, 100, "thin")}</div>
      ${legend(avg, avg, true)}`;
    return;
  }
  if (!m.parts) {
    $("#budget-box").innerHTML = `<h3>Where kill ${m.n}'s ${ms(m.total)} goes</h3>
      <p class="note">One of this kill's steps (the reaction, the arrival or the stop on the target) was not found, so its time can't be split. The run's average:</p>
      ${bar(avg, 100)}${legend(avg)}`;
    return;
  }
  const top = Math.max(sum(m.parts), sum(avg));
  $("#budget-box").innerHTML = `<h3>Where kill ${m.n}'s ${ms(m.total)} goes</h3>
    ${bar(m.parts, 100 * sum(m.parts) / top)}
    <div class="budget-ref"><span class="muted">Average kill, ${ms(sum(avg))}</span>${bar(avg, 100 * sum(avg) / top, "thin")}</div>
    ${legend(m.parts, avg)}`;
}

function arrow(dir) {
  const a = ((dir % 360) + 360) % 360;
  return "→↗↑↖←↙↓↘"[Math.round(a / 45) % 8];
}

function ended(m, R) {
  if (m.end_left > R) return `short, ${m.end_left.toFixed(1)}° to go`;
  if (m.end_left < -R) return `past, by ${(-m.end_left - R).toFixed(1)}°`;
  return "on the target";
}

// ---- flick player ---------------------------------------------------------------------------------------------------
function playFlick(n) {
  selectFlick(report.flicks.find((m) => m.n === n));
  const fps = report.fps;
  const t0 = Math.max(0, flick.start_frame / fps - 0.15), t1 = flick.kill_frame / fps + 1;   // and 1 s after the kill
  stopAt = t1;
  video.currentTime = t0;
  video.playbackRate = rate;
  video.play().catch(() => {});                        // a hidden page may refuse to play
  $("#player").scrollIntoView({ behavior: "smooth", block: "nearest" });
}

// One kill becomes the current one: its row, label, speed chart, cards and ring.
function selectFlick(m, center = false) {
  flick = m;
  document.querySelectorAll("#flicks tr.sel").forEach((t) => t.classList.remove("sel"));
  const row = document.querySelector(`#flicks tr[data-n="${m.n}"]`);
  row?.classList.add("sel");
  if (row && center) {                                 // scroll the list only, never the page
    const box = $("#flick-scroll");
    const d = row.getBoundingClientRect().top - box.getBoundingClientRect().top;
    box.scrollTop += d - box.clientHeight / 2 + row.offsetHeight / 2;
  }
  $("#flick-label").textContent = `Kill ${m.n}: ${m.D0.toFixed(1)}° ${arrow(m.dir)}, ${ms(m.total)}`;
  drawSpeed();
  renderTiles(m);
}

// "Follow the video in the list": the kill on screen is the latest flick to have started by this frame. A flick
// replayed from the list keeps the list until it stops.
const followBox = $("#follow");
followBox.checked = localStorage.getItem("vod-follow") !== "0";
followBox.addEventListener("change", () => {
  localStorage.setItem("vod-follow", followBox.checked ? "1" : "0");
  follow();
});
function follow() {
  if (!report || !followBox.checked || stopAt != null) return;
  const f = shownTime * report.fps;
  let m = null;
  for (const q of report.flicks) if (q.start_frame <= f && (!m || q.start_frame > m.start_frame)) m = q;
  if (m && m !== flick) selectFlick(m, true);
}

document.querySelectorAll("#controls button[data-rate]").forEach((b) => b.addEventListener("click", () => {
  rate = +b.dataset.rate;
  video.playbackRate = rate;
  document.querySelectorAll("#controls button[data-rate]").forEach((x) => x.classList.toggle("on", x === b));
}));
video.addEventListener("click", () => (video.paused ? video.play().catch(() => {}) : video.pause()));

// The frame on screen, from the video's own frame callback when the browser has it.
let shownTime = 0;
function onFrame(now, meta) {
  shownTime = meta ? meta.mediaTime : video.currentTime;
  follow();                                            // before a replay's stop is cleared, so the list stays put
  if (stopAt != null && shownTime >= stopAt) {
    video.pause();
    stopAt = null;
  }
  drawOverlay();
  playhead();
  seekBar();
  if (video.requestVideoFrameCallback) video.requestVideoFrameCallback(onFrame);
}
if (video.requestVideoFrameCallback) video.requestVideoFrameCallback(onFrame);
else (function loop() { onFrame(0, null); requestAnimationFrame(loop); })();
video.addEventListener("seeked", () => { shownTime = video.currentTime; follow(); drawOverlay(); playhead(); seekBar(); });

// ---- seek bar: drag anywhere in the VOD; a tick per kill, the selected one lit ------------------------------------
const seek = $("#seek");
let seeking = false;
const clock = (t) => `${Math.floor(t / 60)}:${(t % 60).toFixed(1).padStart(4, "0")}`;
function seekBar() {
  if (!seeking) seek.value = shownTime;
  $("#time-now").textContent = clock(shownTime);
  document.querySelectorAll("#kill-marks i.sel").forEach((i) => i.classList.remove("sel"));
  if (flick) document.querySelector(`#kill-marks i[data-n="${flick.n}"]`)?.classList.add("sel");
}
function killMarks() {
  const d = video.duration;
  if (!report || !(d > 0)) { $("#kill-marks").innerHTML = ""; return; }
  $("#kill-marks").innerHTML = report.mode === "track"           // a tracking run: where its bots died
    ? (report.summary.switches || []).map(([f]) => `<i style="left:${(100 * f / report.fps / d).toFixed(3)}%"></i>`).join("")
    : report.flicks.map((m) =>
      `<i data-n="${m.n}" style="left:${(100 * m.kill_frame / report.fps / d).toFixed(3)}%"></i>`).join("");
  seekBar();
}
video.addEventListener("loadedmetadata", () => {
  seek.max = video.duration;
  $("#time-end").textContent = clock(video.duration).replace(/\.\d$/, "");
  killMarks();
});
seek.addEventListener("input", () => {
  seeking = true;
  stopAt = null;                                       // dragging ends a flick replay
  video.currentTime = +seek.value;
});
seek.addEventListener("change", () => { seeking = false; });
video.addEventListener("timeupdate", seekBar);

function toPx(xd, yd, g, scale) {
  const x = g.CX + g.K * Math.tan(xd * Math.PI / 180);
  const y = g.CY - Math.tan(yd * Math.PI / 180) * Math.hypot(g.K, x - g.CX);
  return [x * scale, y * scale];
}

function drawOverlay() {
  const w = overlay.clientWidth, h = overlay.clientHeight, dpr = devicePixelRatio || 1;
  if (overlay.width !== w * dpr) { overlay.width = w * dpr; overlay.height = h * dpr; }
  const c = overlay.getContext("2d");
  c.setTransform(dpr, 0, 0, dpr, 0, 0);
  c.clearRect(0, 0, w, h);
  if (excluding) return drawExcluded(c, w, h);
  if (report?.mode === "track") return drawTracked(c, w / report.geometry.W);
  if (report && (orderBox.checked || mineBox.checked)) drawOrder(c, w / report.geometry.W);
  if (report) drawFaint(c, w / report.geometry.W);
  if (!report || !flick || !$("#show-overlay").checked) return;
  const g = report.geometry, scale = w / g.W, path = report.paths[String(flick.n)];
  const frame = Math.round(shownTime * report.fps);
  if (frame < flick.start_frame - 30 || frame > flick.kill_frame + 30) return;
  const R = report.summary.radius;
  // the target's path on screen so far is meaningless (the view moves); draw where it is now, as a ring
  const p = path.find((q) => q[0] === frame) || (frame >= flick.kill_frame ? null : null);
  const [cx, cy] = toPx(0, 0, g, scale);
  c.strokeStyle = "rgba(255,255,255,0.35)";
  c.lineWidth = 1;
  c.beginPath(); c.arc(cx, cy, 10, 0, 2 * Math.PI); c.stroke();
  if (!p) return;
  const [x, y] = toPx(p[1], p[2], g, scale);
  const r = Math.max(6, (toPx(p[1] + R, p[2], g, scale)[0] - x) * 1.8);
  c.strokeStyle = getComputedStyle(document.documentElement).getPropertyValue("--warning").trim();
  c.lineWidth = 2;
  c.beginPath(); c.arc(x, y, r, 0, 2 * Math.PI); c.stroke();
  c.beginPath(); c.moveTo(cx, cy); c.lineTo(x, y); c.setLineDash([4, 4]); c.stroke(); c.setLineDash([]);
  c.fillStyle = "#fff";
  c.font = "12px Segoe UI";
  c.fillText(`${Math.hypot(p[1], p[2]).toFixed(1)}°`, x + r + 4, y - 4);
}

// ---- fastest path -----------------------------------------------------------------------------------------------------
// The order through every target on screen that takes the least time, from the crosshair. A flick's time follows
// Fitts' law, t = a + b * log2(1 + D / W) (D the flick's distance, W the target's width), fitted to this run's kills.
// a is paid once per kill whatever the order, so the fastest order is the one with the smallest sum of log2 terms.
// Two switches: the fastest path (green) and the path you took (orange), each drawn on its own.
const orderBox = $("#show-order"), mineBox = $("#show-mine");
orderBox.checked = localStorage.getItem("vod-order") === "1";
mineBox.checked = localStorage.getItem("vod-mine") !== "0";
for (const [box, key] of [[orderBox, "vod-order"], [mineBox, "vod-mine"]]) {
  box.addEventListener("change", () => {
    localStorage.setItem(key, box.checked ? "1" : "0");
    loadTracks();
    drawOverlay();
  });
}

// A frame with only the tracks keep says, their areas, box sizes and scores with them.
const keepTracks = (f, keep) => {
  const out = { ...f };
  for (const k of ["t", "a", "wh", "s"]) if (f[k]) out[k] = f[k].filter((_, i) => keep[i]);
  return out;
};

async function loadTracks() {
  if (!report || tracks?.id === current) return;
  const id = current;
  const t = await api(`/api/tracks?id=${encodeURIComponent(id)}`);
  if (!t || id !== current) return;
  // the crosshair itself, where the detector marks it as a target (review.crosshair_spots), is not a target to clear
  const spots = report.crosshair || [];
  const ghost = ([, x, y]) => spots.some(([a, b]) => Math.hypot(x - a, y - b) < 0.1);
  const all = spots.length ? t.frames.map((f) => keepTracks(f, f.t.map((q) => !ghost(q)))) : t.frames;
  tracks = { id, all, frames: all };
  faint = faintScores(all);
  const saved = await api(`/api/faint?id=${encodeURIComponent(id)}`);
  if (id !== current) return;
  faint.on = !!saved?.on;
  faint.offset = saved?.offset ?? 0.3;
  faint.submitted = !!saved?.submitted;
  applyFaint();
  renderTimeline();
}

// Every track's score (the rule's), one dot each along the score axis, the cut as a line: where the seams and the
// targets sit. A dot's size follows how long the track was seen; clicking one shows it in the video.
function renderStrip(cut) {
  const svg = $("#faint-strip"), W = Math.max(200, svg.clientWidth || 600), H = 52, lo = 0.2, hi = 1.0;
  const X = (v) => 8 + (W - 16) * (Math.min(hi, Math.max(lo, v)) - lo) / (hi - lo);
  const css = getComputedStyle(document.documentElement);
  const kept = css.getPropertyValue("--series-3").trim(), muted = css.getPropertyValue("--text-muted").trim();
  const dots = [...faint.q].map(([id, v]) => {
    const n = faint.n.get(id), y = 8 + ((id * 2654435761) % 1000) / 1000 * 26, r = Math.min(6, 1.5 + Math.sqrt(n) / 4);
    return `<circle data-id="${id}" cx="${X(v).toFixed(1)}" cy="${y.toFixed(1)}" r="${r.toFixed(1)}"
      fill="${v < cut ? muted : kept}" fill-opacity="${v < cut ? 0.6 : 0.85}"${faint.hl === id ? ' stroke="#fff" stroke-width="2"' : ""}>
      <title>Track ${id}: score ${v.toFixed(2)}, seen in ${n} frames away from the crosshair. Click to show it</title></circle>`;
  }).join("");
  const ticks = [0.2, 0.4, 0.6, 0.8, 1.0].map((t) => `<text x="${X(t)}" y="${H - 2}" font-size="10" fill="${muted}"
    text-anchor="middle">${t.toFixed(1)}</text>`).join("");
  svg.setAttribute("viewBox", `0 0 ${W} ${H}`);
  svg.innerHTML = `<line x1="8" x2="${W - 8}" y1="38" y2="38" stroke="var(--grid)" />${ticks}${dots}
    <line x1="${X(cut)}" x2="${X(cut)}" y1="2" y2="40" stroke="var(--warning)" stroke-width="2">
      <title>The cut: ${cut.toFixed(2)}</title></line>`;
}
$("#faint-strip").addEventListener("click", (e) => {
  const c = e.target.closest("circle[data-id]");
  if (!c || !tracks) return;
  const id = +c.dataset.id, seen = [];
  tracks.all.forEach((f, i) => { if (f.t.some((t) => t[0] === id)) seen.push(i); });
  if (!seen.length) return;
  faint.hl = id;
  video.pause();
  video.currentTime = (seen[Math.floor(seen.length / 2)] + 0.5) / report.fps;
  applyFaint();
});

// The faint-target cut-off (the user's setting, per recording; off by default). Each track's score is the 90th
// percentile of the model's scores for it away from the crosshair (a target under the crosshair scores low); the
// recording's level is the 90th percentile of those, weighted by frames; tracks scoring more than the chosen offset
// below the level are left out of the paths and drawn dimmed. Wall seams on a tiled theme score far below the
// targets (1wall 6targets extra small 889.26: seams 0.30 to 0.37, targets 0.83 and up with full_v3), but moving
// targets and targets cut by the screen's edge can score low too (0.41 to 0.61), so the user sets the offset.
function faintScores(frames) {
  if (!frames.some((f) => f.s)) return { has: false, q: new Map() };
  const sc = new Map(), near = report?.mode === "track" ? 0 : 2;    // tracking: the bot is under the crosshair
  for (const f of frames) {
    if (!f.s) continue;
    f.t.forEach(([id, x, y], k) => {
      if (Math.hypot(x, y) < near) return;
      if (!sc.has(id)) sc.set(id, []);
      sc.get(id).push(f.s[k]);
    });
  }
  const q = new Map(), n = new Map();
  for (const [id, v] of sc) {
    if (v.length < 3) continue;
    v.sort((a, b) => a - b);
    q.set(id, v[Math.min(v.length - 1, Math.floor(0.9 * (v.length - 1) + 0.5))]);
    n.set(id, v.length);
  }
  const order = [...q.keys()].sort((a, b) => q.get(a) - q.get(b));
  const total = order.reduce((a, id) => a + n.get(id), 0);
  let acc = 0, level = null;
  for (const id of order) { acc += n.get(id); if (acc >= 0.9 * total) { level = q.get(id); break; } }
  return { has: level != null, q, n, level, on: false, offset: 0.3, dropped: new Set(), hl: null, hover: null };
}

function applyFaint() {
  const box = $("#faint-box"), on = $("#faint-on"), range = $("#faint-offset"), info = $("#faint-info");
  // shown when asked for (Set cut-off), in the queue, or while this recording's cut-off is on
  box.hidden = !(faintOpen || faintQ || faint?.on) || !(tracks || faintQ);
  $("#cutoff").classList.toggle("on", !box.hidden);
  on.disabled = range.disabled = $("#faint-submit").disabled = $("#faint-next").disabled = !faint?.has;
  if (!faint?.has) {
    info.textContent = tracks ? "Review again to get the model's scores" : "Reviewing: the scores come with the review";
    if (tracks) tracks.frames = tracks.all;
    $("#faint-strip").innerHTML = "";
  } else {
    on.checked = faint.on;
    range.value = faint.offset;
    const cut = faint.level - faint.offset;
    faint.dropped = new Set(faint.on ? [...faint.q].filter(([, v]) => v < cut).map(([id]) => id) : []);
    tracks.frames = faint.dropped.size ? tracks.all.map((f) => keepTracks(f, f.t.map(([id]) => !faint.dropped.has(id))))
      : tracks.all;
    const below = [...faint.q.values()].filter((v) => v < cut).length;
    info.textContent = `offset ${faint.offset.toFixed(2)} · cut ${cut.toFixed(2)} (targets ${faint.level.toFixed(2)}) · ` +
      `${faint.on ? "" : "would leave out "}${below} of ${faint.q.size} tracks${faint.submitted ? " · submitted" : ""}`;
    renderStrip(cut);
  }
  if (tracks && report?.flicks?.length) {
    orderCache = null;
    analyseOrders();
  }
  if (report?.mode === "track") renderTimeline();        // the highlight and the timeline follow the cut at once
  drawOverlay();
}

let faintSave = null;
for (const [el, ev] of [[$("#faint-on"), "change"], [$("#faint-offset"), "input"]]) {
  el.addEventListener(ev, () => {
    if (!faint?.has) return;
    faint.on = $("#faint-on").checked;
    faint.offset = +$("#faint-offset").value;
    applyFaint();
    const id = current, body = JSON.stringify({ on: faint.on, offset: faint.offset });
    clearTimeout(faintSave);
    faintSave = setTimeout(async () => {
      await fetch(`/api/faint?id=${encodeURIComponent(id)}`, { method: "POST", body });
      if (report?.mode === "track" && id === current) watchJob();   // its cards are measured again with the cut
    }, 400);
  });
}

// The tracks the cut-off leaves out, in the shown frame: dimmed dashed rings, so the user sees what it removes. With
// "Show scores", every track's score (the rule's) beside it; the track picked in the strip in orange; the point under
// the mouse with that frame's own score.
function drawFaint(c, scale) {
  if (!faint?.has || !tracks) return;
  const f = tracks.all[Math.round(shownTime * report.fps)];
  if (!f) return;
  const g = report.geometry, scores = $("#faint-scores").checked;
  c.font = "11px Segoe UI";
  f.t.forEach(([id, x, y]) => {
    const [px, py] = toPx(x, y, g, scale), out = faint.dropped.has(id), q = faint.q.get(id);
    if (out || id === faint.hl) {
      c.strokeStyle = id === faint.hl ? getComputedStyle(document.documentElement).getPropertyValue("--warning").trim()
        : "rgba(255,255,255,0.6)";
      c.lineWidth = id === faint.hl ? 2 : 1.2;
      c.setLineDash(id === faint.hl ? [] : [2, 3]);
      c.beginPath(); c.arc(px, py, 9, 0, 2 * Math.PI); c.stroke();
      c.setLineDash([]);
    }
    if (scores || id === faint.hl) {
      const t = q == null ? "–" : q.toFixed(2);
      c.fillStyle = "rgba(0,0,0,0.55)";
      c.fillRect(px + 10, py - 16, c.measureText(t).width + 6, 14);
      c.fillStyle = out ? "rgba(255,255,255,0.6)" : "#fff";
      c.fillText(t, px + 13, py - 5);
    }
  });
  const h = faint.hover;
  if (h && h.fi === Math.round(shownTime * report.fps)) {    // only on the frame it was read from
    c.fillStyle = "rgba(0,0,0,0.75)";
    c.fillRect(h.x + 12, h.y + 8, c.measureText(h.text).width + 10, 18);
    c.fillStyle = "#fff";
    c.fillText(h.text, h.x + 17, h.y + 21);
  }
}

overlay.parentElement.addEventListener("mousemove", (e) => {
  if (!faint?.has || !tracks || excluding) return;
  const r = overlay.getBoundingClientRect(), scale = r.width / report.geometry.W;
  const mx = e.clientX - r.left, my = e.clientY - r.top, fi = Math.round(shownTime * report.fps), f = tracks.all[fi];
  let best = null;
  (f?.t || []).forEach(([id, x, y], k) => {
    const [px, py] = toPx(x, y, report.geometry, scale), d = Math.hypot(px - mx, py - my);
    if (d <= 12 && (!best || d < best.d)) best = { d, id, s: f.s?.[k], px, py };
  });
  const text = best ? `track ${best.id} · this frame ${best.s == null ? "–" : best.s.toFixed(2)} · ` +
    `track ${faint.q.has(best.id) ? faint.q.get(best.id).toFixed(2) : "–"}` : null;
  if ((faint.hover?.text ?? null) === text) return;
  faint.hover = best ? { x: best.px, y: best.py, text, fi } : null;
  drawOverlay();
});
overlay.parentElement.addEventListener("mouseleave", () => { if (faint?.hover) { faint.hover = null; drawOverlay(); } });
$("#faint-scores").addEventListener("change", drawOverlay);

// Submit: the cut-off is saved (on) and written as detector labels (server.submit_faint). The queue goes through
// recordings in the area queue's order; a recording without the model's scores is reviewed again first.
// The run's window, marked by the user (server run.json): start and end, or one of them and the length. Any two
// settle the third; a start or end alone takes the stats file's or the scenario's length. Saving measures the run
// again on its tracks (a few seconds), and "Automatic" forgets the marks.
const parseTime = (t) => {
  t = String(t).trim();
  if (!t) return null;
  const p = t.split(":").map(Number);
  const v = p.length === 2 ? 60 * p[0] + p[1] : p[0];
  return Number.isFinite(v) && v >= 0 ? v : NaN;
};
async function openRun() {
  const box = $("#run-box");
  box.hidden = !box.hidden;
  $("#runbtn").classList.toggle("on", !box.hidden);
  if (box.hidden) return;
  const saved = await api(`/api/run?id=${encodeURIComponent(current)}`).catch(() => null) || {};
  $("#run-start").value = saved.start != null ? clock(saved.start) : "";
  $("#run-end").value = saved.end != null ? clock(saved.end) : "";
  $("#run-len").value = saved.length != null ? saved.length : "";
  const s = report?.summary || {}, fps = report?.fps || 60;
  const marked = saved.start != null || saved.end != null || saved.length != null;
  $("#run-info").textContent = report?.mode === "track" && s.start != null
    ? `Now: ${clock(s.start / fps)} to ${clock(s.end / fps)} (${((s.end - s.start) / fps).toFixed(1)} s), ${marked ? "from your marks" : "found by the review"}`
    : report ? `Used for the path overlay${marked ? "; marked" : ""}` : "";
  box.scrollIntoView({ block: "nearest", behavior: "smooth" });
}
$("#runbtn").addEventListener("click", openRun);
$("#run-start-here").addEventListener("click", () => ($("#run-start").value = clock(video.currentTime)));
$("#run-end-here").addEventListener("click", () => ($("#run-end").value = clock(video.currentTime)));
async function saveRun(body) {
  try {
    await api(`/api/run?id=${encodeURIComponent(current)}`, { method: "POST", body: JSON.stringify(body) });
  } catch (e) {
    return ($("#run-info").textContent = `Could not save: ${e.message}`);
  }
  $("#run-box").hidden = true;
  $("#runbtn").classList.remove("on");
  watchJob();                                           // measured again on its tracks, then the report reloads
}
$("#run-save").addEventListener("click", () => {
  const body = { start: parseTime($("#run-start").value), end: parseTime($("#run-end").value),
    length: $("#run-len").value.trim() ? +$("#run-len").value : null };
  if ([body.start, body.end, body.length].some((v) => Number.isNaN(v)))
    return ($("#run-info").textContent = "Times as 1:02.5 or 62.5 seconds");
  saveRun(body);
});
$("#run-auto").addEventListener("click", () => saveRun({ start: null, end: null, length: null }));

let faintQ = null, faintPos = 0, faintOpen = false;
$("#cutoff").addEventListener("click", () => {             // this recording's cut-off, without the queue
  faintOpen = $("#faint-box").hidden;
  applyFaint();
  if (faintOpen) $("#faint-box").scrollIntoView({ block: "nearest", behavior: "smooth" });
});
async function submitFaint() {
  if (!faint?.has) return false;
  try {
    await api(`/api/faint_submit?id=${encodeURIComponent(current)}&offset=${faint.offset}`, { method: "POST" });
  } catch (e) {
    showProgress(`Could not submit: ${e.message}`, 0);
    return false;
  }
  faint.on = faint.submitted = true;
  applyFaint();
  showProgress(`Submitted: cut ${(faint.level - faint.offset).toFixed(2)} saved; its labels are being written`, 1);
  return true;
}
$("#faint-submit").addEventListener("click", submitFaint);
$("#set-cutoffs").addEventListener("click", async () => {
  faintQ = await api("/api/faint_queue");
  faintPos = 0;
  if (!faintQ.length) { faintQ = null; return showProgress("Every recording has a submitted cut-off already", 1); }
  faintNext();
});
async function faintNext() {
  if (!faintQ) return;
  if (faintPos >= faintQ.length) {
    const n = faintQ.length;
    stopFaintQueue();
    return showProgress(`Went through all ${n} recordings`, 1);
  }
  const id = faintQ[faintPos];
  for (const k of ["faint-next", "faint-skip", "faint-pos"]) $(`#${k}`).hidden = false;
  $("#faint-submit").hidden = true;
  $("#faint-pos").textContent = `Recording ${faintPos + 1} of ${faintQ.length}`;
  await select(id);
  applyFaint();
  const v = vods.find((x) => x.id === id);
  if (!v?.analysed) {                                     // not reviewed yet: review it, the scores come with it
    await api(`/api/analyse?id=${encodeURIComponent(id)}`, { method: "POST" });
    return watchJob();
  }
  for (let k = 0; k < 40 && current === id && tracks?.id !== id; k++) await new Promise((r) => setTimeout(r, 250));
  if (current === id && tracks && !faint?.has) {          // reviewed before the scores were kept: review again
    await api(`/api/analyse?id=${encodeURIComponent(id)}&again=1`, { method: "POST" });
    watchJob();
  }
}
function stopFaintQueue() {
  faintQ = null;
  for (const k of ["faint-next", "faint-skip", "faint-pos"]) $(`#${k}`).hidden = true;
  $("#faint-submit").hidden = false;
}
$("#faint-next").addEventListener("click", async () => {
  if (await submitFaint()) { faintPos++; faintNext(); }
});
$("#faint-skip").addEventListener("click", async () => {      // remembered: the queue does not offer it again
  await api(`/api/faint_skip?id=${encodeURIComponent(current)}`, { method: "POST" }).catch(() => {});
  faintPos++;
  faintNext();
});

function fitFitts() {
  const W = 2 * report.summary.radius;
  const pts = report.flicks.filter((m) => m.total != null && m.D0 > 0).map((m) => [Math.log2(1 + m.D0 / W), m.total]);
  const n = pts.length;
  if (n < 3) return { a: 0, b: 0.1, W };
  const mx = pts.reduce((s, p) => s + p[0], 0) / n, my = pts.reduce((s, p) => s + p[1], 0) / n;
  const sxy = pts.reduce((s, p) => s + (p[0] - mx) * (p[1] - my), 0), sxx = pts.reduce((s, p) => s + (p[0] - mx) ** 2, 0);
  const b = sxx > 0 && sxy > 0 ? sxy / sxx : 0.1;
  return { a: my - b * mx, b, W };
}

const cost0 = (p) => Math.log2(1 + Math.hypot(p[1], p[2]) / fitts.W);                       // from the crosshair
const cost = (p, q) => Math.log2(1 + Math.hypot(p[1] - q[1], p[2] - q[2]) / fitts.W);     // target to target
const seconds = (n, units) => n * fitts.a + fitts.b * units;
const pathUnits = (order) => order.reduce((u, p, i) => u + (i ? cost(order[i - 1], p) : cost0(p)), 0);

// For a set of targets, best[mask][j]: the least cost to visit every target in mask starting at j, and next[mask][j]
// the target after j. It depends only on the targets' places relative to each other, which stay put while the view
// moves, so it is worked out once per set of tracks; each frame only adds the flick from the crosshair.
function orderTable(ts) {
  const n = ts.length, full = (1 << n) - 1;
  const best = new Float64Array((full + 1) * n).fill(Infinity), next = new Int8Array((full + 1) * n).fill(-1);
  for (let j = 0; j < n; j++) best[(1 << j) * n + j] = 0;
  for (let mask = 1; mask <= full; mask++) {
    for (let j = 0; j < n; j++) {
      if (!(mask & (1 << j)) || mask === 1 << j) continue;
      const rest = mask ^ (1 << j);
      for (let k = 0; k < n; k++) {
        if (!(rest & (1 << k))) continue;
        const v = cost(ts[j], ts[k]) + best[rest * n + k];
        if (v < best[mask * n + j]) { best[mask * n + j] = v; next[mask * n + j] = k; }
      }
    }
  }
  return { n, full, best, next };
}

// The best orders through ts: pts (the targets, in the table's order), from[j] = the least cost of the whole set when
// target j goes first, and j0 the best first target. Over 14 targets, only the 14 cheapest to reach are used.
function solve(ts) {
  if (!ts.length) return null;
  if (ts.length > 14) ts = ts.slice().sort((p, q) => cost0(p) - cost0(q)).slice(0, 14);
  const key = ts.map((t) => t[0]).sort((a, b) => a - b).join(",");
  if (orderCache?.key !== key) orderCache = { key, ids: ts.map((t) => t[0]), table: orderTable(ts) };
  const { ids, table } = orderCache, byId = new Map(ts.map((t) => [t[0], t]));
  const pts = ids.map((i) => byId.get(i));
  const from = pts.map((p, j) => cost0(p) + table.best[table.full * table.n + j]);
  const j0 = from.indexOf(Math.min(...from));
  return { pts, table, from, j0 };
}

function fastestOrder(ts) {
  const sol = solve(ts);
  if (!sol) return null;
  const { pts, table } = sol, order = [];
  for (let mask = table.full, j = sol.j0; j >= 0; ) {
    order.push(pts[j]);
    const k = table.next[mask * table.n + j];
    mask ^= 1 << j;
    j = k;
  }
  return { order, seconds: seconds(order.length, sol.from[sol.j0]) };
}

// Your choices. Which tracked target each kill was (its path ends on the track), and for every kill, at the moment
// the next target was picked (3 frames after the flick starts), how much slower clearing the targets on screen gets
// when that one goes first and the rest follow in the best order. 0 when the pick was the fastest first target.
// Only targets on screen at least NEW_MS before you started moving count: a newer one needs a reaction of its own,
// which the model does not price (user, 2026-10-01). A kill whose own target was new gets no cost ("new target").
const NEW_MS = 150;
// the last frame a target may first appear on and still count for flick m: NEW_MS before its movement began
const newCut = (m) => m.start_frame + (m.react ?? 0) * report.fps - NEW_MS / 1000 * report.fps;
function analyseOrders() {
  // when each target first appeared: from the report, which joins a target's track when the tracker lost and found it
  // again (older reports: each track's first frame, which makes a found-again target look new)
  const firstSeen = new Map();
  if (report.appeared) for (const [k, v] of Object.entries(report.appeared)) firstSeen.set(+k, v);
  else tracks.frames.forEach((f, i) => f.t.forEach((t) => { if (!firstSeen.has(t[0])) firstSeen.set(t[0], i); }));
  const killOf = new Map(), trackOf = new Map();
  for (const m of report.flicks) {
    const path = report.paths[String(m.n)];
    const p = path?.[path.length - 1];
    const hit = p && tracks.frames[p[0]]?.t.find((t) => Math.abs(t[1] - p[1]) < 0.05 && Math.abs(t[2] - p[2]) < 0.05);
    if (hit) { killOf.set(hit[0], m); trackOf.set(m.n, hit[0]); }
  }
  const R = report.summary.radius, byKill = new Map();
  for (const m of report.flicks) {
    const id = trackOf.get(m.n), d = m.start_frame + 3;
    if (id == null) continue;
    const cut = newCut(m);
    if (firstSeen.get(id) > cut) { byKill.set(m.n, { cost: 0, best: false, choices: 0, spawned: true }); continue; }
    // the target just killed can linger a frame under the crosshair, and new targets were not options
    const ts = (tracks.frames[d]?.t || []).filter((t) => t[0] === id ||
      (Math.hypot(t[1], t[2]) > R && firstSeen.get(t[0]) <= cut));
    if (!ts.some((t) => t[0] === id)) continue;
    if (ts.length === 1) { byKill.set(m.n, { cost: 0, best: true, choices: 1 }); continue; }
    const sol = solve(ts), j = sol.pts.findIndex((p) => p[0] === id);
    if (j < 0) continue;
    byKill.set(m.n, { cost: fitts.b * (sol.from[j] - sol.from[sol.j0]), best: j === sol.j0, choices: ts.length });
  }
  const v = [...byKill.values()], withChoice = v.filter((x) => x.choices > 1);
  orders = { killOf, trackOf, byKill, firstSeen, total: v.reduce((s, x) => s + x.cost, 0),
    share: withChoice.length ? withChoice.filter((x) => x.best).length / withChoice.length : null };
  document.querySelectorAll("#flicks td.oc").forEach((td) => (td.textContent = orderText(+td.dataset.n)));
  renderTiles(flickTiles);
  renderIssues();
}

// "What to look at": the server's checks, plus Pathing from the order analysis once the tracks are in.
function renderIssues() {
  const list = report.issues.slice();
  const p = pathing();
  if (p) list.push(p);
  list.sort((a, b) => (a.flag === "attention" ? 0 : 1) - (b.flag === "attention" ? 0 : 1));
  $("#issues").innerHTML = list.map((i) => `<div class="issue">
    <div class="top"><span>${i.issue ? `#${i.issue} ` : ""}${esc(i.title)}</span>
      <span class="flag ${i.flag}">${i.flag === "attention" ? "▲ Look at this" : "✓ Fine"}</span></div>
    <div class="val">${esc(i.value)}</div><div class="why">${i.html || esc(i.why)}</div></div>`).join("");
  $("#issues").querySelectorAll("a[data-n]").forEach((a) => a.addEventListener("click", (e) => {
    e.preventDefault();
    playFlick(+a.dataset.n);
  }));
}

// Time lost to picks, as shots: at your pace over the run (shots from the first flick to the last kill), the time
// saved by the best picks would have gone into this many more shots. An estimate: it assumes the pace holds.
function extraShots(seconds) {
  const s = report.summary, f0 = Math.min(...report.flicks.map((m) => m.start_frame));
  const span = (lastKill - f0) / report.fps;
  const count = s.shots ?? s.kills;                    // without a stats file there are no shot counts: kills instead
  return span > 0 && count ? seconds * count / span : 0;
}

function pathing() {
  if (!orders) return null;
  const picks = [...orders.byKill.entries()].filter(([, o]) => o.choices > 1);
  if (!picks.length) return null;
  const lost = picks.reduce((t, [, o]) => t + o.cost, 0), per = lost / picks.length;
  const med = report.summary.median_interval, share = med ? per / med : 0;
  const worst = picks.filter(([, o]) => !o.best).sort((a, b) => b[1].cost - a[1].cost).slice(0, 3);
  return {
    title: "Pathing",
    flag: share >= 0.05 ? "attention" : "fine",
    value: `The fastest next target in ${pct(orders.share)} of ${picks.length} picks; the others cost about ` +
      `${ms(lost)} in all, ${ms(per)} a kill (${pct(share)} of the median kill). With the best picks, about ` +
      `${extraShots(lost).toFixed(1)} more ${report.summary.shots == null ? "kills" : "shots"} at your pace`,
    html: (worst.length ? `Costliest picks: ${worst.map(([n, o]) => `<a href="#" data-n="${n}">kill ${n}</a> +${ms(o.cost)}`)
      .join(", ")}. ` : "") + "Predicted from Fitts' law fitted to this run, for the targets on screen at each pick; " +
      `targets that appeared less than ${NEW_MS} ms before you started moving are left out, since reacting to them ` +
      "costs time of its own. 5% of the median kill or more is flagged.",
  };
}

function orderText(n) {
  if (!orders) return "…";
  const o = orders.byKill.get(n);
  if (!o) return "–";
  if (o.spawned) return "new target";
  return o.choices === 1 ? "only one" : o.best ? "fastest" : `+${Math.round(1000 * o.cost)} ms`;
}

function drawOrder(c, scale) {
  const frame = Math.round(shownTime * report.fps);
  const f = tracks?.frames[frame];
  if (!f || !fitts || frame > lastKill || frame < firstStart) return;   // only during the run: not on the countdown
                                                                       // before it, nor after the last kill
  // new spawns are left out, as in the path cost: the cut of the flick under way (or NEW_MS before this frame)
  let m = null;
  for (const q of report.flicks) if (q.start_frame <= frame && (!m || q.start_frame > m.start_frame)) m = q;
  const cut = m && m.kill_frame >= frame ? newCut(m) : frame - NEW_MS / 1000 * report.fps;
  const fresh = orders ? f.t.filter((t) => orders.firstSeen.get(t[0]) <= cut) : f.t;
  const ts = fresh.length ? fresh : f.t;                            // only new targets (one at a time): no choice to skip
  const skipped = f.t.length - ts.length;
  const best = fastestOrder(ts);
  if (!best) return;
  const g = report.geometry, css = getComputedStyle(document.documentElement);
  const green = css.getPropertyValue("--series-3").trim(), orange = css.getPropertyValue("--series-2").trim();
  // yours: the targets on screen now that you killed from here on, in the order you killed them
  const yours = orders ? ts.map((t) => [t, orders.killOf.get(t[0])]).filter(([, k]) => k && k.kill_frame >= frame)
    .sort((x, y) => x[1].kill_frame - y[1].kill_frame).map(([t]) => t) : [];
  const line = (order, col, dash, dx, dy) => {
    const px = order.map((p) => toPx(p[1], p[2], g, scale));
    c.strokeStyle = col;
    c.lineWidth = 1.8;                                  // round dots, small enough to see past
    c.lineCap = "round";
    c.setLineDash(dash);
    c.globalAlpha = 0.7;
    c.beginPath();
    c.moveTo(...toPx(0, 0, g, scale));
    px.forEach((p) => c.lineTo(...p));
    c.stroke();
    c.lineCap = "butt";
    c.setLineDash([]);
    c.globalAlpha = 0.8;
    c.font = "bold 8px Segoe UI";
    c.textAlign = "center";
    c.textBaseline = "middle";
    px.forEach(([x, y], i) => {
      c.fillStyle = col;
      c.beginPath(); c.arc(x + dx, y + dy, 5, 0, 2 * Math.PI); c.fill();
      c.fillStyle = "#fff";
      c.fillText(String(i + 1), x + dx, y + dy + 0.5);
    });
    c.globalAlpha = 1;
    c.textAlign = "left";
    c.textBaseline = "alphabetic";
  };
  const showMine = mineBox.checked && yours.length, showBest = orderBox.checked;
  if (showMine) line(yours, orange, [0.1, 3], -8, 8);
  if (showBest) line(best.order, green, [0.1, 3], 8, -8);
  const n = best.order.length, rows = [];
  if (showBest) {
    rows.push([green, `Fastest path through the ${n} target${n > 1 ? "s" : ""} on screen` +
      `${skipped ? ` (${skipped} new left out)` : ""}: about ${ms(best.seconds)}`]);
  }
  if (showMine) {
    const mine = seconds(yours.length, pathUnits(yours));
    const ref = yours.length === n ? best.seconds : fastestOrder(yours).seconds;
    const diff = Math.round(1000 * (mine - ref));
    const what = showBest ? "them" : `the ${n} target${n > 1 ? "s" : ""} on screen`;
    rows.push([orange, (yours.length === n ? `Your path through ${what}` : `Your path through the ${yours.length} of ${what} you killed`) +
      `: about ${ms(mine)}, ${diff <= 0 ? "the fastest" : `${diff} ms slower than the fastest`}`]);
  }
  if (!rows.length) return;
  c.font = "11px Segoe UI";
  const wBox = Math.max(...rows.map(([, t]) => c.measureText(t).width)) + 24;
  c.fillStyle = "rgba(0,0,0,0.5)";
  c.fillRect(6, 6, wBox, 6 + 15 * rows.length);
  rows.forEach(([col, t], i) => {
    c.fillStyle = col;
    c.fillRect(11, 13 + 15 * i, 7, 7);
    c.fillStyle = "#fff";
    c.fillText(t, 23, 20 + 15 * i);
  });
}

// ---- speed chart ----------------------------------------------------------------------------------------------------
let chart = null;
function speeds(path, fps) {
  // positions smoothed over 3 frames (the capture moves in uneven steps), speed between neighbours
  const pts = path.map((q, i) => {
    if (i === 0 || i === path.length - 1) return q;
    return [q[0], (path[i - 1][1] + q[1] + path[i + 1][1]) / 3, (path[i - 1][2] + q[2] + path[i + 1][2]) / 3];
  });
  const raw = pts.map((q, i) => [q[0], i ? Math.hypot(q[1] - pts[i - 1][1], q[2] - pts[i - 1][2]) * fps / Math.max(1, q[0] - pts[i - 1][0]) : 0]);
  return smoothBox.checked ? smoothed(raw, 0.025 * fps) : raw;
}

// "Smooth" (the default): a Gaussian over the speeds, sigma 25 ms, weighted by frame distance. The first sample is a
// placeholder 0, so it is left out of the sums.
const smoothBox = $("#smooth");
smoothBox.checked = localStorage.getItem("vod-smooth") !== "0";
smoothBox.addEventListener("change", () => {
  localStorage.setItem("vod-smooth", smoothBox.checked ? "1" : "0");
  if (flick) drawSpeed();
});
function smoothed(data, sigma) {
  return data.map(([f]) => {
    let s = 0, w = 0;
    for (let i = 1; i < data.length; i++) {
      const d = data[i][0] - f;
      if (Math.abs(d) > 3 * sigma) continue;
      const k = Math.exp(-(d * d) / (2 * sigma * sigma));
      s += k * data[i][1];
      w += k;
    }
    return [f, w ? s / w : 0];
  });
}

function drawSpeed() {
  const svg = $("#speed");
  $("#speed-wrap").classList.remove("idle");
  const W = svg.clientWidth, H = 150, m = { l: 40, r: 10, t: 8, b: 22 };
  const fps = report.fps, f0 = flick.start_frame, f1 = flick.kill_frame;
  const data = speeds(report.paths[String(flick.n)], fps);
  const vmax = Math.max(60, ...data.map((d) => d[1])) * 1.08;
  const x = (f) => m.l + (W - m.l - m.r) * (f - f0) / Math.max(1, f1 - f0);
  const y = (v) => H - m.b - (H - m.t - m.b) * v / vmax;
  const step = vmax > 300 ? 100 : vmax > 120 ? 50 : 20;
  let grid = "";
  for (let v = 0; v <= vmax; v += step) {
    grid += `<line x1="${m.l}" x2="${W - m.r}" y1="${y(v)}" y2="${y(v)}" stroke="var(--grid)"/>
      <text x="${m.l - 6}" y="${y(v) + 4}" text-anchor="end" fill="var(--text-muted)" font-size="11">${v}</text>`;
  }
  const tms = Math.round(1000 * (f1 - f0) / fps), tstep = tms > 600 ? 200 : 100;
  for (let t = 0; t <= tms; t += tstep) {
    grid += `<text x="${x(f0 + t * fps / 1000)}" y="${H - 6}" text-anchor="middle" fill="var(--text-muted)" font-size="11">${t} ms</text>`;
  }
  const marks = [["moving", flick.react], ["flick done", flick.react != null && flick.flick != null ? flick.react + flick.flick : null],
    ["on target", flick.arrive], ["click", flick.total]].filter((q) => q[1] != null).map(([name, t]) => {
    const xx = x(f0 + t * fps);
    return `<line x1="${xx}" x2="${xx}" y1="${m.t}" y2="${H - m.b}" stroke="var(--axis)" stroke-dasharray="3 3"/>
      <text x="${xx + 3}" y="${m.t + 10}" fill="var(--text-secondary)" font-size="11">${name}</text>`;
  }).join("");
  const line = data.map((d, i) => `${i ? "L" : "M"}${x(d[0]).toFixed(1)},${y(d[1]).toFixed(1)}`).join("");
  svg.setAttribute("viewBox", `0 0 ${W} ${H}`);
  svg.innerHTML = `${grid}<line x1="${m.l}" x2="${W - m.r}" y1="${y(0)}" y2="${y(0)}" stroke="var(--axis)"/>${marks}
    <path d="${line}" fill="none" stroke="var(--series-1)" stroke-width="2" stroke-linejoin="round"/>
    <line id="ph" x1="0" x2="0" y1="${m.t}" y2="${H - m.b}" stroke="var(--text-primary)" stroke-width="1" opacity="0.7"/>
    <circle id="hov" r="4" fill="var(--series-1)" stroke="var(--surface-0)" stroke-width="2" visibility="hidden"/>`;
  chart = { x, y, data, f0, f1, fps, m, W };
  playhead();
}

function playhead() {
  if (!chart || !flick) return;
  const ph = $("#ph");
  if (!ph) return;
  const f = Math.min(chart.f1, Math.max(chart.f0, shownTime * chart.fps));
  ph.setAttribute("x1", chart.x(f));
  ph.setAttribute("x2", chart.x(f));
}

$("#speed").addEventListener("mousemove", (e) => {
  if (!chart) return;
  const r = e.currentTarget.getBoundingClientRect(), mx = (e.clientX - r.left) * chart.W / r.width;
  const d = chart.data.reduce((a, b) => (Math.abs(chart.x(b[0]) - mx) < Math.abs(chart.x(a[0]) - mx) ? b : a));
  const hov = $("#hov");
  hov.setAttribute("cx", chart.x(d[0]));
  hov.setAttribute("cy", chart.y(d[1]));
  hov.setAttribute("visibility", "visible");
  const tip = $("#speed-tip");
  tip.hidden = false;
  tip.textContent = `${Math.round(1000 * (d[0] - chart.f0) / chart.fps)} ms · ${Math.round(d[1])} °/s${smoothBox.checked ? " (smoothed)" : ""}`;
  tip.style.left = `${Math.min(r.width - 120, chart.x(d[0]) * r.width / chart.W + 10)}px`;
  tip.style.top = "18px";
});
$("#speed").addEventListener("mouseleave", () => {
  $("#speed-tip").hidden = true;
  $("#hov")?.setAttribute("visibility", "hidden");
});
$("#speed").addEventListener("click", (e) => {
  if (!chart) return;
  const r = e.currentTarget.getBoundingClientRect(), mx = (e.clientX - r.left) * chart.W / r.width;
  const f = chart.f0 + (mx - chart.m.l) / (chart.W - chart.m.l - chart.m.r) * (chart.f1 - chart.f0);
  video.pause();
  video.currentTime = Math.max(0, f / chart.fps);
});
window.addEventListener("resize", () => { if (flick) drawSpeed(); drawOverlay(); });

// ---- exclude areas: parts of the screen the review ignores (another player's webcam or overlay), as shares of the
// frame. Saved per recording; an upload starts from the areas last saved for an upload, else KovOBS's layout -----------
let excluding = null, exDrag = null;
const exBar = $("#exclude-bar");
const exAt = (e) => { const r = overlay.getBoundingClientRect(); return [(e.clientX - r.left) / r.width, (e.clientY - r.top) / r.height]; };
const exFrom = { saved: "Saved for this recording.", "last upload": "From the last upload you saved areas for.",
  kovobs: "KovOBS's layout (the default)." };

let exKinds = [];
$("#exclude").addEventListener("click", () => (excluding ? stopExcluding() : startExcluding()));

// types have a fixed id; their name and description can change at any time
function fillKinds() {
  $("#ex-kind").innerHTML = exKinds.map((k) => `<option value="${esc(k.id)}" title="${esc(k.about)}">${esc(k.name)}</option>`)
    .join("") + '<option value="__add">+ Add a type…</option>';
}
const kindName = (id) => (exKinds.find((k) => k.id === id) || {}).name || id;
let exTypeEdit = null;                                       // the type being renamed, or null for a new one
function openTypeForm(kind) {
  exTypeEdit = kind ? kind.id : null;
  $("#ex-type-name").value = kind ? kind.name : "";
  $("#ex-type-about").value = kind ? kind.about : "";
  $("#ex-addtype").hidden = false;
  $("#ex-type-name").focus();
}

async function startExcluding() {
  const r = await api(`/api/exclude?id=${encodeURIComponent(current)}`);
  video.pause();
  stopAt = null;
  excluding = { boxes: r.boxes.map((b) => [...b.slice(0, 4), b[4] || "other"]), sel: -1 };
  exKinds = r.kinds;
  fillKinds();
  const cur = vods.find((x) => x.id === current);
  $("#ex-notaim").textContent = cur && cur.not_aim ? "It is an aim trainer" : "Not an aim trainer";
  exSelect(-1);
  $("#exclude-from").textContent = exFrom[r.source] || "";
  exBar.hidden = false;
  overlay.classList.add("editing");
  $("#exclude").classList.add("on");
  drawOverlay();
}

function stopExcluding() {
  excluding = exDrag = exEdit = null;
  overlay.style.cursor = "";
  exBar.hidden = true;
  $("#ex-addtype").hidden = true;
  overlay.classList.remove("editing");
  $("#exclude").classList.remove("on");
  drawOverlay();
}

function drawExcluded(c, w, h) {
  const all = exDrag ? [...excluding.boxes, [...exDrag, ""]] : excluding.boxes;
  all.forEach(([x0, y0, x1, y1, kind], i) => {
    const x = Math.min(x0, x1) * w, y = Math.min(y0, y1) * h, bw = Math.abs(x1 - x0) * w, bh = Math.abs(y1 - y0) * h;
    const sel = i === excluding.sel;
    c.fillStyle = sel ? "rgba(217, 89, 38, 0.42)" : "rgba(217, 89, 38, 0.28)";
    c.fillRect(x, y, bw, bh);
    c.strokeStyle = sel ? "#fff" : "rgba(217, 89, 38, 0.9)";
    c.lineWidth = sel ? 2 : 1.5;
    c.strokeRect(x + 0.5, y + 0.5, bw - 1, bh - 1);
    if (kind && !(exEdit && exEdit.moved && exEdit.i === i)) {   // its type's name: plain to read on the other
      c.font = "600 11px Segoe UI";                           // areas, see-through on the selected one (its corner
      c.textBaseline = "top";                                 // must stay visible), hidden while it is dragged
      c.save();
      c.beginPath(); c.rect(x + 2, y + 2, bw - 4, 16); c.clip();
      if (!sel) {
        c.fillStyle = "rgba(14, 15, 17, 0.75)";
        c.fillRect(x + 2, y + 2, c.measureText(kindName(kind)).width + 8, 16);
      }
      c.fillStyle = sel ? "rgba(255, 255, 255, 0.55)" : "#fff";
      c.fillText(kindName(kind), x + 6, y + 4);
      c.restore();
      c.textBaseline = "alphabetic";
    }
  });
}

function exSelect(i) {
  excluding.sel = i;
  const on = i >= 0;
  $("#ex-kind").disabled = $("#ex-remove").disabled = $("#ex-edit-type").disabled = !on;
  if (on) $("#ex-kind").value = excluding.boxes[i][4];
  drawOverlay();
}
$("#ex-kind").addEventListener("change", () => {
  if (!excluding || excluding.sel < 0) return;
  if ($("#ex-kind").value === "__add") {                // a type of your own: its name and what it is
    $("#ex-kind").value = excluding.boxes[excluding.sel][4];
    return openTypeForm(null);
  }
  excluding.boxes[excluding.sel][4] = $("#ex-kind").value;
  drawOverlay();
});
$("#ex-edit-type").addEventListener("click", () => {
  if (excluding && excluding.sel >= 0) openTypeForm(exKinds.find((k) => k.id === excluding.boxes[excluding.sel][4]));
});
$("#ex-type-cancel").addEventListener("click", () => { $("#ex-addtype").hidden = true; });
$("#ex-type-add").addEventListener("click", async () => {
  const name = $("#ex-type-name").value.trim(), about = $("#ex-type-about").value.trim(), id = exTypeEdit;
  try {
    exKinds = await api("/api/area_kinds", { method: "POST", body: JSON.stringify({ id, name, about }) });
  } catch (e) {
    return ($("#exclude-from").textContent = `Could not save the type: ${e.message}`);
  }
  fillKinds();
  $("#ex-addtype").hidden = true;
  if (excluding && excluding.sel >= 0) {
    if (!id) excluding.boxes[excluding.sel][4] = (exKinds.find((k) => k.name === name) || {}).id || "other";
    exSelect(excluding.sel);                              // a renamed type shows its new name everywhere
  }
});
function exRemove() {
  if (!excluding || excluding.sel < 0) return;
  excluding.boxes.splice(excluding.sel, 1);
  exSelect(-1);
}
$("#ex-remove").addEventListener("click", exRemove);

// Drag an area's inside to move it, its edge or corner to resize it, the empty video to draw a new one; a click
// selects. The selected area is grabbed first, then the topmost one under the pointer.
let exEdit = null;
function exHit(e) {
  const r = overlay.getBoundingClientRect(), [x, y] = exAt(e), px = 6 / r.width, py = 6 / r.height;
  const order = [...excluding.boxes.keys()].reverse();
  if (excluding.sel >= 0) order.unshift(excluding.sel);
  for (const i of order) {
    const [a, b, c, d] = excluding.boxes[i];
    if (x < a - px || x > c + px || y < b - py || y > d + py) continue;
    const edges = { l: Math.abs(x - a) <= px, r: Math.abs(x - c) <= px, t: Math.abs(y - b) <= py, b: Math.abs(y - d) <= py };
    return { i, edges: edges.l || edges.r || edges.t || edges.b ? edges : null };
  }
  return null;
}
const exCursor = (h) => {
  if (!h) return "crosshair";
  if (!h.edges) return "move";
  const { l, r, t, b } = h.edges;
  return (l && t) || (r && b) ? "nwse-resize" : (r && t) || (l && b) ? "nesw-resize" : l || r ? "ew-resize" : "ns-resize";
};

overlay.addEventListener("mousedown", (e) => {
  if (!excluding) return;
  const [x, y] = exAt(e), hit = exHit(e);
  if (hit) {
    const wasSel = excluding.sel === hit.i;
    exSelect(hit.i);
    exEdit = { i: hit.i, edges: hit.edges, start: [x, y], orig: excluding.boxes[hit.i].slice(0, 4), wasSel, moved: false };
  } else {
    exDrag = [x, y, x, y];
  }
});
overlay.addEventListener("mousemove", (e) => {
  if (!excluding) return;
  const [x, y] = exAt(e);
  if (exEdit) {
    const r = overlay.getBoundingClientRect(), mw = 8 / r.width, mh = 8 / r.height;
    const [a, b, c, d] = exEdit.orig, dx = x - exEdit.start[0], dy = y - exEdit.start[1], box = excluding.boxes[exEdit.i];
    if (!exEdit.moved && Math.abs(dx) * r.width < 3 && Math.abs(dy) * r.height < 3) return;
    exEdit.moved = true;
    const cl = (v, lo, hi) => Math.min(hi, Math.max(lo, v));
    if (!exEdit.edges) {                                 // move, kept on screen
      const ox = cl(dx, -a, 1 - c), oy = cl(dy, -b, 1 - d);
      [box[0], box[1], box[2], box[3]] = [a + ox, b + oy, c + ox, d + oy];
    } else {                                             // resize by the grabbed edges, at least 8 px
      const { l, r: rr, t, b: bb } = exEdit.edges;
      if (l) box[0] = cl(a + dx, 0, c - mw);
      if (rr) box[2] = cl(c + dx, a + mw, 1);
      if (t) box[1] = cl(b + dy, 0, d - mh);
      if (bb) box[3] = cl(d + dy, b + mh, 1);
    }
    return drawOverlay();
  }
  if (exDrag) {
    [exDrag[2], exDrag[3]] = [x, y];
    return drawOverlay();
  }
  overlay.style.cursor = exCursor(exHit(e));
});
addEventListener("mouseup", () => {
  if (!excluding) return;
  if (exEdit) {
    // areas can overlap: a click on the selected area selects the next one under the pointer
    if (!exEdit.moved && exEdit.wasSel) {
      const [x, y] = exEdit.start, n = excluding.boxes.length;
      for (let k = 1; k < n; k++) {
        const j = (exEdit.i - k + n) % n, [a, b, c, d] = excluding.boxes[j];
        if (x >= a && x <= c && y >= b && y <= d) { exSelect(j); break; }
      }
    }
    exEdit = null;
    return;
  }
  if (!exDrag) return;
  const [x0, y0, x1, y1] = exDrag;
  exDrag = null;
  const r = overlay.getBoundingClientRect();
  if (Math.abs(x1 - x0) * r.width < 6 && Math.abs(y1 - y0) * r.height < 6) {      // a click on the empty video
    exSelect(-1);
  } else {                                                                          // a new area, selected to label it
    const cl = (v) => Math.min(1, Math.max(0, v));
    excluding.boxes.push([cl(Math.min(x0, x1)), cl(Math.min(y0, y1)), cl(Math.max(x0, x1)), cl(Math.max(y0, y1)), "other"]);
    exSelect(excluding.boxes.length - 1);
  }
});

$("#ex-cancel").addEventListener("click", () => { stopLabelling(); stopExcluding(); });
$("#ex-clear").addEventListener("click", () => { excluding.boxes = []; exSelect(-1); });

$("#ex-find").addEventListener("click", () => findAreas());
$("#ex-detect").addEventListener("click", () => findAreas(false));
async function findAreas(copy = true) {
  const id = current, btn = copy ? $("#ex-find") : $("#ex-detect"), label = btn.textContent;
  btn.disabled = true;
  btn.textContent = "Finding…";
  try {
    const r = await api(`/api/find_areas?id=${encodeURIComponent(id)}&copy=${copy ? 1 : 0}`);
    if (!excluding || id !== current) return;
    excluding.boxes = r.boxes.map((b) => b.slice());
    exSelect(-1);
    const taught = `Learned from ${r.recordings} recording${r.recordings === 1 ? "" : "s"} you saved.`;
    $("#exclude-from").textContent = r.copied
      ? `Same layout as ${r.copied.replace(/_/g, " ")}: your areas from there. ${taught} Check them, then Save.`
      : `Found ${r.boxes.length} area${r.boxes.length === 1 ? "" : "s"}: ${r.by.learned || 0} named from what you ` +
        `taught, ${r.by.rule || 0} by rules. ${taught} Check them, then Save.`;
  } catch (e) {
    $("#exclude-from").textContent = `Could not find areas: ${e.message}`;
  } finally {
    btn.disabled = false;
    btn.textContent = label;
  }
}

$("#ex-save").addEventListener("click", async () => {
  const id = current, v = vods.find((x) => x.id === id);
  try {
    await api(`/api/exclude?id=${encodeURIComponent(id)}`, { method: "POST", body: JSON.stringify(excluding.boxes) });
  } catch (e) {
    return showProgress(`Could not save the areas: ${e.message}`, 0);
  }
  stopExcluding();
  if (v?.analysed) {
    showProgress("Areas saved: press Review again to use them", 0);
    $("#analyse").classList.add("primary");
  }
});

// ---- labelling areas: recordings one by one (uploads first, then one per scenario), each opened in the editor with
// the areas found in it; Save and next teaches the area finder (areas.py) -------------------------------------------------
let labelQ = null, labelPos = 0;
$("#label-areas").addEventListener("click", async () => {
  labelQ = await api("/api/label_queue");
  labelPos = 0;
  if (!labelQ.length) { labelQ = null; return showProgress("Every recording has saved areas already", 1); }
  labelNext();
});

async function labelNext() {
  if (!labelQ) return;
  if (labelPos >= labelQ.length) {
    const n = labelQ.length;
    stopLabelling();
    stopExcluding();
    return showProgress(`Went through all ${n} recordings`, 1);
  }
  await select(labelQ[labelPos]);
  await startExcluding();
  for (const id of ["ex-next", "ex-skip", "label-pos"]) $(`#${id}`).hidden = false;
  $("#ex-save").hidden = true;
  $("#label-pos").textContent = `Recording ${labelPos + 1} of ${labelQ.length}`;
  video.currentTime = Math.min(video.duration || 30, 20);   // a frame from the run, not the countdown
  const next = labelQ[labelPos + 1];                          // the next recording's areas, found while this one
  if (next) api(`/api/find_areas?id=${encodeURIComponent(next)}`).catch(() => {});   // is checked (cached)
  await findAreas();
}

function stopLabelling() {
  labelQ = null;
  for (const id of ["ex-next", "ex-skip", "label-pos"]) $(`#${id}`).hidden = true;
  $("#ex-save").hidden = false;
}

$("#ex-skip").addEventListener("click", async () => {      // remembered: the queue does not offer it again
  await api(`/api/label_skip?id=${encodeURIComponent(current)}`, { method: "POST" }).catch(() => {});
  labelPos++;
  labelNext();
});
$("#ex-notaim").addEventListener("click", async () => {      // another game: out of the queue and of the learning
  const id = current, v = vods.find((x) => x.id === id);
  const on = !(v && v.not_aim);
  await api(`/api/not_aim?id=${encodeURIComponent(id)}&on=${on ? 1 : 0}`, { method: "POST" });
  if (v) v.not_aim = on;
  renderVods();
  if (labelQ && on) { labelPos++; return labelNext(); }
  stopExcluding();
  showProgress(on ? "Marked as another game: left out of labelling and learning" : "Marked as an aim trainer again", 1);
});
$("#ex-next").addEventListener("click", async () => {
  try {
    await api(`/api/exclude?id=${encodeURIComponent(current)}`, { method: "POST", body: JSON.stringify(excluding.boxes) });
  } catch (e) {
    return ($("#exclude-from").textContent = `Could not save the areas: ${e.message}`);
  }
  labelPos++;
  labelNext();
});

// ---- full screen: the video fills the window (and the screen, where the browser allows it), with the area editor's
// bar, the seek bar and the controls still there ---------------------------------------------------------------------
function fitFull() {
  if (!document.body.classList.contains("full")) return;
  const chrome = ["exclude-bar", "seek-row", "controls"].reduce((h, id) => h + ($(`#${id}`).offsetHeight || 0), 0) + 48;
  document.body.style.setProperty("--full-chrome", `${chrome}px`);
  requestAnimationFrame(drawOverlay);
}
function setFull(on) {
  document.body.classList.toggle("full", on);
  if (on) document.documentElement.requestFullscreen?.().catch(() => {});
  else if (document.fullscreenElement) document.exitFullscreen().catch(() => {});
  fitFull();
  requestAnimationFrame(drawOverlay);
}
for (const b of document.querySelectorAll(".full-toggle"))
  b.addEventListener("click", () => setFull(!document.body.classList.contains("full")));
document.addEventListener("fullscreenchange", () => { if (!document.fullscreenElement) setFull(false); });
addEventListener("resize", fitFull);
new ResizeObserver(fitFull).observe(exBar);

// ---- keyboard: space plays or pauses, the arrows step a frame, shift and the arrows go to the previous or next kill,
// Escape shows the whole run's cards again ------------------------------------------------------------------------
addEventListener("keydown", (e) => {
  if (e.target.closest("input, textarea, select, dialog")) return;
  if (e.key === "Escape" && document.body.classList.contains("full")) return setFull(false);
  if ((e.key === "f" || e.key === "F") && !e.ctrlKey && !e.metaKey && !e.altKey && current) {
    return setFull(!document.body.classList.contains("full"));
  }
  if (excluding && e.key === "Escape") return stopExcluding();
  if (excluding && (e.key === "Delete" || e.key === "Backspace") && !e.target.closest("input, select")) return exRemove();
  if (excluding || e.target.closest("input, textarea, select") || e.ctrlKey || e.metaKey || e.altKey || !report) return;
  const fps = report.fps;
  if (e.key === " ") {
    e.preventDefault();
    stopAt = null;
    video.paused ? video.play().catch(() => {}) : video.pause();
  } else if ((e.key === "ArrowLeft" || e.key === "ArrowRight") && e.shiftKey) {
    e.preventDefault();
    const list = report.flicks, i = flick ? list.indexOf(flick) : -1;
    const next = e.key === "ArrowRight" ? list[Math.min(list.length - 1, i + 1)] : list[Math.max(0, i - 1)];
    if (next) playFlick(next.n);
  } else if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
    e.preventDefault();
    video.pause();
    stopAt = null;
    video.currentTime = Math.max(0, video.currentTime + (e.key === "ArrowRight" ? 1 : -1) / fps);
  } else if (e.key === "Escape" && flickTiles) {
    renderTiles(null);
  }
});

// ---- upload: any recording, by the button or dropped anywhere on the page; a stats .csv dropped with it goes along --
const isVideo = (f) => /\.(mp4|mkv|mov|webm)$/i.test(f.name);
function send(url, file, label) {
  return new Promise((resolve, reject) => {
    const x = new XMLHttpRequest();
    x.open("POST", url);
    x.upload.onprogress = (e) => {
      if (!e.lengthComputable) return;
      $("#up-bar").style.width = `${(100 * e.loaded / e.total).toFixed(1)}%`;
      $("#up-text").textContent = `${label} ${(e.loaded / 1e6).toFixed(0)} of ${(e.total / 1e6).toFixed(0)} MB`;
    };
    x.onload = () => {
      const j = JSON.parse(x.responseText || "{}");
      x.status < 300 ? resolve(j) : reject(new Error(j.error || x.statusText));
    };
    x.onerror = () => reject(new Error("the upload failed"));
    x.send(file);
  });
}
async function upload(files) {
  const videos = [...files].filter(isVideo), csvs = [...files].filter((f) => /\.csv$/i.test(f.name));
  if (!videos.length) { alert("Pick a video (.mp4, .mkv, .mov or .webm), and its stats .csv if you have it."); return; }
  $("#upload-status").hidden = false;
  $("#upload").disabled = true;
  let last = null;
  try {
    for (const v of videos) {
      last = await send(`/api/upload?name=${encodeURIComponent(v.name)}`, v, `Uploading ${v.name}:`);
      if (videos.length === 1 && csvs.length) await send(`/api/upload?name=${encodeURIComponent(csvs[0].name)}&for=${encodeURIComponent(v.name)}`, csvs[0], "Stats file:");
    }
    $("#up-text").textContent = "Uploaded";
    await loadVods();
    if (last?.id) select(last.id);
  } catch (e) {
    $("#up-text").textContent = `Upload failed: ${e.message}`;
  } finally {
    $("#upload").disabled = false;
    setTimeout(() => { $("#upload-status").hidden = true; $("#up-bar").style.width = "0"; }, 2500);
  }
}
$("#upload").addEventListener("click", () => $("#file").click());
$("#file").addEventListener("change", (e) => { upload(e.target.files); e.target.value = ""; });
let dragDepth = 0;
addEventListener("dragenter", (e) => { if (e.dataTransfer?.types.includes("Files")) { dragDepth++; $("#drop").hidden = false; } });
addEventListener("dragleave", () => { if (--dragDepth <= 0) { dragDepth = 0; $("#drop").hidden = true; } });
addEventListener("dragover", (e) => e.preventDefault());
addEventListener("drop", (e) => {
  e.preventDefault();
  dragDepth = 0;
  $("#drop").hidden = true;
  if (e.dataTransfer?.files.length) upload(e.dataTransfer.files);
});

// ---- models: what each one does best, and the one new reviews use ---------------------------------------------------
let models = null;                                     // /api/models: {chosen, device, checks, models, ...}
const modelName = (n) => (n === "hand" ? "Hand-written" : n);

function setModels(m) {
  models = m;
  $("#model").textContent = `${modelName(m.chosen)} · ${m.device === "cuda" ? "GPU" : "CPU"}`;
}
const loadModels = async () => setModels(await api("/api/models"));

// One column per model, one row per measure; the best value in a row is marked (in words too, for screen readers).
function renderModels() {
  const main = models.models.filter((m) => !m.older);
  const cells = (vals, show, better) => {
    const have = vals.filter((v) => v != null), best = better && have.length > 1 ? better(have) : null;
    return vals.map((v, i) => (v == null ? '<td class="muted">–</td>'
      : v === best ? `<td class="best">${show(v, i)}<span class="sr"> (best)</span></td>` : `<td>${show(v, i)}</td>`)).join("");
  };
  const row = (label, title, vals, show, better) =>
    `<tr><th scope="row"${title ? ` title="${esc(title)}"` : ""}>${label}</th>${cells(vals, show, better)}</tr>`;
  const text = (label, key) =>
    `<tr><th scope="row">${label}</th>${main.map((m) => `<td class="text">${esc(m[key] || "–")}</td>`).join("")}</tr>`;
  const low = (v) => Math.min(...v), high = (v) => Math.max(...v);
  const speed = (k) => main.map((m) => m.speed_ms?.[k] ?? null);
  const checks = models.checks.map((c) => {
    const got = main.map((m) => m.checks?.[c.key] ?? null);
    if (c.key === "tracking") {
      return row(`${c.name}<span class="row-what">${esc(c.what)}</span>`, "", got.map((g) => g && g[1]),
        (v, i) => `${v.toFixed(3)} <span class="muted">(mean ${got[i][0].toFixed(3)})</span>`, low);
    }
    return row(`${c.name} <span class="muted">(${num(c.of)} kills)</span>`, c.what, got.map((g) => g && g[1]),
      (v, i) => `${num(v)} flicks <span class="muted">· ${num(got[i][0])} matched</span>`, high);
  });
  $("#models-table").innerHTML = `
    <thead><tr><td></td>${main.map((m) => `<th scope="col">${esc(m.label)}${m.default ? ' <span class="badge">default</span>' : ""}${
      m.name === models.chosen ? ' <span class="badge done">in use</span>' : ""}</th>`).join("")}</tr></thead>
    <tbody>
      ${text("Trained on", "trained")}
      ${text("Best at", "best")}
      ${text("Weak at", "weak")}
      ${checks.join("")}
      ${row("GPU, per frame", "PyTorch, batches of 16", speed("gpu"), (v) => `${v} ms`, low)}
      ${row("CPU, per frame", "ONNX Runtime, fp32, 4 threads", speed("cpu"), (v) => `${v} ms`, low)}
      ${row("Browser, per frame", "onnxruntime-web, WASM", speed("browser"), (v) => `${v} ms`, low)}
      ${row("Parameters", "", main.map((m) => m.params ?? null), num)}
      ${row("File size", "The fp32 ONNX file", main.map((m) => m.kb ?? null), (v) => `${v} KB`)}
      <tr><th scope="row"></th>${main.map((m) => `<td>${useButton(m)}</td>`).join("")}</tr>
    </tbody>`;
  $("#models-notes").textContent = `${models.checked_on} ${models.speed}`;
  $("#models-older-list").innerHTML = models.models.filter((m) => m.older).map((m) => `
    <div class="older-row"><span>${esc(m.name)}</span><span class="muted">${m.kb ? `${m.kb} KB` : "PyTorch file only"}</span>
      ${useButton(m)}</div>`).join("");
}

function useButton(m) {
  if (m.name === models.chosen) return '<span class="muted">In use</span>';
  return m.available ? `<button data-use="${esc(m.name)}">Use ${esc(m.name === "hand" ? "hand-written" : m.name)}</button>`
    : '<span class="muted">Needs the GPU</span>';
}

$("#model").addEventListener("click", async () => {
  try {
    await loadModels();
  } catch (e) {
    showProgress(`Could not load the models: ${e.message}`, 0);
    return;
  }
  renderModels();
  $("#models-status").textContent = "";
  $("#models").showModal();
});
$("#models-close").addEventListener("click", () => $("#models").close());
$("#models").addEventListener("click", async (e) => {
  const b = e.target.closest("button[data-use]");
  if (!b) return;
  const name = b.dataset.use;
  b.disabled = true;
  $("#models-status").textContent = `Loading ${modelName(name)}…`;
  try {
    setModels(await api(`/api/model?name=${encodeURIComponent(name)}`, { method: "POST" }));
  } catch (err) {
    $("#models-status").textContent = `Could not switch: ${err.message}`;
    b.disabled = false;
    return;
  }
  renderModels();
  $("#models-close").focus();
  $("#models-status").textContent = `Now using ${modelName(name)}. New reviews use it.`;
  if (current && report) {                             // this recording may have a review by that model already
    tracks = orders = orderCache = faint = null;
    showReport();
  }
});

loadModels().catch(() => {});
loadVods();
