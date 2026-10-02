# VOD review

These scripts measure a KovaaK's static clicking run from its KovOBS recording. They track every target frame by
frame, match the kills with the run's stats file, and measure each flick. The last script turns the measures into a
short manim video. They were first used on 2026-09-30, on the user's 143 run in 1w4ts Voltaic.

## Steps

Work in a folder under `test_out/`, which git ignores:

```bash
python python/track_vod.py "<video>.mp4" test_out/vod
python python/flicks.py test_out/vod "<stats csv>"
python python/measure.py test_out/vod
VOD_DIR=test_out/vod test_out/manim_venv/Scripts/manim -qh --media_dir test_out/vod/media python/flick_video.py FlickVideo
```

1. `track_vod.py` writes `tracks.json`: each target's position from the crosshair, in view degrees, per frame.
2. `flicks.py` writes `flicks.json`: one entry per kill, holding the killed target's path from the previous kill.
3. `measure.py` writes `measures.json` and prints the summary. It measures the reaction, the main flick, where the flick
   ended, corrections, time on the target, speed and offset at the click. It also splits the times by distance and
   direction.
4. `flick_video.py` renders the video. The kills it shows are picked for the 143 run; pick new ones for another run.

## What the scripts assume

- **Recording:** KovOBS at 16:9 (the 143 run was 2560 x 1440 at 120 fps), 103 FOV on the Overwatch scale, and the
  crosshair at the center. The tracker scales the video to 1280 x 720 and masks the overlay boxes: session stats, timer,
  clock and FPS, settings, gun and title, crosshair zoom, and hand cam. If the overlay moves, change `MASK`.
