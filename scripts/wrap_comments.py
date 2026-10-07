"""Re-flows the Rust comment paragraphs that have a line over the width (120, the repo's hand-formatted limit), in
place.

Usage: python scripts/wrap_comments.py [--width 120] [--containing TEXT] <file.rs> ...

A paragraph is a run of lines with the same indent and marker (//!, /// or //), split at an empty comment line. One
that holds a list item, a code fence or a table is left alone and reported, as are long lines that are not comments.
--containing re-flows only the paragraphs with that text (the ones an edit made long). No line starts with a word that
would read as a list item or a heading (`*`, `-`, `+`, `#`, `>`, `1.`): the word before it moves down with it.
"""
import argparse
import re
from pathlib import Path

# a comment line: its indent, its marker and its text (None when the line holds only the marker)
COMMENT = re.compile(r"^(\s*)(//!|///|//)(?: (.*))?$")
# a comment's text that starts a list item, a code fence or a table row
LIST_OR_CODE = re.compile(r"^(- |\* |\d+\. |```|\|)")
# a word that Markdown would read as a list item or a heading at the start of a line
MARKER_WORD = re.compile(r"^([*+\->#]|\d+\.)$")


def paragraphs(lines):
    """(first, last) line indexes of each comment paragraph."""
    start = None
    for i, line in enumerate(lines + [""]):
        match = COMMENT.match(line)
        same = start is not None and match and COMMENT.match(lines[start]).group(1, 2) == match.group(1, 2)
        if start is not None and (not same or not (match.group(3) or "").strip()):
            yield start, i - 1
            start = None
        if start is None and match and (match.group(3) or "").strip():
            start = i


def reflow(lines, first, last, width):
    """The paragraph from line `first` to `last`, its words laid out again in lines of at most `width` characters with
    its indent and marker. A word that would start a line as a list item or heading takes the word before it along."""
    indent, marker = COMMENT.match(lines[first]).group(1, 2)
    words = " ".join(COMMENT.match(line).group(3) for line in lines[first:last + 1]).split()
    start = f"{indent}{marker}"
    out, current = [], start
    for word in words:
        if len(current) + 1 + len(word) > width and current != start:
            if MARKER_WORD.match(word) and current.count(" ") > start.count(" ") + 1:
                current, carried = current.rsplit(" ", 1)
                out.append(current)
                current = f"{start} {carried}"
            else:
                out.append(current)
                current = start
        current += f" {word}"
    return out + [current]


def main():
    """Re-flows each file's long paragraphs in place, then prints the paragraphs and lines it left over the width."""
    parser = argparse.ArgumentParser(description="Re-flows Rust comment paragraphs that run over the width.")
    parser.add_argument("--width", type=int, default=120)
    parser.add_argument("--containing", default="")
    parser.add_argument("files", nargs="+", type=Path)
    args = parser.parse_args()
    for path in args.files:
        lines = path.read_text(encoding="utf8").split("\n")
        changes = []
        for first, last in paragraphs(lines):
            if all(len(line) <= args.width for line in lines[first:last + 1]):
                continue
            if args.containing not in "\n".join(lines[first:last + 1]):
                continue
            if any(LIST_OR_CODE.match(COMMENT.match(line).group(3)) for line in lines[first:last + 1]):
                print(f"{path}:{first + 1}: a list or code paragraph over the width, left alone")
                continue
            changes.append((first, last, reflow(lines, first, last, args.width)))
        for first, last, new in reversed(changes):
            lines[first:last + 1] = new
        if changes:
            path.write_text("\n".join(lines), encoding="utf8", newline="")
        in_comments = {i for first, last in paragraphs(lines) for i in range(first, last + 1)}
        for i, line in enumerate(lines):
            if len(line) > args.width and i not in in_comments:
                print(f"{path}:{i + 1}: over the width, not a comment paragraph: left alone")
        if changes:
            print(f"{path}: {len(changes)} paragraphs re-flowed")


if __name__ == "__main__":
    main()
