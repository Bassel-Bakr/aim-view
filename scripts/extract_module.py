"""Copies chosen top-level definitions of a Python module, and everything they use from it, into a new module, verbatim:
the way to retire a big module while its users keep only the parts they need.

Usage: python scripts/extract_module.py <source.py> <out.py> <name> [<name> ...] [--doc "the new module's docstring"]

<name> is a top-level function, class or variable of the source. The new module gets the source's imports and its other
top-level setup statements (in the source's order; Ruff then drops the unused imports), then every needed definition in
the source's order, each with the comments just above it. It then checks that each copied definition parses to the same
tree as the source's, and lists the definitions it left out.
"""
import argparse
import ast
import subprocess
import sys
from pathlib import Path


def defined_names(node):
    """The names a top-level statement defines."""
    if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
        return [node.name]
    targets = node.targets if isinstance(node, ast.Assign) else [node.target] if isinstance(node, ast.AnnAssign) else []
    names = []
    for target in targets:
        elements = target.elts if isinstance(target, ast.Tuple) else [target]
        names += [element.id for element in elements if isinstance(element, ast.Name)]
    return names


def is_main_block(node):
    """Whether a top-level statement is the `if __name__ == "__main__":` block, which the new module leaves out."""
    return isinstance(node, ast.If) and "__main__" in ast.unparse(node.test)


def source_lines(lines, node):
    """A statement's source lines, from its decorators and the comments just above it to its end."""
    first = min([node.lineno] + [decorator.lineno for decorator in getattr(node, "decorator_list", [])]) - 1
    while first > 0 and lines[first - 1].lstrip().startswith("#"):
        first -= 1
    return lines[first:node.end_lineno]


def needed(definitions, wanted):
    """The wanted definitions and every top-level definition they use, by name."""
    found, waiting = set(), list(wanted)
    while waiting:
        name = waiting.pop()
        if name in found:
            continue
        found.add(name)
        used = {node.id for node in ast.walk(definitions[name]) if isinstance(node, ast.Name)}
        waiting += [other for other in used if other in definitions and other not in found]
    return found


def main():
    """Writes the new module, drops its unused imports with Ruff, and stops with an error when a name is not defined
    at the source's top level or a copied definition does not parse to the source's tree."""
    parser = argparse.ArgumentParser(description="Copies chosen definitions of a module into a new one, verbatim.")
    parser.add_argument("source", type=Path)
    parser.add_argument("out", type=Path)
    parser.add_argument("names", nargs="+")
    parser.add_argument("--doc", default="")
    args = parser.parse_args()
    text = args.source.read_text(encoding="utf8")
    lines, tree = text.splitlines(), ast.parse(text)
    definitions = {name: node for node in tree.body for name in defined_names(node)}
    unknown = [name for name in args.names if name not in definitions]
    if unknown:
        sys.exit(f"not defined at the top of {args.source}: {', '.join(unknown)}")
    keep = needed(definitions, args.names)
    setup, copied = [], []
    for node in tree.body:
        names = defined_names(node)
        if names and any(name in keep for name in names):
            copied.append(node)
        elif not names and not is_main_block(node) and not (isinstance(node, ast.Expr) and node is tree.body[0]):
            setup.append(node)
    parts = [f'"""{args.doc}"""'] if args.doc else []
    parts.append("\n".join(line for node in setup for line in source_lines(lines, node)))
    parts += ["\n".join(source_lines(lines, node)) for node in copied]
    args.out.write_text("\n\n\n".join(parts) + "\n", encoding="utf8")
    subprocess.run([sys.executable, "-m", "ruff", "check", "--select", "F401", "--fix", "--quiet", str(args.out)])
    new = {name: node for node in ast.parse(args.out.read_text(encoding="utf8")).body for name in defined_names(node)}
    differ = [name for name in sorted(keep) if ast.dump(new.get(name, ast.Pass())) != ast.dump(definitions[name])]
    left_out = sorted(set(definitions) - keep)
    print(f"{len(keep)} of {len(definitions)} definitions copied into {args.out}; setup statements: {len(setup)}")
    print(f"left out: {', '.join(left_out) or 'none'}")
    if differ:
        sys.exit(f"copied but not the same tree: {', '.join(differ)}")
    print("every copied definition parses to the same tree as the source's")


if __name__ == "__main__":
    main()
