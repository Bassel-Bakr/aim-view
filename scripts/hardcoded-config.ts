/**
 * Checks that a commit adds no hard-coded configuration (AGENTS.md, "No hard-coded configuration"): an absolute
 * folder (`E:\OBS`, `C:/Program Files`) or a local address with a port (`127.0.0.1:8770`, `localhost:4200`) in code.
 * Those belong in aimview.defaults.json or this computer's aimview.json. Only the lines a commit adds are read, so the
 * code that is already there is not judged. Comment lines, docs, retired code and the prototypes are left out; a line
 * that must hold one (a test of the server's address rules) says why with `hardcoded-ok`. In: the staged diff (the
 * pre-commit hook, lefthook.yml). Out: each such line with its file and line, and exit code 1 when there is one.
 * Usage: bun scripts/hardcoded-config.ts
 */

/** The files whose added lines are read: code, not settings files or docs. */
const CODE_FILE = /\.(rs|ts|js|py|scss|html|cmd|ps1|sh)$/;
/** The folders left out: code kept only for the record, and experiments. */
const LEFT_OUT = /(^|\/)(retired|prototypes)\//;
/** What hard-coded configuration looks like: an absolute Windows folder, or a local address with a port. */
const HARD_CODED = [/(?<![\w])[A-Za-z]:[\\/]+[A-Za-z]/, /\b(localhost|127\.0\.0\.1|0\.0\.0\.0)(:\d+)/, /\[::1\]:\d+/];
/** A line that opens with a comment in one of the checked languages. */
const COMMENT = /^\s*(\/\/|#|\*|\/\*|<!--|rem\b)/i;
/** The marker that lets a line through, followed by why. */
const ALLOWED = 'hardcoded-ok';
/** A diff hunk's header, which gives the new file's first line number. */
const HUNK = /^@@ -\d+(?:,\d+)? \+(\d+)/;

/** A line that adds hard-coded configuration, where it is. */
interface Finding {
  /** The file, from the repo's root. */
  file: string;
  /** The line in the new file, from 1. */
  line: number;
  /** The line's text. */
  text: string;
}

/** Whether one added line holds hard-coded configuration. */
export function isHardCoded(text: string): boolean {
  if (COMMENT.test(text) || text.includes(ALLOWED)) return false;
  return HARD_CODED.some((pattern) => pattern.test(text));
}

/** The added lines with hard-coded configuration, from a zero-context diff (`git diff -U0`). */
export function findingsIn(diff: string): Finding[] {
  const out: Finding[] = [];
  let file = '';
  let line = 0;
  for (const text of diff.split('\n')) {
    if (text.startsWith('+++ ')) file = text.slice(6);
    const hunk = HUNK.exec(text);
    if (hunk) line = Number(hunk[1]);
    else if (text.startsWith('+') && !text.startsWith('+++')) {
      const added = text.slice(1);
      if (CODE_FILE.test(file) && !LEFT_OUT.test(file) && isHardCoded(added)) out.push({ file, line, text: added });
      line++;
    }
  }
  return out;
}

if (import.meta.main) {
  const diff = Bun.spawnSync(['git', 'diff', '--cached', '-U0', '--diff-filter=AM', '--no-color']).stdout.toString();
  const findings = findingsIn(diff);
  for (const { file, line, text } of findings)
    console.error(`${file}:${line}: hard-coded configuration (a folder or a local address): ${text.trim()}`);
  if (findings.length) {
    console.error(`Put it in aimview.defaults.json or aimview.json, or end the line with "${ALLOWED}: <why>".`);
    process.exit(1);
  }
}
