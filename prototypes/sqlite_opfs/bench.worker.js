// The prototype's benchmark, in a worker as the app's review service runs: one SQLite database on OPFS (SQLite's
// WebAssembly build, its sync-access-handle pool) against today's way (a file per review file in OPFS), with a real
// review (av1, 6,000 frames: tracks.json 1.17 MB, readings.json 0.32 MB). Posts each result to the page.
import sqlite3InitModule from '/node_modules/@sqlite.org/sqlite-wasm/dist/index.mjs';

const REVIEWS = 50;
const RECORDINGS = 1000;
const say = (row) => postMessage(row);
const ms = (start) => performance.now() - start;

async function gzip(bytes) {
  return new Uint8Array(await new Response(new Blob([bytes]).stream().pipeThrough(new CompressionStream('gzip'))).arrayBuffer());
}
async function gunzip(bytes) {
  return new Uint8Array(await new Response(new Blob([bytes]).stream().pipeThrough(new DecompressionStream('gzip'))).arrayBuffer());
}
const same = (a, b) => a.length === b.length && a.every((value, i) => value === b[i]);

async function sqlite(tracks, readings) {
  let start = performance.now();
  const sqlite3 = await sqlite3InitModule();
  const pool = await sqlite3.installOpfsSAHPoolVfs({ name: 'aimview-proto', clearOnInit: true });
  const db = new pool.OpfsSAHPoolDb('/aimview.sqlite3');
  say({ group: 'SQLite on OPFS', what: 'open (load SQLite, the pool, the database)', value: ms(start), unit: 'ms' });
  db.exec(`CREATE TABLE recordings (id TEXT PRIMARY KEY, scenario TEXT, size INTEGER, modified REAL, kind TEXT);
           CREATE TABLE reviews (recording TEXT, model TEXT, version INTEGER, tracks BLOB, readings BLOB,
                                 PRIMARY KEY (recording, model));`);

  start = performance.now();
  db.transaction(() => {
    const insert = db.prepare('INSERT INTO recordings VALUES (?, ?, ?, ?, ?)');
    for (let i = 0; i < RECORDINGS; i++) insert.bind([`rec_${i}`, `Scenario ${i % 40}`, 1e9 + i, 1.7e9 + i, 'static']).stepReset();
    insert.finalize();
  });
  say({ group: 'SQLite on OPFS', what: `add ${RECORDINGS} recordings (one transaction)`, value: ms(start), unit: 'ms' });

  // a review is kept compressed: gzip as the page or the service would
  start = performance.now();
  const [packedTracks, packedReadings] = await Promise.all([gzip(tracks), gzip(readings)]);
  const gzipMs = ms(start);
  start = performance.now();
  for (let i = 0; i < REVIEWS; i++) {
    db.exec({ sql: 'INSERT INTO reviews VALUES (?, ?, 3, ?, ?)', bind: [`rec_${i}`, 'large_v13e4', packedTracks, packedReadings] });
  }
  say({ group: 'SQLite on OPFS', what: 'write a review (compressed, its own transaction)', value: ms(start) / REVIEWS, unit: 'ms', note: `+ ${gzipMs.toFixed(0)} ms to compress once` });

  start = performance.now();
  let ok = true;
  for (let i = 0; i < REVIEWS; i++) {
    const row = db.selectArray('SELECT tracks, readings FROM reviews WHERE recording = ? AND model = ?', [`rec_${i}`, 'large_v13e4']);
    const [t, r] = await Promise.all([gunzip(row[0]), gunzip(row[1])]);
    if (i === 0) ok = same(t, tracks) && same(r, readings);
  }
  say({ group: 'SQLite on OPFS', what: 'read a review (and decompress)', value: ms(start) / REVIEWS, unit: 'ms', note: ok ? 'the same bytes as written' : 'BYTES DIFFER' });

  start = performance.now();
  const listed = db.selectArrays(`SELECT r.id, r.scenario, EXISTS (SELECT 1 FROM reviews v WHERE v.recording = r.id)
                                  FROM recordings r ORDER BY r.modified DESC`);
  say({ group: 'SQLite on OPFS', what: `list ${RECORDINGS} recordings, each with "has a review"`, value: ms(start), unit: 'ms', note: `${listed.length} rows` });

  const size = pool.exportFile('/aimview.sqlite3').length;
  say({ group: 'SQLite on OPFS', what: `database size (${REVIEWS} reviews, ${RECORDINGS} recordings)`, value: size / 1e6, unit: 'MB' });
  db.close();
}

async function files(tracks, readings) {
  const root = await navigator.storage.getDirectory();
  await root.removeEntry('aimview-proto-files', { recursive: true }).catch(() => {});
  const base = await root.getDirectoryHandle('aimview-proto-files', { create: true });
  const write = async (dir, name, bytes) => {
    const writable = await (await dir.getFileHandle(name, { create: true })).createWritable();
    await writable.write(bytes);
    await writable.close();
  };

  let start = performance.now();
  for (let i = 0; i < RECORDINGS; i++) {
    const dir = await base.getDirectoryHandle(`rec_${i}`, { create: true });
    await write(dir, 'run.json', new TextEncoder().encode('{"start":0,"end":60}'));
  }
  say({ group: 'Files on OPFS (today)', what: `add ${RECORDINGS} recordings (a folder and a small file each)`, value: ms(start), unit: 'ms' });

  start = performance.now();
  for (let i = 0; i < REVIEWS; i++) {
    const dir = await (await base.getDirectoryHandle(`rec_${i}`)).getDirectoryHandle('models', { create: true })
      .then((models) => models.getDirectoryHandle('large_v13e4', { create: true }));
    await write(dir, 'tracks.json', tracks);
    await write(dir, 'readings.json', readings);
  }
  say({ group: 'Files on OPFS (today)', what: 'write a review (plain JSON, two files)', value: ms(start) / REVIEWS, unit: 'ms' });

  start = performance.now();
  for (let i = 0; i < REVIEWS; i++) {
    const dir = await (await (await base.getDirectoryHandle(`rec_${i}`)).getDirectoryHandle('models')).getDirectoryHandle('large_v13e4');
    await (await (await dir.getFileHandle('tracks.json')).getFile()).arrayBuffer();
    await (await (await dir.getFileHandle('readings.json')).getFile()).arrayBuffer();
  }
  say({ group: 'Files on OPFS (today)', what: 'read a review', value: ms(start) / REVIEWS, unit: 'ms' });

  start = performance.now();
  let reviewed = 0;
  for await (const [name, handle] of base.entries()) {
    const has = await handle.getDirectoryHandle('models').then(() => true, () => false);
    if (has) reviewed += 1;
  }
  say({ group: 'Files on OPFS (today)', what: `list ${RECORDINGS} recordings, each with "has a review"`, value: ms(start), unit: 'ms', note: `${reviewed} with a review` });
  say({ group: 'Files on OPFS (today)', what: `size (${REVIEWS} reviews)`, value: (REVIEWS * (tracks.length + readings.length)) / 1e6, unit: 'MB' });
}

onmessage = async () => {
  try {
    const [tracks, readings] = await Promise.all(
      ['tracks.json', 'readings.json'].map((name) => fetch(`/sample/${name}`).then((r) => r.arrayBuffer()).then((b) => new Uint8Array(b))),
    );
    await sqlite(tracks, readings);
    await files(tracks, readings);
    say({ done: true });
  } catch (error) {
    say({ error: String(error?.stack ?? error) });
  }
};
