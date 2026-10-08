# Storage: one SQLite database per data folder

Status: steps 1 to 3 done (2026-10-07); step 4 (the data panel and the export zip) done (2026-10-08). The
prototype that backs it: `prototypes/sqlite_opfs/`
(README.md has its numbers).

## Why

Today the review service keeps everything as plain files in its data folder (`disk.rs`: the real disk on the desktop
and the server, OPFS through the page in the browser), and browser mode adds a second store, IndexedDB. That costs:

- **Size.** A review is plain JSON floats (av1: tracks.json 1.17 MB, readings.json 0.32 MB), a copy per model used,
  never pruned. Browser mode copies all 72,083 of KovaaK's stats files (401 MB) into append-only packs that never
  shrink.
- **Speed.** Listing recordings is a file call per folder and file; in the browser each call also waits on the page
  (Asyncify). The prototype listed 1,000 recordings in 179 ms from files, 6.8 ms from SQLite, before any service
  overhead.
- **Two stores in the browser** (OPFS and IndexedDB), no transactions, no version for the layout, no way to compact,
  and nothing to export or back up as one thing.

## What goes where

One database file, `aimview.sqlite3`, in the data folder of the app's layout (desktop, server, browser).

| In the database | Stays files |
| --- | --- |
| settings; the recordings and each one's marks (run window, picked stats file, cut-off, labelling flags); reviews; areas and the area finder's results; area kinds and examples; cut-off labels; KovaaK's stats files and scenarios (browser mode); mouse logs | the recordings (read where they are); uploads and remuxes (videos); the models; ffmpeg; the crop-check folders (training tooling); aimview-tool's `--out` folders (Python reads them) |

The Python layout (`test_out/`, the training workbench) stays on files: Python's scripts read its
`area_examples.jsonl`, crop-check folders and `checked.jsonl`. So the library gets one storage interface with two
backends: files (today's code, for the Python layout) and SQLite (the app's layout).

## Schema, version 2

`PRAGMA user_version = 2`. Times are seconds since 1970 (REAL). Table names are singular. The first four tables keep
what store.rs's `Item`s name, one row each, so the database gives back the bytes `Files` would
(service/src/database.rs). Version 1 had them in the plural and no KovaaK tables; opening one renames them and adds
those.

| Table | Columns | Holds |
| --- | --- | --- |
| `library` | `name TEXT PRIMARY KEY, bytes BLOB, changed REAL` | the library's own items by file name: settings.json, area_kinds.json, area_examples.jsonl, exclude_uploads.json, the three id lists, the cut-off's checked.jsonl |
| `mark` | `recording TEXT, mark TEXT, bytes BLOB, changed REAL, PRIMARY KEY (recording, mark)` | each recording's marks by its folder name (slug) and the mark's file name: run.json, stats.json, faint.json, exclude.json, areas.json, areas_maps.npz |
| `review` | `recording TEXT, model TEXT, part TEXT, bytes BLOB, changed REAL, PRIMARY KEY (recording, model, part)` | each review's parts (tracks, readings, hud, kills), gzip-compressed; `model` is "" for the old review |
| `cutoff_crop` | `file TEXT PRIMARY KEY, bytes BLOB, changed REAL` | the cut-off labels' crops by their file (train/<name>.npz) |
| `stats_file` | `name TEXT PRIMARY KEY, size INTEGER, modified REAL, score REAL, kills REAL, accuracy REAL, csv BLOB` | browser mode only: each of KovaaK's stats files once read, its run (no score: null), and its whole text gzip-compressed only when one of the user's recordings pairs with it |
| `scenario` | `path TEXT PRIMARY KEY, size INTEGER, modified REAL, facts TEXT` | browser mode only: each scenario file's facts (kind, time limit, targets, ammo, hitbox) as JSON, by its path in /kovaak |

Text Python keeps as text is kept with Windows' line ends, as `Files` writes it. Models and recordings are listed in a
Windows folder's order (NTFS: by upper case), as `Files` read them. The proposal's wider tables (a recording's path,
size and scenario; mouse logs) come with the steps that need them.

KovaaK's files in browser mode are read once, not copied (2026-10-07): the page sends each file new or changed since
it last sent it (kovaak-batch.ts, POST /api/kovaak_files in batches of up to 1,000 files or 8 MB); the service keeps
its run or facts in one transaction a batch, and the whole text of a stats file a recording pairs with (by its name's
scenario and time, within 5 s). A stats file picked by hand is kept when a visit that chose the folder reads it;
otherwise its report asks for the folder again. The packs the earlier version copied (72,083 files, 401 MB) are sent
once in the background (kovaak-move.ts) and removed. Natively the service reads KovaaK's folders as before.

