/**
 * Writes the UI's design tokens as TypeScript (`bun run tokens`), for code that needs a token's value rather than the
 * CSS variable: ui/src/app/tokens/tokens.ts, in git. The source stays the stylesheets; this file is their typed copy.
 * In: ui/src/styles.scss compiled by Sass (its :root tokens and the light theme's), ui/src/tailwind.css (Tailwind's
 * groups and the token each reads), ui/src/themes/controls.scss (the shared control classes) and
 * scripts/control-values.ts (their variants). Out:
 *  - TOKENS: every token's value in each theme, var() and derived colors resolved (`oklch(from ...)`, `color-mix`);
 *  - TAILWIND: Tailwind's groups (color, radius, text, font-weight, leading, tracking, spacing, default): each key and
 *    the token it reads;
 *  - CONTROL_CLASSES and CONTROL_VARIANTS: the shared controls and the values their attributes take.
 * Usage: bun scripts/ui-tokens.ts [--check]   (--check: fail when the file is stale; bun run lint:ui runs it)
 */
import { readFileSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { join } from 'node:path';
import { CONTROLS } from './control-values';
import { ROOT } from './local-config';

/** The UI's folder, whose packages (sass, prettier) this uses. */
const UI = join(ROOT, 'ui');
/** The written file. */
const OUT = join(UI, 'src', 'app', 'tokens', 'tokens.ts');
/** The UI's packages, as the UI resolves them. */
const uiRequire = createRequire(join(UI, 'package.json'));
/** Tailwind's groups, longest prefix first, so font-weight is not read as font. */
const TAILWIND_GROUPS = ['font-weight', 'default', 'color', 'radius', 'font', 'text', 'leading', 'tracking'];
/** Decimal places kept in a resolved color's channels. */
const COLOR_PLACES = 4;

/** Token names to their CSS values. */
type Tokens = Record<string, string>;
/** An oklch color: lightness 0 to 1, chroma, hue in degrees, alpha 0 to 1. */
type Oklch = [l: number, c: number, h: number, alpha: number];

/** The declarations of the first rule whose selector is exactly `selector`, at the top level of `css`. */
function block(css: string, selector: string): Tokens {
  const lines = css.split('\n');
  const start = lines.indexOf(`${selector} {`);
  if (start < 0) throw new Error(`no ${selector} rule in the compiled styles`);
  const end = lines.indexOf('}', start);
  const tokens: Tokens = {};
  for (const match of lines.slice(start + 1, end).join('\n').matchAll(/^\s+--([\w-]+):\s*([\s\S]*?);$/gm))
    tokens[match[1]] = match[2].replace(/\s+/g, ' ').trim();
  return tokens;
}

/** A value with every var() replaced by its token's value, all the way down. */
function substitute(value: string, tokens: Tokens, seen: string[] = []): string {
  return value.replace(/var\(--([\w-]+)\)/g, (_, name: string) => {
    if (seen.includes(name)) throw new Error(`token loop: ${[...seen, name].join(' > ')}`);
    if (!(name in tokens)) throw new Error(`unknown token --${name}`);
    return substitute(tokens[name], tokens, [...seen, name]);
  });
}

/** sRGB (0 to 1 each, gamma encoded) from oklch. */
function toSrgb([l, c, h]: Oklch): [number, number, number] {
  const a = c * Math.cos((h * Math.PI) / 180);
  const b = c * Math.sin((h * Math.PI) / 180);
  const lms = [l + 0.3963377774 * a + 0.2158037573 * b, l - 0.1055613458 * a - 0.0638541728 * b, l - 0.0894841775 * a - 1.291485548 * b].map((v) => v ** 3);
  const linear = [
    4.0767416621 * lms[0] - 3.3077115913 * lms[1] + 0.2309699292 * lms[2],
    -1.2684380046 * lms[0] + 2.6097574011 * lms[1] - 0.3413193965 * lms[2],
    -0.0041960863 * lms[0] - 0.7034186147 * lms[1] + 1.707614701 * lms[2],
  ];
  return linear.map((v) => (v <= 0.0031308 ? 12.92 * v : 1.055 * v ** (1 / 2.4) - 0.055)) as [number, number, number];
}

/** oklch from sRGB (0 to 1 each, gamma encoded). */
function fromSrgb(rgb: [number, number, number], alpha: number): Oklch {
  const [r, g, b] = rgb.map((v) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
  const lms = [
    0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b,
    0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b,
    0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b,
  ].map(Math.cbrt);
  const l = 0.2104542553 * lms[0] + 0.793617785 * lms[1] - 0.0040720468 * lms[2];
  const a = 1.9779984951 * lms[0] - 2.428592205 * lms[1] + 0.4505937099 * lms[2];
  const bb = 0.0259040371 * lms[0] + 0.7827717662 * lms[1] - 0.808675766 * lms[2];
  return [l, Math.hypot(a, bb), ((Math.atan2(bb, a) * 180) / Math.PI + 360) % 360, alpha];
}

/** An oklch(L C H [/ A]) literal's channels. */
function parseOklch(text: string): Oklch {
  const match = /^oklch\(\s*([\d.]+)\s+([\d.]+)\s+([\d.]+)\s*(?:\/\s*([\d.]+))?\s*\)$/.exec(text.trim());
  if (!match) throw new Error(`not an oklch color: ${text}`);
  return [Number(match[1]), Number(match[2]), Number(match[3]), match[4] === undefined ? 1 : Number(match[4])];
}

/** An oklch color as CSS, its channels rounded; the alpha only when below 1. */
function formatOklch([l, c, h, alpha]: Oklch): string {
  const round = (v: number) => String(Number(v.toFixed(COLOR_PLACES)));
  const color = c < 1e-4 ? `${round(l)} 0 0` : `${round(l)} ${round(c)} ${round(h)}`;
  return alpha < 1 ? `oklch(${color} / ${round(alpha)})` : `oklch(${color})`;
}

/** A relative oklch color, `oklch(from <oklch> l c h [/ alpha])`, as a plain one. */
function resolveRelative(value: string): string {
  return value.replace(
    /oklch\(from (oklch\([^)]*\)) l c h(?:\s*\/\s*([\d.]+))?\)/g,
    (_, base: string, alpha?: string) => {
      const [l, c, h, own] = parseOklch(base);
      return formatOklch([l, c, h, alpha === undefined ? own : Number(alpha)]);
    },
  );
}

/** `color-mix(in srgb, <a> p%, <b>)` of two opaque oklch colors, as an oklch color. */
function resolveMix(value: string): string {
  return value.replace(
    /color-mix\(in srgb, (oklch\([^)]*\)) ([\d.]+)%, (oklch\([^)]*\))\)/g,
    (_, first: string, percent: string, second: string) => {
      const share = Number(percent) / 100;
      const a = toSrgb(parseOklch(first));
      const b = toSrgb(parseOklch(second));
      return formatOklch(fromSrgb([0, 1, 2].map((i) => a[i] * share + b[i] * (1 - share)) as [number, number, number], 1));
    },
  );
}

