#!/usr/bin/env python3
"""Fail when a Markdown file links to a local file that does not exist.

Only relative links to files are checked. External URLs (anything with a scheme, such as
https: or mailto:), protocol-relative links and pure `#anchor` links are skipped, and so is the
`#fragment` / `?query` part of a link to a file, so the check never touches the network and does
not verify that an anchor exists. Inline code is ignored. Fenced code blocks are scanned like
the rest of the text, because many fences in this repository are not balanced and a state machine
over them would hide real links; to show an example link in a code block, put
`link-check: ignore` on the same line. A link starting with `/` is resolved from the repository
root; any other link from the directory of the file that contains it.

Usage: scripts/check-doc-links.py [PATH ...]
PATH is a Markdown file or a directory searched recursively; the default is the repository's
tracked Markdown files.
"""
import re
import subprocess
import sys
from pathlib import Path
from urllib.parse import unquote

ROOT = Path(__file__).resolve().parent.parent

# [text](target), [text](<target with spaces>), and optional "title". Images match the same way.
INLINE_LINK = re.compile(r"\[[^\]]*\]\(\s*(?:<([^>]*)>|([^)\s]*))(?:\s+[\"'][^)]*[\"'])?\s*\)")
# [label]: target
REFERENCE_LINK = re.compile(r"^\s{0,3}\[[^\]]+\]:\s*(?:<([^>]*)>|(\S+))", re.MULTILINE)
SCHEME = re.compile(r"^[A-Za-z][A-Za-z0-9+.-]*:")
INLINE_CODE = re.compile(r"`+[^`\n]*`+")


def markdown_files(args):
    if not args:
        out = subprocess.run(
            ["git", "ls-files", "*.md"], cwd=ROOT, capture_output=True, text=True, check=True
        ).stdout
        return [ROOT / line for line in out.splitlines() if line]
    files = []
    for arg in args:
        path = Path(arg).resolve()
        files.extend(sorted(path.rglob("*.md")) if path.is_dir() else [path])
    return files


def strip_code(text):
    """Blank out inline code and lines marked `link-check: ignore`, keeping line numbers."""
    return "\n".join(
        "" if "link-check: ignore" in line else INLINE_CODE.sub("", line)
        for line in text.splitlines()
    )


def targets(text):
    for line_no, line in enumerate(text.splitlines(), 1):
        for match in INLINE_LINK.finditer(line):
            yield line_no, match.group(1) or match.group(2)
    for match in REFERENCE_LINK.finditer(text):
        yield text.count("\n", 0, match.start()) + 1, match.group(1) or match.group(2)


def check(path):
    problems = []
    text = strip_code(path.read_text(encoding="utf-8"))
    for line_no, target in targets(text):
        if not target or target.startswith(("#", "//")) or SCHEME.match(target):
            continue
        file_part = unquote(re.split(r"[#?]", target, maxsplit=1)[0])
        if not file_part:
            continue
        base = ROOT if file_part.startswith("/") else path.parent
        resolved = (base / file_part.lstrip("/")).resolve()
        try:
            resolved.relative_to(ROOT)
        except ValueError:
            problems.append((line_no, target, "points outside the repository"))
            continue
        if not resolved.exists():
            problems.append((line_no, target, "does not exist"))
    return problems


def main(argv):
    failed = 0
    files = markdown_files(argv)
    for path in files:
        for line_no, target, reason in check(path):
            failed += 1
            print(f"{path.relative_to(ROOT)}:{line_no}: link '{target}' {reason}", file=sys.stderr)
    if failed:
        print(f"{failed} broken local link(s) in {len(files)} Markdown file(s).", file=sys.stderr)
        return 1
    print(f"No broken local links in {len(files)} Markdown file(s).")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