- **Targets:** dark and round on a light wall (the user's theme Bassel 3). `measure.py` takes the target radius as 0.43
  deg, measured for 1w4ts Voltaic.
- **Uneven frames:** the capture moves in alternating long and short steps, so the game's frames reach OBS unevenly.
  Speeds are therefore taken over 2 or 3 frames, never over 1.

## Review app

`review.py` holds the whole pipeline: track, match, measure, summarize, judge against the issue rules, and write a
report. `server.py` serves it as a local web app at `http://127.0.0.1:8770/` (the `vod-review` entry in
`.claude/launch.json`): pick a VOD from the KovOBS library, press Analyse, and read the report, with each flick played
back at quarter speed.

Any other recording can be added with "Upload VOD" or by dropping it on the page (mp4, mkv, mov or webm), with its
stats .csv if there is one. Uploads are kept in `test_out/vod_uploads/` and listed with the library. A pill next to
the run's title says where its kills came from: the stats file, the session HUD, or the video only. Keys: Space
plays or pauses, Left and Right step one frame, Shift with Left or Right goes to the previous or next kill, and
Escape shows the whole run's cards again.

Beside "Review again", three buttons work on the open recording: "Label areas", "Set cut-off" and "Set run" (below).
The top bar's "Label areas queue" and "Cut-off queue" do the first two across recordings, one after another.

"Label areas" marks parts of the screen the review ignores: drag on the video to cover a webcam, an overlay or a
stream's text, drag an area to move it or its edge or corner to resize it, click it to set its type or remove it, and
Save. "Full screen" (or F; Escape leaves) makes the video fill the window, with the editor's bar, the seek bar and the
controls kept, for placing areas precisely. The areas are kept per recording, as shares of the frame, in the
recording's cache folder (`exclude.json`); targets found inside them are dropped while tracking, so press "Review
again" after a change. A recording starts from KovOBS's layout (the boxes in `review.OVERLAY`); an upload starts from
the areas last saved for an upload (`test_out/vod_app/exclude_uploads.json`), since an uploader's setup tends to stay
the same. The HUD readers look in their own places and are not affected.

An area that only sometimes shows (KovaaK's "Last kill" panel, a few seconds after a kill) is excluded only while it
shows (`review.AreaWatch`). The review looks at every other frame of each area: what stands out from its
neighbours, as text and boxes do and a plain wall does not. An area is taken for a pop-up when it is off for 30% of
the run or more, comes and goes 3 times or more (the results screen covering it once at the end is not), and looks
the same whenever it is on; it is then excluded in its on frames and 4 frames either side, and every other area all
the time. On the user's 12 labelled recordings with a Last kill area (about 120 areas), only Last kill was taken for a
pop-up, on the 3 tracking runs where kills are far apart (on 31% to 37% of the run); in clicking runs it is up nearly
all the time and stays excluded. On Smooth Tracking, 209 detections in that area came back in its off frames.
Finding such areas by themselves was tried (tiles that switch on and off with the same pattern, at 6 frames a
second) and dropped: on 37 labelled recordings it proposed 31 to 104 areas, 3 to 5 of them where the user drew one,
and found 3 of the 12 Last kill panels; copying the user's areas from the same layout brings them instead.

After 154 labelled recordings (2026-10-02, 1,765 examples), leaving each recording out in turn: the learner named
89% of the found areas, 97% of those right, and 152 of the 154 recordings had another one with the same layout to
copy from.

Each area has a type (Session stats, Timer, Clock, Scenario name, Magazine, Weapon, Settings, Webcam, Version, Other,
or one the user adds with "+ Add a type…", with a line on what it is; `test_out/vod_app/area_kinds.json`). "Find areas"
proposes a recording's areas (`areas.py`): what stays put on screen while the view moves, grouped, and named first by
rules (KovaaK's session box where the HUD reader finds it, Aim Lab's boxes, a big area whose content does not stay put
is a webcam, the top center the timer, a top corner the clock, the bottom center the scenario name, and so on), then by
what was learned: every saved recording's areas become examples (`area_examples.jsonl`; a found area the user removed
is an example of "not an area"), and a found area takes the type most of its 5 nearest examples have, when they agree
and are near. "Label areas queue" (top bar) goes through recordings one by one (uploads first, then the latest of each scenario), with
the found areas ready to fix: "Save and next" teaches the finder. Most recordings share a layout, so when a
recording's found areas sit where those of one the user labelled do (half or more overlap, both ways), the editor
starts from the user's own areas for that one, as drawn and named: on the user's first 12 labelled recordings, 9 had
such a match, covering 74% to 94% of what they drew. "Detect fresh" never copies: it finds the recording's areas itself and
names them from what was learned. A box whose picture is a magnified copy of the screen round the
crosshair is named Zoomed crosshair. Each type has a fixed id (`area_kinds.json`); "Edit type" changes its name and
description at any time, and saved areas and examples keep the id. Skip is remembered (`label_skipped.json`), so the
queue resumes after the recordings already skipped; "Not an aim trainer" marks a recording of another game
(`not_aim_trainer.json`): it leaves the queue and the learning, and the list shows it as such. Areas can overlap: clicking the selected area again selects the one underneath,
and learning matches each found area with the saved area that fits it best. The finder gives up on a recording whose view hardly
moves (a probe), where the room itself stays put. It started from 30 of the user's recordings with KovOBS's layout
(`python python/areas.py bootstrap`): leaving each recording out in turn, the learner named half the areas, all right,
and the rules named the rest.

When the scenario is installed, the review reads its target count from the `.sce` file and keeps at most that many
targets a frame, by score. Targets within 2 degrees of the crosshair always stay, since a target under the crosshair
scores low but is the one being shot. This removes most false targets on busy walls.

Kills are not shots. In the video-only review, a target that takes two hits counts once.

"Show the fastest path" draws, on any frame, the path through every target on screen that would take the least
time from the crosshair. A flick's time is taken from Fitts' law, t = a + b × log2(1 + D / W), fitted to the run's own
kills (D the flick's distance, W the target's width). The per-kill a is the same for every path, so the fastest path
has the smallest sum of log2 terms. It is solved exactly for up to 14 targets. It only knows the targets on screen:
in a scenario that respawns, new targets change the best path. It shows only during the run: not on the countdown
before it (the run starts at the stats file's challenge start; without one, two median kill intervals before the
first kill), nor after the last kill. Where the detector marks the crosshair itself (`review.crosshair_spots`, in the
report as `crosshair`), detections on that spot are not targets to clear: on a valorant run the path counted the
crosshair as a third target where the scenario has two.

"Show my path", a separate option, draws the path you took through those targets (orange), with its predicted time
against the fastest. For every kill, the app also works out what the pick cost: at the moment the next target was
picked (3 frames after the flick starts), how much slower clearing the targets on screen gets when that target goes
first, with the best path after it. Only targets on screen at least 150 ms before you started moving count as
options: a target that spawned just before or during the flick needs a reaction of its own, which the model does not
price (the user's rule, 2026-10-01). A kill that went to such a new target gets no cost ("new target"). The flick
list's "Path cost" column, the kill cards and the "Pathing" card in "What to look at" show it. The Pathing card flags
picks that cost 5% of the median kill or more on average (provisional, like the other checks). It also turns the time
lost into shots: the time times your pace over the run (shots from the first flick to the last kill). That assumes the
pace would have held. The path overlay leaves out new spawns by the same 150 ms rule.

"Set cut-off" (beside "Review again") opens this recording's cut-off panel under the video; the panel also shows
while the cut-off is on. It works on tracking runs too (since 2026-10-02): there, a track's score counts every frame,
since the bot sits under the crosshair most of the time (Aethercontrol Easy: only 82 of the bot's 3,618 frames lie 2°
or more away), and the cut leaves its tracks out of the highlight and the timeline at once and out of every measure,
which are measured again in about 2 s (`review.without_faint`). On three tracking runs the cut at the start offset
removed 13 to 46 junk tracks (HUD marks, seams) and left the measures almost the same, since the review already takes
the bot nearest the crosshair; it matters when junk lies near the crosshair. "Leave out faint targets" (off by default, the user's choice, 2026-10-02) drops the tracks the model is far less sure
of than of the recording's targets, such as wall seams on a tiled theme. Each track's score is the 90th percentile of
the model's scores for it away from the crosshair (a target under the crosshair scores low). The recording's level is
the 90th percentile of those, weighted by frames. A track scoring more than the slider's offset below the level is
left out of the paths and the path costs, and is drawn on the video as a dimmed dashed ring, so you can see what the
cut removes. The slider runs from 0.20 to 0.60 (0.30 to start), and the text beside it gives the cut, the level and
how many tracks fall below the cut. The setting changes the overlay at once and is kept per recording
(`faint.json` in its cache folder). Tracks need the model's scores, so a recording reviewed before 2026-10-02 needs
"Review again".

To help pick the cut:
- **The strip** under the slider has a dot for every track at its score, sized by how long it was seen; left-out
  tracks are grey, kept ones green, and the cut is the orange line. Clicking a dot jumps the video to the middle of
  that track and rings it in orange.
- **Show scores** writes each track's score beside it on the video. Pointing at a track shows its score in that
  frame and its track score.

No offset is safe for every recording. On 1wall 6targets extra small 889.26 (full_v3), the seams score 0.30 to 0.37
and the targets 0.83 and up; at the start offset (cut 0.62) the seams and the countdown and HUD marks go and every bot
stays. On ww5t 2040 the targets, many cut by the screen's edge, score from 0.67 down to 0.32, among overlay text at
0.30 to 0.59. With its level at 0.86, every offset from 0.20 to 0.53 drops real targets; there, leave it off. Moving
targets score low too (Pasu Voltaic, 0.58 to 0.61).

**Submit** saves the cut (and turns it on) and writes it as labels for the detector, in the hand labels' format
(`model/hand_crops.cutoff_crops`): crops round the tracks, the left-out ones as no target and the kept ones as targets,
in `test_out/vod_model/hand/cutoff/` (`checked.jsonl` and `train/`). Nobody checks these crops one by one, so they are
guarded: only frames inside the run (from the first flick to the last kill), only crops that miss every exclude area,
and none holding a track too short to have a score. Up to 20 crops round left-out tracks and 20 round kept ones are
written per recording; a later submit of the same recording replaces its rows. For training they become a dataset with
`python python/model/hand_crops.py --labels test_out/vod_model/hand/cutoff/checked.jsonl --out <dataset> --prefix
cut_crop_`, counted 5 times (a `cut_crop_ 5` line in the `--repeat` file) against 20 for the hand-checked crops.
`faint.json` records when the cut was submitted and how many crops it gave (889.26: 15).

**Cut-off queue** (top bar) goes through recordings one by one, in the area queue's order (uploads first, then the
most recent recording of each scenario), leaving out probes, other games, and recordings already submitted or skipped.
A recording not reviewed yet, or reviewed before the scores were kept, is reviewed first. "Submit and next" submits
and opens the next one; "Skip" leaves it out of the queue from now on (`test_out/vod_app/faint_skipped.json`).

### Which track is the killed target

For each kill (from the stats file or a HUD), the killed target is the track nearest the crosshair just before it. Some
models mark the crosshair itself as a target on some runs (KovaaK's crosshairs of other players, Aim Lab's), at a fixed
spot (`review.crosshair_spots`). Such a track sits at the crosshair, so it always looked nearest, and the kill got a
flick a few frames long that could not be measured: small_v10 measured 142 of 192 flicks on ww5t 1920. A track that
never leaves that spot is now the killed target only when no other track is near (a cost of 1 deg added). With it,
the five YouTube uploads measure 196, 196, 194, 192 and 202 flicks of 197, 198, 195, 192 and 203 kills (small_v11),
and the four stats-file runs, which have no such spot, are unchanged.

A track is judged by its frame nearest the crosshair and the kill, not by its frame 2 after the kill: a track can go
on past the kill and move off (the tracker picked up the next target), which lost the Aim Lab upload's first kill.
When no track is near at the kill, the latest track that ended at the crosshair since the previous kill (up to 1 s
back) is taken: a tiny target held under a red-dot crosshair can stay hidden for longer than the 0.25 s window
(Pokeball 5 and 1 lost one kill each that way). With both, every kill matches a target on all four stats-file runs
(143, 155, 114 and 84), the valorant run (66), the Aim Lab upload (206) and the five YouTube uploads.

For the review from the video alone, the user suggested taking a hidden target's kill as the moment the crosshair moves
off and the target is not there. Measured against the stats files (kills within 3 frames of the true one), placing
the kill one usual reaction time before the next flick starts helped Pokeball 1 (75% to 81%) but hurt 10 Sphere (86%
to 62%) and 1w4ts (96% to 85%), where a slow reaction after a visible kill looks the same; it is not used.

### Stats files

A VOD and its stats file are matched by name. KovOBS names a recording `Scenario - Score - 2026.08.12-00.41.24.mp4`,
and KovaaK's names a stats file `Scenario - Challenge - 2026.08.12-00.41.24 Stats.csv`. They belong together when the
scenario name is the same and the two times are within 5 seconds (the nearest wins). Some KovOBS recordings from June
2026 are named with the year 0026; they are read as 2026. Inside the file, the stats file's kill times are lined up
with the video by one clock offset: the one that puts the most kills on a target vanishing at the crosshair.

A run without a stats file (freeplay) is reviewed from KovaaK's session HUD when the recording shows it: the box
headed SESSION, with Kill Count, KPS, Accuracy (hits/shots), Damage, SPM and Avg TTK. It is optional in KovaaK's, but
most players show it. `hud.py` reads it frame by frame: the Kill Count changes 0 to 2 frames after each kill, and the
Accuracy row gives every shot and hit, so the review gets kill times, shots, hits and misses as good as the stats
file's. The score comes from the file name. The theme can recolour the HUD, so nothing assumes a colour: text is told
from the box by contrast, and the digits are learned from the recording itself. The Kill Count goes up one at a time,
so its last digit cycles 0 to 9, and the shape on the units place when the tens place changes is 0. The reader gives
up (and the next fallback runs) when the box is missing or its readings do not count up. It also handles a box that
widens as its numbers grow, a recording with a restarted attempt (it keeps the stretch with the most kills), and the
reset to zero when the run ends.

Other players' recordings show the HUD smaller or elsewhere, so the reader looks for the box instead of assuming its
size. Small text from a re-encoded upload is blurred: neighbouring digits join (a glyph wider than 0.9 of its height is
cut into digits, which never happens in the user's recordings), and glyphs are compared as grey images, not black and
white, since at that size a 0 and an 8 differ only in grey. The shapes are learned from the Kill Count row alone; a
blurred digit can leave more than one shape, and once the ten digits are known, the others join the most alike digit.
KovaaK's compact HUD (two columns: Kill Count and SPM, Accuracy, Damage, Avg TTK and KPS) puts each value just after
its label's colon, so there the value is read from the colon on. In a very blurred recording one digit can still split
into two shapes and the digits are not learned; then looser likenesses are tried (0.96 down to 0.93), and a reading is
kept only if the Kill Count goes up by one at 95% of its steps or more (0.94 read ww5t 1920 as 105 kills, with 91% of
steps +1). Checked on five 1080p YouTube runs, all read within one kill or shot of their last HUD frame: 1902 1w6ts
small 197 kills, 1931 WR 198, ww5t 2040 203 against 204, Multiclick 195 with 195/219 exact, ww5t 1920 192 (compact
HUD, read at 0.95). The user's 19 recordings read exactly as before. A lone short misread between two readings
that follow on (0, 9, 1 on ww5t 2040: a 1 caught mid-change, 5 frames) is dropped; it had looked like a restart, which
split the run and lost the kill before it. A reading that stays up longer than 10 frames is kept (a real restart's
0 between two 1s on 1w2tes stayed up for 105 frames).

An Aim Lab recording has no session HUD, but Aim Lab shows POINTS, TIME and ACCURACY in boxes at the top center.
`hud.read_aimlab` reads the POINTS number: a hit adds points and a miss takes some off, so every change is one or more
hits and misses (the most common rise is a hit, the most common drop a miss; a jump of two hits, or a hit and a miss,
in one step is split). The digits are learned from the TIME box, which counts down one second at a time. Each hit is
counted as a kill; when the hits outnumber the video's kills by more than 30% (targets that take several hits), the
review falls back to the video. Aim Lab's crosshair is taken for a target by every model, so its tracks are left out
before the kills are matched. On the uploaded Aim Lab run (2007 1w6ts): 206 kills, 217 shots and a score of 2,007,
all exact, with 202 flicks measured.

Checked against the stats files of 15 recordings (click and hold-fire scenarios, 60 and 120 fps): the kill count was
exact in every one, and shots and hits were exact or within 1% (hold-fire counts shots in fast bursts). In click
scenarios every stats kill had a HUD kill within 3 frames; in hold-fire ones 63% to 100%. The Accuracy row can update
up to a third of a second after the Kill Count, so in one-hit scenarios each kill gets one shot plus the misses in its
stretch: on 1w4ts that matched the stats file's shots for all 142 kills. Reviewed through the HUD, the 1w4ts run gave
the same kills, misses, accuracy and flagged checks as with its stats file. The score comes from the file name. A HUD
read takes about 10 s and runs alongside the tracking.

Without a stats file or a readable HUD, the run is reviewed from the video alone. A kill is a target whose track
ends near the crosshair, unless another track picks the same target up again within 0.5 s (allowing for the camera's
turn), it is the crosshair, or three or more steady tracks vanish at once (the run ended). Short, flickering
detections, such as a game's HUD text, do not count towards that last rule: on the uploaded Aim Lab run (2007 1w6ts,
206 kills from 217 shots) they made the review drop half the kills.

A track is the crosshair when it stays on the crosshair while the camera turns, which a static target cannot do, or
when it lasts 3 frames or fewer on a crosshair spot (`review.crosshair_spots`): a fixed point near the crosshair where
detections pile up while the camera turns. Aim Lab's crosshair (a red cross with a dark edge, bigger than its targets)
is marked by every model in about half the turning frames; right after a kill, those one-frame "targets" made the dead
target look picked up again, so its kill was lost or came late. The user's KovaaK's runs have no such spot with
small_v7.

Checked on small_v7's tracks against the stats files (precision, recall, a kill within 3 frames): 1w4ts 0.99, 0.99;
ClickTrack Vertical 2t 1.00, 0.97; 10 Sphere 0.81, 0.82 (156 kills for 155, but often 4 to 6 frames early at 120 fps:
the tiny target is lost under the crosshair just before the click); Pokeball 1 0.67, 0.70; Pokeball 5 0.64, 0.61;
Jumbo1wall9000targets 0.6, 0.1 (a dense field). On the Aim Lab run, against the hits read from Aim Lab's POINTS number
(it adds 10 a hit and takes about 5 a miss): 211 kills, 194 right, 17 extra, 12 missed. Score, shots, misses, accuracy
and sensitivity are not available, and the misses check is skipped.

A recording named other than KovOBS's way (`Scenario - Score - date`) gets no score unless the part after the
scenario starts with a number.

At 60 fps a fast flick can blur the target until it reappears at the crosshair as a new track that is not joined to
the old one; that kill is counted but its flick is not measured (on two YouTube runs 162 of 197 and 155 of 203 flicks
were measured).

### Scenario kinds

`review.scenario_kinds()` sorts every installed scenario into static or dynamic clicking, tracking or switching, from
the game's own tags (`AimTypeTag`, `AimSubTypeTag`). An untagged, older file counts as tracking when its weapon fires
fully automatic, as static when no bot can move, and as dynamic otherwise. The user's KovOBS library holds 290 tracking
folders, 236 static, 142 dynamic and 67 switching (48 have no installed scenario file).

Static, dynamic and switching runs are reviewed the same way: their bots die, so the stats file has a row per kill,
and each kill is matched to a target in the video and its flick measured.

### Tracking runs

In many tracking runs the bots never die, and the stats file holds only totals: score, hits and misses. In others
(84 of the 242 tracking folders with stats files, such as Pokeball 1w4ts 30% or Pasu Track Smaller), bots die and new
ones spawn. The review measures the time on the target (`review.track_summary`). The crosshair is on the target in a
frame when it lies inside a target's box plus 0.05 deg (the model can split a thin capsule into several short boxes,
so any box counts). The report gives:
- **On target:** the share of the time tracking with the crosshair on the target, beside the stats file's accuracy (hits
  over hits and misses), the game's own measure of the same thing.
