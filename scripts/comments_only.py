"""Checks that a change touched only comments: for every file changed since a commit (git diff), the code is the same
with its comments left out. A documentation pass runs it to prove it changed no behavior.

Rust, TypeScript, JavaScript and SCSS: the diff's removed and added lines, less whole-line comments (//, ///, //!, a
block comment's lines) and blank lines, must be the same code lines in the same order once a trailing // comment is
cut off each. Python: the files' syntax trees must be equal with docstrings left out (comments are not in the tree).
Other files (Markdown, JSON, HTML) are listed, not checked.

Usage: python scripts/comments_only.py <commit> [path ...]     (exit 1 when a file changed more than comments)
"""
import ast
import re
import subprocess
import sys

COMMENT_LINE = re.compile(r"^\s*(//|/\*|\*|\*/)")
LINE_CHECKED = (".rs", ".ts", ".js", ".mjs", ".scss")
HUNK = re.compile(r"^@@ ")


def git(*args):
    return subprocess.run(["git", *args], capture_output=True, text=True, encoding="utf-8", check=True).stdout


def without_trailing_comment(line):
    """The line with a trailing // comment cut off, when the // is outside a string (an even count of quotes before
    it), and its spaces trimmed."""
    for match in re.finditer(r"//", line):
        before = line[:match.start()]
        if before.count('"') % 2 == 0 and before.count("'") % 2 == 0 and before.count("`") % 2 == 0 \
                and not before.rstrip().endswith(":"):
            return before.strip()
    return line.strip()


def code_lines(lines):
    return [without_trailing_comment(line) for line in lines if line.strip() and not COMMENT_LINE.match(line)]


def line_check(commit, path):
    """None when only comments changed, else the first code line that differs."""
    removed, added = [], []
    for line in git("diff", "-U0", commit, "--", path).splitlines():
        if line.startswith(("---", "+++")) or HUNK.match(line):
            continue
        if line.startswith("-"):
            removed.append(line[1:])
        elif line.startswith("+"):
            added.append(line[1:])
    old, new = code_lines(removed), code_lines(added)
    if old == new:
        return None
    for a, b in zip(old + [""] * len(new), new + [""] * len(old), strict=False):
        if a != b:
            return f"-{a!r} +{b!r}"
    return "code lines differ"


class NoDocstrings(ast.NodeTransformer):
    """A syntax tree with each module's, class's and function's docstring taken out."""

    def strip(self, node):
        body = getattr(node, "body", None)
        if body and isinstance(body[0], ast.Expr) and isinstance(body[0].value, ast.Constant) \
                and isinstance(body[0].value.value, str):
            node.body = body[1:] or [ast.Pass()]
        return self.generic_visit(node)

    visit_Module = visit_ClassDef = visit_FunctionDef = visit_AsyncFunctionDef = strip


def python_check(commit, path):
    old = git("show", f"{commit}:{path}")
    with open(path, encoding="utf-8") as file:
        new = file.read()
    dump = lambda text: ast.dump(NoDocstrings().visit(ast.parse(text)))  # noqa: E731
    return None if dump(old) == dump(new) else "the code differs (docstrings left out)"


def main():
    commit, paths = sys.argv[1], sys.argv[2:]
    changed = git("diff", "--name-only", "--diff-filter=M", commit, "--", *paths).splitlines()
    failed = 0
    for path in changed:
        if path.endswith(".py"):
            problem = python_check(commit, path)
        elif path.endswith(LINE_CHECKED):
            problem = line_check(commit, path)
        else:
            print(f"not checked: {path}")
            continue
        if problem:
            failed += 1
            print(f"CODE CHANGED: {path}: {problem}")
    added = git("diff", "--name-only", "--diff-filter=ADR", commit, "--", *paths).splitlines()
    for path in added:
        failed += 1
        print(f"ADDED, DELETED OR RENAMED: {path}")
    print(f"{len(changed)} files changed, {failed} with more than comments")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
