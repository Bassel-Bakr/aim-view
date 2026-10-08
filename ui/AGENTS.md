# The UI: rules for agents

The rules for work under `ui/`. The project's other rules are in the root `AGENTS.md`.

- **Angular for speed.** No zone.js. OnPush everywhere (Angular 22's default; never set `Eager`). Prefer signals for
  state; RxJS is fine where it fits better. Data comes through `HttpClient` (it sends with `fetch`), as
  `httpResource()` for reads, so every request passes the interceptors in `app.config.ts`. Tests answer requests with
  `provideHttpClientTesting()` (`fake-api.ts`). Prefer signal forms (`@angular/forms/signals`); the lint warns on the
  older forms. Anything that changes every frame (the video overlay, timelines) is drawn on a canvas in
  `requestAnimationFrame` or `requestVideoFrameCallback`, never through a template. A resource's `value()` throws in its
  error state, so check `error()` or `hasValue()` first.
- **Three modes, one app, one backend.** The UI runs in browser mode (everything in the browser), server mode (the
  review server does the work) or desktop mode (Tauri 2). All three answer the same API with the same review service
  (`service/`). The review server answers over HTTP, the desktop app over its own protocol, and browser mode runs the
  service as WebAssembly (`browser-service/`) in a worker (`modes/service/service.worker.ts`) that an interceptor
  (`modes/service/service-api.ts`) sends the `/api` requests to. The service reads and writes files with plain calls.
  The page's storage only answers asynchronously, so `bun run assets` puts the service's WebAssembly through Binaryen's
  Asyncify (`wasm-opt`), which lets those calls wait in every browser (JSPI would leave out iOS before 27). So every
  mode uses the server mode's services (`modes/http/`). Where the page itself must act, a mode's class extends the
  server's. Browser mode does the VODs folder picker and its videos, reading KovaaK's folders in once, the review, the
  area finder and the cut-off's labels in workers, and link downloads (`modes/service/browser-*.ts`). Desktop mode
  does its folder dialog and mouse logger (`modes/tauri/`).
- **Contracts.** Features and `services/` inject only the contracts in `ui/src/app/platform/` (`RecordingSource`,
  `StatsFiles`, `ReviewEngine`, `ModelCatalog` and the others), never `/api` or a mode's class. The implementations are
  in `ui/src/app/modes/`, in folders named for what they wrap (`http/` the API, `service/` the service in the page,
  `tauri/` the desktop app, `wasm/` the review core and its workers, `web-files/` the page's own files). Each
  `mode.<name>.ts` picks one per contract, and the build configurations (`browser`, `server`, `desktop`) swap
  `modes/mode.ts` for it, so a build carries only its own mode's code. Each contract has one spec that runs against the
  browser and server modes (`platform/*.spec.ts`), both answered by the fake review server (`fake-api.ts`, without the
  browser mode's interceptor).
- **Generated code follows the same style.** A generator renames its files to kebab-case (rewriting their imports) and
  runs Prettier over them, and lint passes on them. Only `ui/generated/` (the copied assets) is in `.prettierignore`.
  Don't add `withFetch()`: fetch is already `HttpClient`'s default, and Angular 22 deprecates it.
- **Named types.** Every object or tuple type gets a name (an interface or a type alias). No inline anonymous types
  such as `{ gpu: number; cpu: number }` in a field or a signature; ESLint enforces it. The types of the JSON that Rust
  structs write are generated (`bun run types`, the `ts` feature, into `ui/src/app/generated/`, kept in git), and
  `api.ts` re-exports them under the UI's names. Only the answers the service builds with `json!`, the bodies the page
  sends and the UI's own types are written by hand.
- **Styles are SCSS, and every design value is a token.** A token is a CSS variable (editable live in the browser) with
  an SCSS name: `$surface-0: var(--surface-0)`. The main tokens are in `ui/src/themes/theme.scss`; tokens only one page
  or module uses are in `ui/src/themes/<page or module>.scss`. Each file has a `tokens` mixin, which `styles.scss`
  includes in `:root`. Component styles `@use 'themes/...'` and use only `$tokens`: no raw colors, sizes, spaces, fonts
  or durations. Keywords and layout values (`flex`, `solid`, `0`, `100%`, `1fr`) are fine. Canvas drawings read their
  colors and fonts from the same CSS variables.
- **Tailwind on the tokens.** `ui/src/tailwind.css` maps Tailwind 4's theme onto the tokens (`@theme inline reference`,
  Tailwind's own scales off), so a class can only reach a token: `bg-surface-1`, `text-muted`, `p-4` (4 x `--space-1`),
  `w-(--sidebar-width)`. No arbitrary values such as `p-[13px]`: `bun run lint:ui` fails on one
  (`scripts/tailwind-values.ts`). Two widths are variants: `max-narrow:` (a phone) and `@max-stack:` (a run page too
  narrow for the flick list beside the video, measured on the main area). Tailwind looks for class names in `ui/src`
  only, so the docs and `ui/retired/` add no CSS. Page-wide element styles are in `@layer base`, named classes in
  `@layer components`, so a utility on an element always wins.
- **Named classes in SCSS, variants as data attributes.** Each part of a component is a class in its own SCSS
  (`.screen { @apply relative overflow-hidden rounded-lg; }`), and the template names it (`class="screen"`). A
  component's SCSS starts with `@reference` to `tailwind.css`, so `@apply` reaches the token theme (Sass compiles
  first, then Tailwind). The shared controls are global classes in `ui/src/themes/controls.scss` (`.button`, `.badge`,
  `.chip`, `.card`, `.pill`, `.segmented`, `.switch`, `.section-note`, `.color-swatch`, `.dialog`, `.tool-panel`,
  `.status-line`, `.fill`). A part that takes the room left in a flex column or a grid row is a `.fill` (it may
  shrink below its content, so what scrolls inside it scrolls there); a dialog never scrolls as a whole, only its
  `.dialog-body .fill`, between its header and its `<footer class="dialog-foot">`. Nothing is sized by the viewport's
  height (`vh`, `dvh`, `h-screen`) except where the window is the parent, on a line marked
  `// viewport-height-ok: <why>`; `bun run lint:ui` fails on the others (`scripts/viewport-heights.ts`). A tooltip is a
  `data-tooltip` attribute, not `title` (which cannot be styled, waits, and never shows for the keyboard or touch):
  one popover in the top layer (`controls/tooltips.ts`, `.tooltip`) shows it for the mouse and the keyboard's focus,
  with no component or directive. `title` stays only on an `<option>`, which the browser's own list shows. A variant is a data attribute (`&[data-intent='primary']`), set by the control's directive in
  `ui/src/app/controls/` from a typed input (`<button appButton intent="primary">`, `<span appBadge tone="good">`),
  so templates get type checks. Where HTML or ARIA already says the state, the style reads it instead, with no
  directive: a `.status-line` binds its role (`[attr.role]="n.failed ? 'alert' : 'status'"`) and is styled by it,
  as a `.chip` is by `aria-pressed`. `bun run lint:ui` checks those values (`scripts/status-lines.ts`), and fails on a
  long `@apply` (5 classes or more) in 3 stylesheets or more: it belongs in one shared class. A variant's
  selector is more specific than the base, so it overrides it with no class merging. A class name must not be a
  Tailwind utility (`table`, `grid`, `hidden`, `table-row`, `table-cell`), or Tailwind adds the utility too. The old
  tailwind-variants modules are in `ui/retired/themes/`.
- **Every table is the data table** (`ui/src/app/data-table/`, `<app-data-table>`). It takes rows and typed columns
  (`DataColumn`) and does TanStack Table's sorting, grouping and column hiding. It follows the Goldman Sachs design
  system's data grid guidance: headers on one line and stuck to the top, numbers to the right, prose columns wide
  enough to read, a Columns menu, the first column stuck to the left, cards on a phone. A column's cells can be your
  own templates (`appCell`, `appHeader`). Its styles are global (`ui/src/themes/data-table.scss`, tokens in
  `table.scss`). No hand-written `<table>`.
- **Angular's style guide** (angular.dev/style-guide, the 2025 one). Folders by feature (`recordings/`, `run/`, each
  part in its own folder). A service more than one feature uses goes in `services/`; a service one feature uses stays
  in that feature. File names follow their class (`stamp-pipe.ts` for `StampPipe`), never generic (`utils.ts`,
  `helpers.ts`). Event handlers are named for what they do (`selectRow`, not `onClick`). Services are `@Service()`
  (Angular 22's), not `@Injectable({ providedIn: 'root' })`. Use `inject()`, `protected` for template-only members, and
  `readonly` for inputs and queries.
- **Format and lint** before calling a change done: `bun run format`, then `bun run lint:ui`.

## The data browser mode starts with

`bun run assets` copies the area finder's examples and types (`test_out/vod_app/area_examples.jsonl`,
`area_kinds.json`) into `ui/generated/data/`. On its first run, the review service in the page copies them into its
own data folder if it has none. From then on the service keeps them, as the review server keeps its own: it learns
into them when areas are saved, and the user can download them or load the review server's files into them. The
examples name the user's recordings, so a build for others must not ship them. `bun run assets` and the `dev*` scripts
copy them (`--no-data` leaves them out). `--release`, which every `build:*` script uses, leaves them out unless
`--data` is given (`bun run build:browser --data`).

The browser runs every model that `python/model/models.json` lists, from its `_u8in` export in
`python/model/exports/`. A new model shows up once it is exported (`python python/model/export.py <best.pt>`, or
`--u8in` for that file only), listed in models.json, and `bun run assets` has run again. `assets` names any listed
model that has no export.
