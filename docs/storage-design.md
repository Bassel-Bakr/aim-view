# Storage: one SQLite database per data folder

Status: step 1 done (2026-10-07), steps 2 to 4 proposed. The prototype that backs it: `prototypes/sqlite_opfs/`
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

## Schema, version 1

`PRAGMA user_version = 1`. Times are seconds since 1970 (REAL), JSON is text, blobs are bytes.

| Table | Columns | Notes |
| --- | --- | --- |
| `settings` | `key TEXT PRIMARY KEY, value TEXT` | each setting's JSON |
| `recordings` | `id TEXT PRIMARY KEY, path TEXT, size INTEGER, modified REAL, scenario TEXT, game TEXT, kind TEXT, run TEXT, stats_file TEXT, faint TEXT, not_aim_trainer INTEGER, label_skipped INTEGER` | one row per recording the app has seen; `run` and `faint` are today's run.json and faint.json |
| `reviews` | `recording TEXT, model TEXT, version INTEGER, made REAL, device TEXT, tracks BLOB, readings BLOB, hud BLOB, kills BLOB, PRIMARY KEY (recording, model)` | each blob is today's JSON, gzip-compressed: read back, the same bytes, so reports stay byte-identical |
| `areas` | `recording TEXT PRIMARY KEY, excluded TEXT, found TEXT, maps BLOB` | today's exclude.json, areas.json and areas_maps.npz |
| `area_kinds` | `id TEXT PRIMARY KEY, kind TEXT` | area_kinds.json |
| `area_examples` | `id INTEGER PRIMARY KEY, example TEXT` | one row per line of area_examples.jsonl |
| `cutoff_labels` | `id INTEGER PRIMARY KEY, recording TEXT, row TEXT, crop BLOB` | a checked.jsonl row and its crop's npz; the download (cutoff.zip) is built from them |
| `stats_files` | `name TEXT PRIMARY KEY, scenario TEXT, played REAL, size INTEGER, modified REAL, csv BLOB` | browser mode's copy, the CSV text gzip-compressed (the core parses it as today); index on `(scenario, played)` |
| `scenarios` | `name TEXT PRIMARY KEY, source TEXT, kind TEXT, time_limit REAL, targets INTEGER, sce BLOB` | browser mode's copy of the .sce files and what the service reads from them |
| `mouse_logs` | `name TEXT PRIMARY KEY, recorded REAL, log BLOB` | today's mouse/*.bin |

Reviews are one blob per kind, not a row per frame: 6,000 frames of several boxes each would make millions of rows,
slower to write and to read, and the core reads the whole review at once anyway. On the desktop and the server the
stats files are read from KovaaK's folder as today (no copy); `stats_files` and `scenarios` fill only in the browser,
which cannot keep access to a folder under Program Files.

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
   file backend's.
3. **SQLite in the browser.** `host_sql`, SQLite's build in the worker, the tab lock, the import from OPFS files and
   IndexedDB. Check: the browser mode's specs, and a review made and reopened in Chrome, the same report as natively.
4. **The data panel** and **the export zip**, on the interface.

## Open questions

- Whether `host_sql`'s calls from the Rust service, which runs through Asyncify, need anything special: a sync import
  should pass straight through, and step 3 starts by proving it.
- How large the database grows for a heavy user (thousands of recordings and several models), and whether reviews of
  old models are pruned automatically or only from the data panel.
- The crop-check folders stay files in this design; they could become a table later.
- The mouse logs stay files after step 1: the desktop app's logger (a process of its own) writes them, and the
  measures read only each log's first and last records to find the one that covers a run. In the database they need
  the logger to write through the service, or an import when the service starts, and a read of a blob's ends.
