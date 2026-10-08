/**
 * Checks that the UI sizes nothing by the viewport's height (ui/AGENTS.md, "Tailwind on the tokens"): a part takes
 * its height from its parent (a `.fill` in a flex column or a grid row), so it still fits where the viewport is not
 * its parent (a dialog, a panel, a phone with the browser's toolbar). A viewport height (`100vh`, `45dvh`, `h-screen`,
 * `min-h-dvh`) is allowed only on a line that says why with VIEWPORT_OK (on it, or in a comment on the line above):
 * the page's shell, the player fitting the window. In: ui/src's styles and templates. Out: each such line with its file and line, and exit code 1 when there
 * is one.
 * Usage: bun scripts/viewport-heights.ts (bun run lint:ui runs it).
 */
import { Glob } from 'bun';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { ROOT } from './local-config';

/** Where the UI's sources are. */
const SOURCES = join(ROOT, 'ui', 'src');
/** The files checked: styles, templates and components (inline templates, host classes). */
const FILES = new Glob('**/*.{scss,css,html,ts}');
/** A length in a viewport-height unit: vh, vb and their dynamic, small and large forms. */
const VIEWPORT_LENGTH = /(?<![\w-])\d*\.?\d+[dsl]?v[hb]\b/;
/** A Tailwind class sized by the viewport's height: h-screen, min-h-dvh, size-svh... */
const VIEWPORT_CLASS = /(?<![\w-])(?:[\w-]+:)*(?:min-h|max-h|h|size)-(?:screen|dvh|svh|lvh)(?![\w-])/;
/** The marker that lets a line through, followed by why. */
const VIEWPORT_OK = 'viewport-height-ok';

/** A line sized by the viewport's height, where it is. */
interface Finding {
  /** The file, below ui/src. */
  file: string;
  /** The line, from 1. */
  line: number;
  /** The line's text. */
  text: string;
}

/** Whether one line sizes something by the viewport's height without saying why. */
export function isViewportHeight(text: string): boolean {
  if (text.includes(VIEWPORT_OK)) return false;
  return VIEWPORT_LENGTH.test(text) || VIEWPORT_CLASS.test(text);
}

/** A comment line, which is prose, not styles. */
const COMMENT = /^\s*(\/\/|\*|\/\*)/;

/**
 * The lines in one file that size something by the viewport's height. The marker may also be in a comment on the
 * line above, for a line too long to hold it.
 */
export function findingsIn(file: string, text: string): Finding[] {
  const lines = text.split('\n');
  return lines
    .map((line, index) => ({ file, line: index + 1, text: line }))
    .filter(({ text: line, line: number }) => {
      const above = lines[number - 2] ?? '';
      if (COMMENT.test(line) || (COMMENT.test(above) && above.includes(VIEWPORT_OK))) return false;
      return isViewportHeight(line);
    });
}

if (import.meta.main) {
  const findings: Finding[] = [];
  for (const file of FILES.scanSync(SOURCES)) {
    // generated copies: the stylesheets they come from carry the markers (app/tokens/tokens.ts from bun run tokens)
    if (file.includes('generated') || file.replace(/\\/g, '/').startsWith('app/tokens/')) continue;
    findings.push(...findingsIn(file, readFileSync(join(SOURCES, file), 'utf8')));
  }
  for (const { file, line, text } of findings)
    console.error(`ui/src/${file}:${line}: a viewport height (${text.trim()}); fill the parent instead (.fill)`);
  if (findings.length) {
    console.error(`Where the viewport is the parent, end the line with "// ${VIEWPORT_OK}: <why>".`);
    process.exit(1);
  }
  console.log('viewport heights: none outside the allowed places');
}
