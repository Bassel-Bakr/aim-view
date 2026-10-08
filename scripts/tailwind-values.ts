/**
 * Checks that no Tailwind class takes an arbitrary value (AGENTS.md, ui/AGENTS.md "Tailwind on the
 * tokens"): `p-[13px]`, `bg-[#fff]` or an arbitrary property such as `[mask-type:alpha]` reach past
 * the tokens, and Tailwind compiles them even with its own scales switched off. A token through its
 * variable (`w-(--sidebar-width)`) and arbitrary variants (`data-[intent=primary]:`) are fine. In:
 * ui/src's SCSS (@apply), templates (class="...") and components (host classes). Out: each class
 * that breaks the rule, with its file and line, and exit code 1 when there is one.
 * Usage: bun scripts/tailwind-values.ts (bun run lint:ui runs it).
 */
import { Glob } from 'bun';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { ROOT } from './local-config';

/** Where the UI's sources are. */
const SOURCES = join(ROOT, 'ui', 'src');
/** The files whose classes are checked. */
const FILES = new Glob('**/*.{scss,css,html,ts}');
/** Class lists: an @apply's, a class attribute's, and a host's class. */
const CLASS_LISTS = [/@apply\s+([^;]+);/g, /\bclass="([^"]*)"/g, /\bclass:\s*'([^']*)'/g];

/** A class that breaks the rule, where it is. */
interface Finding {
  /** The file, below ui/src. */
  file: string;
  /** The line, from 1. */
  line: number;
  /** The class as written. */
  name: string;
}

/**
 * A class's utility: what follows its last variant (the last ':' outside brackets and
 * parentheses).
 */
export function utilityOf(name: string): string {
  let depth = 0;
  let start = 0;
  for (let i = 0; i < name.length; i++) {
    const char = name[i];
    if (char === '[' || char === '(') depth++;
    else if (char === ']' || char === ')') depth--;
    else if (char === ':' && depth === 0) start = i + 1;
  }
  return name.slice(start);
}

/** Whether a class takes an arbitrary value or is an arbitrary property: its utility has brackets. */
export function isArbitrary(name: string): boolean {
  return utilityOf(name).includes('[');
}

/** The classes in one file that break the rule. */
export function findingsIn(file: string, text: string): Finding[] {
  const out: Finding[] = [];
  for (const pattern of CLASS_LISTS) {
    for (const match of text.matchAll(pattern)) {
      const line = text.slice(0, match.index).split('\n').length;
      for (const name of match[1].split(/\s+/).filter(isArbitrary)) out.push({ file, line, name });
    }
  }
  return out;
}

if (import.meta.main) {
  const findings: Finding[] = [];
  for (const file of FILES.scanSync(SOURCES)) {
    if (file.includes('generated')) continue;
    findings.push(...findingsIn(file, readFileSync(join(SOURCES, file), 'utf8')));
  }
  for (const { file, line, name } of findings)
    console.error(`ui/src/${file}:${line}: ${name} takes an arbitrary value; use a token`);
  if (findings.length) process.exit(1);
  console.log('tailwind values: every class reaches a token');
}