/** Every token resolved: var() substituted, then derived colors made plain. */
function resolveAll(tokens: Tokens): Tokens {
  return Object.fromEntries(
    Object.entries(tokens).map(([name, value]) => [name, resolveMix(resolveRelative(substitute(value, tokens)))]),
  );
}

/** Tailwind's groups from tailwind.css's @theme: each group's keys and the token each reads. */
function tailwindGroups(css: string): Record<string, Record<string, string>> {
  const groups: Record<string, Record<string, string>> = { spacing: {} };
  for (const match of css.matchAll(/^\s+--([\w-]+):\s*var\(--([\w-]+)\);/gm)) {
    const [, name, token] = match;
    if (name === 'spacing') groups['spacing']['unit'] = token;
    const group = TAILWIND_GROUPS.find((prefix) => name.startsWith(`${prefix}-`));
    if (group) (groups[group] ??= {})[name.slice(group.length + 1)] = token;
  }
  return groups;
}

/** The shared control classes: controls.scss's top-level class rules. */
function controlClasses(scss: string): string[] {
  return [...new Set([...scss.matchAll(/^ {2}\.([a-z][\w-]*)[\s,{:[]/gm)].map((match) => match[1]))].sort();
}

/** The file's text, before Prettier. */
function source(): string {
  const sass = uiRequire('sass');
  const css: string = sass.compile(join(UI, 'src', 'styles.scss'), { loadPaths: [join(UI, 'src')], style: 'expanded' }).css;
  const dark = block(css, ':root');
  const light = { ...dark, ...block(css, ":root[data-theme=light]") };
  const tokens = { dark: resolveAll(dark), light: resolveAll(light) };
  const tailwind = tailwindGroups(readFileSync(join(UI, 'src', 'tailwind.css'), 'utf8'));
  const classes = controlClasses(readFileSync(join(UI, 'src', 'themes', 'controls.scss'), 'utf8'));
  const json = (value: unknown) => JSON.stringify(value, null, 2);
  return `/**
 * The UI's design tokens as TypeScript, written by \`bun run tokens\` (scripts/ui-tokens.ts) from the stylesheets: do not
 * edit; change the stylesheets and run it again (\`bun run lint:ui\` fails when this is stale). For code that needs a
 * value rather than the CSS variable. In: themes/*.scss, styles.scss, tailwind.css, themes/controls.scss. Out: the
 * constants below.
 */

/** Every token's value in each theme: var() and derived colors resolved. */
export const TOKENS = ${json(tokens)} as const;

/** A token's name (its CSS variable without the leading dashes). */
export type TokenName = keyof typeof TOKENS.dark;

/** Tailwind's groups (tailwind.css): each utility key and the token it reads, as in \`bg-surface-1\` or \`text-xs\`. */
export const TAILWIND = ${json(tailwind)} as const;

/** The shared controls' classes (themes/controls.scss). */
export const CONTROL_CLASSES = ${json(classes)} as const;

/** A shared control's class. */
export type ControlClass = (typeof CONTROL_CLASSES)[number];

/** The attributes the shared controls' styles read, and their values (scripts/control-values.ts checks templates). */
export const CONTROL_VARIANTS = ${json(CONTROLS)} as const;
`;
}

/** The file's text, formatted as the UI formats its code (Prettier and its plugins, run in ui/). */
function formatted(): string {
  const prettier = Bun.spawnSync(['bun', 'run', 'prettier', '--stdin-filepath', 'src/app/tokens/tokens.ts'], {
    cwd: UI,
    stdin: Buffer.from(source()),
  });
  if (prettier.exitCode !== 0) throw new Error(`prettier failed: ${prettier.stderr.toString()}`);
  return prettier.stdout.toString();
}

if (import.meta.main) {
  const text = formatted();
  if (process.argv.includes('--check')) {
    let current = '';
    try {
      current = readFileSync(OUT, 'utf8');
    } catch {
      // no file yet: stale
    }
    if (current.replace(/\r\n/g, '\n') !== text) {
      console.error('ui/src/app/tokens/tokens.ts is stale: run bun run tokens');
      process.exit(1);
    }
    console.log('tokens: ui/src/app/tokens/tokens.ts matches the stylesheets');
  } else {
    writeFileSync(OUT, text);
    console.log(`wrote ${OUT}`);
  }
}