- **Distance from the center:** the median distance from the target's center line (a sphere's center, a capsule's long
  axis), over the frames on or near it.
- **Lost the bot, per second, and time to get back:** the stretches off the target longer than 0.1 s, per second of
  tracking, and their median length, with the accuracy they cost: their time as a share of the tracking time. The
  shorter slips' cost is given too; with "on target" the two add up to the whole tracking time.
- **Longest off:** the longest of those stretches.
- **On target over the run**, directly under the video: how far outside the bot's edge the crosshair was through
  the run (0 while on it; the average and, lighter, the furthest in each moment), over a strip that is green while on
  the bot, orange while off and grey while switching, with each death marked. A marker follows the video; click or
  drag on it to go there, and point at it for the moment's numbers.

**Bots that die.** Their deaths come from the stats file's kill rows, matched in the video as for clicking runs (every
kill matched on four such runs: 76, 82, 121 and 96), or, without a stats file, from the session HUD's Kill Count. From
a death until the crosshair is on a target again is switching, not tracking. It is left out of "on target while
tracking", the lost stretches and the motion measures below, and measured on its own:
- **Bots killed**, and the **time to the next bot** (median), split into **waiting for a spawn** (no target on screen)
  and **getting onto it** (from the first target shown to being on it).
- **Switching:** the share of the run it took.
- **On target, whole run:** switching included, as the stats file's accuracy counts it (on Pokeball 1w4ts 30%: 70% while
  tracking, 27% over the whole run, 24% accuracy).
