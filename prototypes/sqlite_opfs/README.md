# SQLite on OPFS: prototype

Whether browser mode can keep its data in one SQLite database on OPFS (the origin private file system), as
docs/storage-design.md proposes: SQLite's WebAssembly build (`@sqlite.org/sqlite-wasm`, its sync-access-handle pool
VFS) in a worker, as the app's review service runs, against today's storage (a file per review file on OPFS), with a
real review (av1, 6,000 frames: tracks.json 1.17 MB, readings.json 0.32 MB).

```bash
cd prototypes/sqlite_opfs && bun install
# copy a review's tracks.json and readings.json into sample/ (not in git: they are the user's data)
bun server.ts            # http://localhost:8790/, then Run (Chrome)
```

Results (2026-10-07, Chrome on the Ryzen 7 9800X3D, 50 reviews and 1,000 recordings):

| | SQLite on OPFS | Files on OPFS (today) |
| --- | --- | --- |
| Open (load SQLite, the pool and the database) | 41 ms | |
| Add 1,000 recordings | 22 ms (one transaction) | 1,162 ms (a folder and a file each) |
| List 1,000 recordings with "has a review" | 6.8 ms | 179 ms |
| Write a review | 20.5 ms, plus 42 ms to gzip it | 5.1 ms (plain JSON) |
| Read a review | 15.6 ms with gunzip, the same bytes | 3.6 ms |
| Size of 50 reviews | 23.2 MB | 74.5 MB |

The files' numbers are the page's own OPFS calls; in the app each call also goes through the review service and waits
on the page (Asyncify), so today's real costs are higher. SQLite's WebAssembly is 869 KB before compression.
