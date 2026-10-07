// The SQLite-on-OPFS prototype's static server: index.html, the worker, the sample review and SQLite's WebAssembly
// build from node_modules, on http://localhost:8790/.
import { join } from 'node:path';

const root = import.meta.dir;
const TYPES: Record<string, string> = {
  '.html': 'text/html',
  '.js': 'text/javascript',
  '.mjs': 'text/javascript',
  '.wasm': 'application/wasm',
  '.json': 'application/json',
};

Bun.serve({
  port: 8790,
  async fetch(request) {
    const asked = new URL(request.url).pathname;
    const path = asked === '/' ? '/index.html' : asked;
    const file = Bun.file(join(root, path));
    if (!(await file.exists())) return new Response('not found', { status: 404 });
    const type = TYPES[path.slice(path.lastIndexOf('.'))] ?? 'application/octet-stream';
    return new Response(file, { headers: { 'Content-Type': type, 'Cache-Control': 'no-store' } });
  },
});
console.log('http://localhost:8790/');