- The timeline's strip shows switching in grey, and it and the seek bar mark each death.

**The run's window.** With a stats file, the run starts at its Challenge Start, placed on the video's clock by the
matched kills, and its length is the gap between that and the stats file's name (written when the run ended), rounded
up to the second (`review.stats_length`). The scenario's `Timelimit` can differ from what was played (Pasu Track
Smaller: 42 against 60 s). Without matched kills, the run starts on the frame after KovaaK's countdown ("Challenge
begins in", over a teal bar) last shows (`review.countdown_end`): a recording can begin a few seconds before the
scenario is restarted, with a bot already showing (Controlsphere: the countdown ends 2.13 s in, cA FBS Easy 2.81 s).
Where no countdown is found (another player's upload with a different UI scale), the run ends at the last frame with
the crosshair on a bot and starts its length before that. Without a stats file, its length is the
`Timelimit` (`review.scenario_facts`), stretched to the last death if that comes later. KovOBS goes
on recording about 5 s of the results screen, which this leaves out. On 12 held-out tracking runs, full_v3's time on
target came within 0.075 of the stats file's accuracy on average (`model/eval_moving.py`); thin capsules and small
targets held under the crosshair read low (`model/MODEL_STATUS.md`).

**On the video**, "Show the tracked target" boxes the bot the review takes for the target in each frame (the track
nearest the crosshair, by its center line): green while the crosshair is on it, orange with a dashed line and the
distance outside its edge while off, and "switching" after a death until the crosshair is on a target again. The other detected
targets get a thin grey box. The path options are hidden, since a tracking run has no flicks.

"How you followed the target" (since 2026-10-02) splits the motion in two, from the video alone, with no mouse log, so
it works on anyone's recording:
- **The camera's turn** (`review.camera_motion`): as the view turns, the room slides across the screen. Each frame is
  resampled onto a grid of degrees round the crosshair (36° either side, 18° up and down) and cut into 18 tiles of
  12°, and each tile is matched with the frame before (phase correlation). Tiles with a tracked target in them, HUD,
  excluded areas or the fixed map, or too little texture are left out. The frame's reading is the mean of the tiles
  that agree with their median within 0.1°, if 3 or more do. Checked on static runs, where the static targets'
  common move is the truth: median error 0.013 to 0.042°, 90th percentile 0.05 to 0.11°. Plain white walls leave
  22% of frames without a reading, tiled walls 2%. It takes about 20 s for a 60 s run at 120 fps.
- **The target's own motion:** its move on screen less the room's. On Aethercontrol Easy (a strafing sphere) it reads
  a steady 13°/s, with a direction change every 0.62 s (median).

From these, `review.track_motion` measures, while the target moves (over 5°/s) and the crosshair is with it (within
2° of its center line, or 5 radii for a big target):
- **Behind or ahead:** the median offset along the target's motion, in degrees and in ms at its speed.
- **Off target: behind, ahead, to the side:** the share of the off-target time spent trailing the bot, ahead of it
  (past its leading edge), or beside its path.
- **Overshoots a second:** stretches of 2 frames or more ahead of the target past its edge, with the median distance
  past it.
- **Over-correcting:** the share of your corrections that went too far. A correction is each turn of the crosshair
  back toward the bot's middle along its motion (jitter under 0.05° ignored); it went too far when it crossed over
  the middle to the other side by half the bot's width or more, while the bot kept its direction. Shown with the
  counts (Aethercontrol Easy: 4%, 12 of 280 corrections).
- **Direction changes:** the target's reversals (horizontal or vertical, 8°/s or more either side). The reaction is
  the median time until the mouse moves the new way. "Carried past" is the share of changes after which the crosshair
  went on the old way past the target's edge within 0.4 s, with the median distance.
- **Horizontal and vertical distance** from the target's center line.
- **By the target's direction:** for each of 8 directions of its motion, the share of the time, the share on target,
  the distance from the line, and behind or ahead.

The measures need 10 s or more of such motion and a fifth of the run; otherwise the report says why. Pokeball Frenzy
(several slow targets, killed in turn) has 2 s. On a scenario with several targets at once, the target is the one
nearest the crosshair. The measures have been checked for sense on six tracking runs, but not against a known path:
the next check is a test scenario whose bot follows set waypoints.

**What would raise your accuracy** (`review.what_if`) lists changes, each with how much more of the run you would have
been on the bot had that one thing been different and all else the same: the off-target time it accounts for, as a
share of the whole run (as the game's accuracy counts it). They overlap, so they do not add up; each is the most that
change could give. The options:
- **Keep up your best 10 seconds all run:** your best 10-second stretch's time on target, kept up.
- **Get back on twice as fast:** half the time off in drops longer than 0.1 s.
- **Don't trail:** the time off behind the bot. **Don't lead:** the time off ahead of it, past its edge.
- **Don't get thrown by its turns:** the time off in the 0.4 s after each of its direction changes.
- **Don't slip:** the time off in slips shorter than 0.1 s.
- **Track every direction like your best one:** each direction of its motion brought up to your best direction's time
  on target (among directions it moved in 5% of the time or more).
- **Get onto the next bot 100 ms faster** (when bots die): 100 ms less per switch.

"Don't lose the bot" (all the time off in drops) was dropped: it is nearly all the off-target time, so it says
nothing you can act on (the user, 2026-10-02).

**Set run** (beside "Review again") marks where the run starts and ends, for when the review gets it wrong: Start
and End (typed as 1:02.5 or 62.5, or "Here" for the video's current time) and Length in seconds. Any two settle the
third; a start or an end alone takes the stats file's length (or the scenario's). Save keeps them per recording
(`run.json` in its cache folder) and measures the run again on its tracks and camera reading (a few seconds; the
camera reading is kept in `camera.json`); "Automatic" forgets them. A tracking run's measures use the marks before any
other start or length; on a clicking run they bound the path overlay. Saved while a review of the recording runs,
they are applied when it ends.

## Detector model

The hand-written detector assumes dark targets, so the review uses a small trained model when it is there
(`model/`, `infer.BEST`: full_v3 since 2026-10-02). The model finds targets of any colour, on any theme, and through
the crosshair, still or moving, spheres and capsules; it gives a box and a score per target. `server.py --detector auto` (the default) uses the model on the GPU through PyTorch, or on the CPU through
ONNX Runtime, and falls back to the hand-written detector when neither is available; `--detector hand` forces the
hand-written one. With the model, the review of a 66-second 120 fps VOD takes about 13 s on an RTX 5070 Ti.

On other machines (measured here, with the GPU switched off for the CPU case):
- **No NVIDIA GPU:** the model runs on the CPU through ONNX Runtime. Tracking that VOD took 30 s on an 8-core Ryzen 7
  9800X3D; a typical 6-core laptop is likely two to three times slower. Recordings at 60 fps take half as long. Kills
  match as on the GPU, but a few targets held under the crosshair are confirmed less often (fp32, not bf16).
- **An older NVIDIA GPU:** the model is tiny (0.59 GMAC a frame), so any CUDA card keeps up. Cards before the RTX 30
  series run fp32 instead of bf16, with the same small difference as the CPU.
- **The HUD reader and the hand-written detector** run on the CPU everywhere (about 10 s and 20 to 35 s here).

- [model/MODEL_STATUS.md](model/MODEL_STATUS.md): what the model is, how well it works, its speed on CPU, GPU, a web
  server and the browser, its limits and the next steps.
- [model/REPRODUCE.md](model/REPRODUCE.md): every command, from building the dataset to the deployment prototypes.

## Setup

- The tracker needs ffmpeg, NumPy and SciPy. The model needs ONNX Runtime (CPU) or PyTorch (GPU).
- The video needs manim. It lives in its own Python 3.12 environment, so the main Python stays untouched:

  ```bash
  py -3.12 -m venv test_out/manim_venv
  test_out/manim_venv/Scripts/python -m pip install manim
  ```

- No LaTeX is installed, so the video uses plain text (Segoe UI) and no `MathTex` or number labels from manim.

## Raw mouse log

Video gives at most 120 frames a second, and unevenly. The mouse (Razer Viper V3 Pro) reports up to 8000 times a
second. `mouse_log.py` logs that raw stream during a run, and `mouse_read.py` measures each flick from it: when the
mouse starts and stops, its peak speed, and above all how long the crosshair sits still on the target before the
click. Both use only Python's standard library. They were written on 2026-10-01 and checked on synthetic runs only;
the first real run will show whether Windows throttles the logger (see below).

### Logging a run

1. Start the logger in a terminal:

   ```bash
   python python/mouse_log.py
   ```

   It prints the file it writes, `test_out/mouse/mouse_<date>_<time>.bin`. `--out` picks another file, and
   `--seconds N` stops the logger after N seconds.
2. Play the run in KovaaK's in challenge mode, so the stats file is written. The logger keeps logging while the game
   has focus. It only listens: it sends no input and never moves the cursor.
3. Press Ctrl+C in the terminal. The logger prints the number of events, the duration, the mean rate and the devices
   it saw. If the terminal kills it instead of passing Ctrl+C on, the log still holds everything up to the last
   quarter second. `--seconds N` always stops cleanly.

`python python/mouse_log.py --bench` times the logger's work per event without any real input. On 2026-10-01 it took
about 3 µs, so the logger can handle about 330,000 events a second. An 8000 Hz mouse needs 8,000. The file format is
in the docstring of `mouse_log.py`.

### The Windows throttle

Windows 11 limits raw mouse input to programs in the background to about 125 messages a second. It sums the motion
in between, so no motion is lost, but the times are then only good to about 8 ms. Microsoft added this in 2023 to
cut game stutter with high polling rate mice. While KovaaK's has focus, the logger is in the background, so the
limit applies to it.

The logger prints the throttle setting at start, and both scripts warn when the events come about 8 ms apart. To log
at the full rate, set the DWORD `RawMouseThrottleEnabled` to 0 under `HKEY_CURRENT_USER\Control Panel\Mouse`. Microsoft
does not document this value. Tools that change it say it takes effect after you sign out and back in. Delete the
value after logging to return to the default, because the throttle protects the game from stutter.

### Reading a log

```bash
python python/mouse_read.py test_out/mouse/<log>.bin
python python/mouse_read.py test_out/mouse/<log>.bin --stats "<stats csv>"
python python/mouse_read.py --selftest
```

The first command prints a summary: the duration, the event rate, the total travel in degrees and the left-button
presses. The event rate has two numbers. The median interval between events gives the rate while the mouse moves.
The busiest 100 ms gives the highest rate seen; at 8000 Hz it should come near 8000 when the mouse moves fast.

The second command measures the run. It reads the dpi and cm/360 from the stats file; `--dpi` and `--cm360` override
them. It matches each kill in the stats file with the left-button press that made it. The stats clock and the log's
clock can differ a little, so it finds the one offset that lines up the most kills with presses. It prints that
offset and how many kills matched within 10 ms. The gap between a press and its kill should be a millisecond or two;
when Windows throttles the logger, it grows to several. Presses in the run that killed nothing are misses. The kills
plus the misses should equal the shots in the stats file.

For each kill, from the previous kill's press to this one, it prints the median, p10 and p90 of these numbers:

- **Reaction:** the time from the previous press until the mouse starts moving, when the speed first reaches
  30 deg/s.
- **Flick:** the time from that start until the mouse stops, when the speed stays under 10 deg/s for 5 ms.
- **Peak speed:** the highest speed in the flick, in deg/s. Each speed is taken over 4 ms around its moment.
- **Stop to click:** the time from the stop to the click. It includes any corrections after the flick.
- **Still before the click:** how long the speed stayed under 10 deg/s right before the click. This is the time the
  crosshair sat still on the target. It is 0 when the click came while the mouse moved.
- **Speed at the click:** the speed over the last 2 ms before the click.
- **Distance:** how far the crosshair moved from the previous press to this one, in degrees.

It also counts the clicks made while moving, the kills with no stop before the click, and the kills with
corrections. It writes every kill to `<log name>.kills.json` next to the log. Each kill holds its press time in
local time, so it can be found in the video. `--window`, `--start`, `--stop` and `--hold` change the speed window and
the thresholds.

`--selftest` builds a synthetic run of 20 known flicks in `test_out/mouse/selftest/` and checks that the reader finds
them. The starts, stops, still times and clicks come out within 1 ms of the truth, and the peak speeds within 5%.
The peak reads a few percent high, because the jitter in when the logger handles each message moves the edges of the
4 ms window.
