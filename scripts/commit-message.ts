/**
 * Checks a commit message's subject against Conventional Commits (AGENTS.md, "Commits"): `type(scope)!: words`, with
 * one of COMMIT_TYPES. scripts/next-version.ts reads these types to version a release, so a misspelled type would
 * silently release nothing. Git's own subjects (a merge, a revert, fixup! and squash!) pass. In: the message file git
 * gives the commit-msg hook (lefthook.yml). Out: exit code 1, with why, when the subject does not follow the rule.
 * Usage: bun scripts/commit-message.ts <message file>
 */
import { readFileSync } from 'node:fs';

/** The commit types: next-version.ts releases feat, fix and perf; the others release nothing. */
const COMMIT_TYPES = ['feat', 'fix', 'perf', 'docs', 'refactor', 'style', 'test', 'build', 'ci', 'chore', 'revert'];
/** A Conventional Commits subject: a type, an optional scope in parentheses, an optional "!", then ": " and words. */
const SUBJECT = new RegExp(`^(${COMMIT_TYPES.join('|')})(\\([\\w./, -]+\\))?!?: \\S`);
/** Subjects git writes itself, which pass as they are. */
const GIT_SUBJECT = /^(Merge |Revert "|fixup! |squash! |amend! )/;

/** Why a commit message's subject breaks the rule, or null when it follows it. Comment lines are git's, not the message's. */
export function problemOf(message: string): string | null {
  const subject = message.split('\n').find((line) => line.trim() !== '' && !line.startsWith('#')) ?? '';
  if (SUBJECT.test(subject) || GIT_SUBJECT.test(subject)) return null;
  return `"${subject}" is not "type(scope): words", with a type among ${COMMIT_TYPES.join(', ')}`;
}

if (import.meta.main) {
  const problem = problemOf(readFileSync(process.argv[2], 'utf8'));
  if (problem) {
    console.error(`commit message: ${problem}`);
    process.exit(1);
  }
}
