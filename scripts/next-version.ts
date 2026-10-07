/**
 * The next release's version from the Conventional Commits since the last release tag (vX.Y.Z),
 * as semver: a breaking change (`type!:`, or a BREAKING CHANGE footer) bumps the major version
 * (the minor while it is 0), `feat` the minor, `fix` and `perf` the patch; the other types (docs,
 * chore, refactor, test, style, ci, build) make no release. With no tag yet the count starts from
 * 0.0.0. Prints the version, or nothing when there is nothing to release (`--force` makes that a
 * patch); with `--notes`, the release's notes in Markdown instead: its commits by kind. The
 * release workflow (.github/workflows/release.yml) runs it. In: git's tags and log.
 * Usage: bun scripts/next-version.ts [--force] [--notes]
 */
import { $ } from 'bun';

/** A commit as the notes and the bump read it. */
interface Commit {
  /** Its Conventional Commits type (feat, fix...); "other" when the subject has none. */
  kind: string;
  /** Its subject line, as written. */
  subject: string;
  /** Whether it is a breaking change. */
  breaking: boolean;
}

/** How much a commit bumps the version: none, the patch, the minor, the major. */
enum Bump {
  /** No release. */
  None,
  /** X.Y.Z+1. */
  Patch,
  /** X.Y+1.0. */
  Minor,
  /** X+1.0.0. */
  Major,
}

/** A Conventional Commits subject: its type, scope, "!" for a breaking change, and description. */
const HEADER = /^(\w+)(\([^)]*\))?(!)?:\s*(.+)$/;
/** The notes' sections after Breaking changes: each commit type and its heading, in order. */
const SECTIONS: [string, string][] = [
  ['feat', 'Features'],
  ['fix', 'Fixes'],
  ['perf', 'Performance'],
];

/** The command line's flags. */
const args = process.argv.slice(2);
/** The release tags' pattern. */
const pattern = 'v*.*.*';
/** The release tags, newest version first. */
const tags = (await $`git tag --list ${pattern} --sort=-v:refname`.text()).split('\n').filter(Boolean);
/** The last release's tag; undefined before the first release. */
const last = tags[0];
/** The commits to read: since the last release, or all of them. */
const range = last ? `${last}..HEAD` : 'HEAD';
/** Each commit's subject and body, ended by a NUL byte. */
const log = await $`git log ${range} --format=%s%n%b%x00`.text();
/** The commits since the last release, newest first. */
const commits: Commit[] = log
  .split('\0')
  .map((text) => text.trim())
  .filter(Boolean)
  .map((text) => {
    const [subject, ...body] = text.split('\n');
    const header = HEADER.exec(subject);
    const breaking = Boolean(header?.[3]) || body.some((line) => line.startsWith('BREAKING CHANGE'));
    return { kind: header?.[1] ?? 'other', subject, breaking };
  });

if (args.includes('--notes')) {
  const lines: string[] = [];
  const breaking = commits.filter((commit) => commit.breaking);
  if (breaking.length) lines.push('## Breaking changes', ...breaking.map((commit) => `- ${commit.subject}`), '');
  for (const [kind, title] of SECTIONS) {
    const of = commits.filter((commit) => commit.kind === kind);
    if (of.length) lines.push(`## ${title}`, ...of.map((commit) => `- ${commit.subject}`), '');
  }
  console.log(lines.join('\n') || 'Builds of the same code as the last release.');
} else {
  const bumpOf = (commit: Commit): Bump =>
    commit.breaking
      ? Bump.Major
      : commit.kind === 'feat'
        ? Bump.Minor
        : commit.kind === 'fix' || commit.kind === 'perf'
          ? Bump.Patch
          : Bump.None;
  let bump = commits.reduce((most, commit) => Math.max(most, bumpOf(commit)), Bump.None);
  if (bump === Bump.None && args.includes('--force')) bump = Bump.Patch;
  const [major, minor, patch] = last ? last.slice(1).split('.').map(Number) : [0, 0, 0];
  // before 1.0 a breaking change is a minor bump: 1.0 is a decision, not a commit
  if (bump === Bump.Major && major === 0) bump = Bump.Minor;
  const next =
    bump === Bump.Major
      ? `${major + 1}.0.0`
      : bump === Bump.Minor
        ? `${major}.${minor + 1}.0`
        : bump === Bump.Patch
          ? `${major}.${minor}.${patch + 1}`
          : '';
  if (next) console.log(next);
}
