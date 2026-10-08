/**
 * Checks the UI's status lines (ui/AGENTS.md, themes/controls.scss `.status-line`): a line saying how an action went
 * says whether it failed through its ARIA role, which both screen readers and the style read, so the role's values
 * must be ones the style knows. In: ui/src's templates. Out: each element that breaks a rule, with its file and line,
 * and exit code 1 when there is one. The rules:
 *  - a `.status-line` has a role, static (`role="alert"`) or bound (`[attr.role]="x ? 'alert' : 'status'"`), and each
 *    role it can take is one of STATUS_ROLES;
 *  - a `.status-line`'s `data-tone`, when it has one, is one of STATUS_TONES (other elements' tones are their own).
 * Usage: bun scripts/status-lines.ts (bun run lint:ui runs it).
 */
import { Glob } from 'bun';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { ROOT } from './local-config';

/** Where the UI's sources are. */
const SOURCES = join(ROOT, 'ui', 'src');
/** The templates. */
const TEMPLATES = new Glob('**/*.html');
/** The roles a status line can take: it went well, or it failed. */
const STATUS_ROLES = new Set(['status', 'alert']);
/** The tones a status line can take while nothing failed. */
const STATUS_TONES = new Set(['secondary', 'muted']);
/** An element's start tag: its name and its attributes (a value in double quotes may hold any other character). */
const START_TAG = /<([a-zA-Z][\w-]*)((?:\s+[^\s=>"]+(?:\s*=\s*"[^"]*")?)*)\s*\/?>/g;
/** One attribute: its name, then its value when it has one. */
const ATTRIBUTE = /([^\s=>"]+)(?:\s*=\s*"([^"]*)")?/g;
/** A string literal in a binding's expression. */
const LITERAL = /'([^']*)'/g;

/** An element that breaks a rule, where it is. */
interface Finding {
  /** The file, below ui/src. */
  file: string;
  /** The line, from 1. */
  line: number;
  /** What is wrong. */
  problem: string;
}

/** The values an attribute can take: its static value, or every string literal in its binding. */
function valuesOf(attributes: Map<string, string>, name: string): string[] | null {
  const plain = attributes.get(name);
  if (plain !== undefined) return [plain];
  const bound = attributes.get(`[attr.${name}]`);
  if (bound === undefined) return null;
  return [...bound.matchAll(LITERAL)].map((match) => match[1]);
}

/** What is wrong with one element's status-line attributes, from its attributes (none when it is right). */
export function problemsOf(attributes: Map<string, string>): string[] {
  const problems: string[] = [];
  const isStatusLine = (attributes.get('class') ?? '').split(/\s+/).includes('status-line');
  const roles = valuesOf(attributes, 'role');
  if (isStatusLine && (roles === null || roles.length === 0))
    problems.push('a .status-line needs a role: status, or alert once it failed');
  for (const role of isStatusLine ? (roles ?? []) : [])
    if (!STATUS_ROLES.has(role)) problems.push(`a .status-line's role is status or alert, not "${role}"`);
  const tones = isStatusLine ? valuesOf(attributes, 'data-tone') : null;
  for (const tone of tones ?? [])
    if (!STATUS_TONES.has(tone)) problems.push(`a status line's tone is secondary or muted, not "${tone}"`);
  return problems;
}

/** The elements in one template that break a rule. */
export function findingsIn(file: string, text: string): Finding[] {
  const out: Finding[] = [];
  for (const tag of text.matchAll(START_TAG)) {
    const attributes = new Map<string, string>();
    for (const attribute of tag[2].matchAll(ATTRIBUTE)) attributes.set(attribute[1], attribute[2] ?? '');
    const line = text.slice(0, tag.index).split('\n').length;
    for (const problem of problemsOf(attributes)) out.push({ file, line, problem });
  }
  return out;
}

if (import.meta.main) {
  const findings: Finding[] = [];
  for (const file of TEMPLATES.scanSync(SOURCES))
    findings.push(...findingsIn(file, readFileSync(join(SOURCES, file), 'utf8')));
  for (const { file, line, problem } of findings) console.error(`ui/src/${file}:${line}: ${problem}`);
  if (findings.length) process.exit(1);
  console.log('status lines: every role and tone is one the style knows');
}
