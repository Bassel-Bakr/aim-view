/**
 * Checks the shared controls' variants (ui/AGENTS.md, themes/controls.scss): a control is a class, and its variant a
 * data attribute or an ARIA role written in the template, with no directive to type it, so this checks the values
 * instead. In: ui/src's templates. Out: each element that breaks a rule, with its file and line, and exit code 1 when
 * there is one. The rules, from CONTROLS: an attribute a control may carry takes one of its values, static
 * (`data-intent="primary"`) or as every string literal in its binding (`[attr.role]="x ? 'alert' : 'status'"`); a
 * required attribute must be there. The same attribute on other elements is theirs (the data table's data-tone).
 * Usage: bun scripts/control-values.ts (bun run lint:ui runs it).
 */
import { Glob } from 'bun';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { ROOT } from './local-config';

/** Where the UI's sources are. */
const SOURCES = join(ROOT, 'ui', 'src');
/** The templates. */
const TEMPLATES = new Glob('**/*.html');
/** An element's start tag: its name and its attributes (a value in double quotes may hold any other character). */
const START_TAG = /<([a-zA-Z][\w-]*)((?:\s+[^\s=>"]+(?:\s*=\s*"[^"]*")?)*)\s*\/?>/g;
/** One attribute: its name, then its value when it has one. */
const ATTRIBUTE = /([^\s=>"]+)(?:\s*=\s*"([^"]*)")?/g;
/** A string literal in a binding's expression. */
const LITERAL = /'([^']*)'/g;

/** One attribute of a control: the values the style knows, and whether the control must have it. */
interface ControlAttribute {
  /** The values its style knows. */
  values: string[];
  /** Whether every element of the control must carry it. */
  required?: boolean;
}

/** Each control's class and the attributes its style reads. */
const CONTROLS: Record<string, Record<string, ControlAttribute>> = {
  // a line saying how an action went: its role says whether it failed
  'status-line': { role: { values: ['status', 'alert'], required: true }, 'data-tone': { values: ['secondary', 'muted'] } },
  // a button: the one main action on a page is primary
  button: { 'data-intent': { values: ['normal', 'primary'] } },
  // a status in words: good is reviewed, done
  badge: { 'data-tone': { values: ['neutral', 'good'] } },
};

/** An element that breaks a rule, where it is. */
interface Finding {
  /** The file, below ui/src. */
  file: string;
  /** The line, from 1. */
  line: number;
  /** What is wrong. */
  problem: string;
}

/** The values an attribute can take: its static value, or every string literal in its binding; null when absent. */
function valuesOf(attributes: Map<string, string>, name: string): string[] | null {
  const plain = attributes.get(name);
  if (plain !== undefined) return [plain];
  const bound = attributes.get(`[attr.${name}]`);
  if (bound === undefined) return null;
  return [...bound.matchAll(LITERAL)].map((match) => match[1]);
}

/** What is wrong with one element's control attributes, from its attributes (none when it is right). */
export function problemsOf(attributes: Map<string, string>): string[] {
  const problems: string[] = [];
  const classes = (attributes.get('class') ?? '').split(/\s+/);
  for (const [control, rules] of Object.entries(CONTROLS)) {
    if (!classes.includes(control)) continue;
    for (const [name, rule] of Object.entries(rules)) {
      const values = valuesOf(attributes, name);
      if (rule.required && !values?.length) problems.push(`a .${control} needs ${name}: ${rule.values.join(' or ')}`);
      for (const value of values ?? [])
        if (!rule.values.includes(value))
          problems.push(`a .${control}'s ${name} is ${rule.values.join(' or ')}, not "${value}"`);
    }
  }
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
  console.log('control values: every role, intent and tone is one the style knows');
}