Reviews are one row per part, not a row per frame: 6,000 frames of several boxes each would make millions of rows,
slower to write and to read, and the core reads the whole review at once anyway. On the desktop and the server the
stats files are read from KovaaK's folder as today (no copy); only the browser will need a copy.

## The engine in each mode

- **Desktop and server:** `rusqlite` (SQLite bundled into the exe), WAL journal, one connection per process behind a
  mutex. One process opens a data folder at a time, as today.
- **Browser:** SQLite's own WebAssembly build in the service's worker, on OPFS through its sync-access-handle pool
  (the prototype's setup). The Rust service reaches it through a second host interface beside `host_fs`: `host_sql`,
  a statement and its parameters in, rows out, in a small binary form (blobs as raw bytes, not base64). Sync access
  handles are synchronous, so these calls return at once: no Asyncify wait, unlike today's file calls.
- **The Rust side** talks to one `Sql` trait (execute, query rows, a transaction), with `rusqlite` behind it natively
  and `host_sql` in the browser, so the SQL and the migrations are written once.

Not chosen: compiling SQLite into the service's own WebAssembly (`sqlite-wasm-rs`). It brings wasm-bindgen into a
module that has none and goes through Binaryen's Asyncify; SQLite's own build in the worker keeps the service as it is.

## Tabs (browser)

A sync access handle is exclusive: one tab at a time can hold the database. The tab that opens first takes a Web Lock
(`navigator.locks`, "aimview-db"). A second tab asks for it; the first is told on a BroadcastChannel, finishes what it
is doing, closes the database and lets go; the second opens it and the first shows "open in another tab". The old
tab can take it back the same way.

## Migration from today's files

On opening a data folder whose database is missing (or at `user_version` 0), the service imports what is there, in one
transaction: settings.json, each `reviews/<id>/` (its marks, areas and every `models/<model>/` review, compressed on
the way in), area_kinds.json, area_examples.jsonl, the labelling lists, `cutoff/`, `mouse/`; in the browser also the
KovaaK packs, the cut-off labels in IndexedDB and the old IndexedDB store. Nothing is deleted: the old files stay where
they are, marked as imported, and the data panel offers to remove them once the user is happy. A review read from the
database must give the same report as the same review read from its files: that is the migration's check.

IndexedDB keeps one entry only: the VODs folder's handle, which nothing else can hold. Everything else leaves it.

## Space and cleanup

- Reviews are compressed (the prototype: 50 reviews in 23.2 MB against 74.5 MB).
- The data panel lists what is kept, with sizes: reviews (by model: those of models no longer listed can go), uploads
  and remuxes, KovaaK's copy, cut-off labels, old files left by the migration, ffmpeg (desktop). Each can be removed;
  the database is compacted (`VACUUM`) afterwards.
- Browser mode asks for persistent storage (`navigator.storage.persist()`), so the browser does not evict it under
  pressure.

## Export (for sending recordings to the developer)

A zip, the same from every mode: `manifest.json` (format version, app version, model, device, mode, what each
recording includes), and a folder per recording with its review (plain JSON, as today's files), its stats file and
scenario, its marks, and its video when the user includes it. It is built from the storage interface, so it works
before and after the move to SQLite; the developer opens it with "Open an export", which adds the recordings as
uploads with their reviews. In the browser the zip is written to disk as it is made (Chrome's save dialog), so a large
video never sits in memory.

## Steps

Each ends in a check, and each is committed on its own.

1. **The storage interface.** The library's reads and writes go through one interface, its file backend being today's
   code. Check: the replay test, the parity tests and the UI's tests pass unchanged; the API's answers on a copy of
   test_out are byte-equal (the apicheck scripts). Done: `Store` in service/src/store.rs. An `Item` names each thing
   kept (the settings, the lists of ids, the area kinds and examples, a recording's marks, a review's parts by model,
   the cut-off labels); a store takes bytes and gives the same bytes back, and answers what a recording has (its
   reviews, the old one, which recordings have anything kept, which have saved and found areas). `Files` keeps them as
   before, in either layout, and Python's text items with its line ends. Checked with `scripts/storage_check.py`: the
   API's answers and every file left are the same as the build before it, on copies of test_out and the desktop
   app's data; the Rust tests pass.
2. **SQLite natively.** The `Sql` trait, `rusqlite`, the schema, the import from files. Check: on a copy of the desktop
   app's data and of test_out converted to the app's layout, every review's report and the API's answers equal the
   file backend's. Done: `Database` (service/src/database.rs) over `Sql` (service/src/sql.rs, rusqlite natively), on
   for the app's layout natively (`Config.database`); the review server keeps Python's layout and its files. Checked
   with `scripts/storage_check.py backends`: test_out converted to the app's layout twice, every reviewed recording's
   report and tracks and the check's questions answered the same by both stores; the database's unit tests compare
   it with `Files` on a small data folder.
3. **SQLite in the browser.** `host_sql`, SQLite's build in the worker, the tab lock, the import from OPFS files and
   IndexedDB. Check: the browser mode's specs, and a review made and reopened in Chrome, the same report as natively.
   Done: `HostSql` (service/src/sql.rs), answered by ui/src/app/modes/service/service-database.ts (SQLite 3.53's
   WebAssembly, `bun run assets` copies it to service/sqlite3.wasm; its pool in OPFS `.aimview-sqlite`). `host_sql`
   is synchronous, so it is not one of Asyncify's imports, and the import from files works through it. A tab that
   opens steals the Web Lock; the tab that held it closes the database after its running request and answers 503
   with "open in another tab" until it is reloaded. The page reads and replaces area_kinds.json through
   /api/area_kinds_file instead of the data folder. Checked in Chrome (the built-in browser): a recording and its
   review seeded as files, imported on the first opening, answered every route as the native files did (tracks,
   report, run, stats, exclude, faint, job, mouse; find_areas differs only by the other recordings each has);
   writes (a run window, the 327 KB examples, the area types) read back after a reload; the lock passed both ways
   between two tabs. The cut-off labels the page kept in IndexedDB moved later that day: the service keeps a
   submit's labels (POST /api/cutoff_labels) and builds the zip in every mode (GET /api/cutoff_labels); the old ones
   are sent once and marked moved. A new review made in the browser was not run (the pane was not drawing). KovaaK's files were moved later the same day (version 2, above): checked in
   the built-in browser with 40 of the user's stats files and a scenario sent as a batch, the history equal to
   native's for those 40 runs, the pairing, the scenario's kind, the paired text kept across a reload, and a layout-1
   database and the old copies moved on opening. On 2026-10-08 all 72,127 of the user's stats files (401 MB) were
   sent the same way: 6.1 s once in the page, a 14.5 MB database, nothing sent when chosen again (docs/COSTS.md); a
   new review on WebGPU (6.6 s) was kept in the database and its report read back. Not tried: the move of a real
   401 MB pack set (only the user's own Chrome has one).
4. **The data panel** and **the export zip**, on the interface. The data panel is done (2026-10-08): the top bar's
   Storage button (ui/src/app/storage-panel/, the StoredData contract, GET and POST /api/storage,
   service/src/library/usage.rs) lists each part with its size, largest first: each model's reviews (and whether
   models.json still offers the model), the user's marks and areas, the cut-off labels, KovaaK's files the browser
   keeps, the videos added, the mouse logs, the files from before the database, and the downloaded ffmpeg. Parts the
   user made are listed but never removed there; the others take a second click, and the database is compacted
   (VACUUM) after. Browser mode copies its shipped area data into /data only on its first run, so removed old files
   do not come back. The export zip is done too (2026-10-08): the Storage dialog's Share section picks reviewed
   recordings and writes one zip (service/src/library/export.rs gives each recording's reviews, marks, stats file
   and scenario facts through POST /api/export; the page adds the videos, streamed, ui/src/app/modes/web-files/
   zip-stream.ts, zip64), to a file the user picks or as a download. Opening one (zip-read.ts slices the zip, so a
   video is never read whole) adds each recording that has its video as an upload with its stats file, then its
   reviews, marks and facts (POST /api/import). Checked in the built-in browser: a recording exported with its
   video, its review and uploads removed, then the zip opened gave back the same 1 MB of tracks byte for byte, its
   run window and its 7 saved areas; Python's zipfile reads the writer's zips.

## Open questions

- `host_sql` passes straight through Asyncify (step 3): it is not one of its imports, and calls before a `host_fs` wait
  are not made again when the stack rewinds.
- How large the database grows for a heavy user (thousands of recordings and several models), and whether reviews of
  old models are pruned automatically or only from the data panel.
- The crop-check folders stay files in this design; they could become a table later.
- The mouse logs stay files after step 1: the desktop app's logger (a process of its own) writes them, and the
  measures read only each log's first and last records to find the one that covers a run. In the database they need
  the logger to write through the service, or an import when the service starts, and a read of a blob's ends.
