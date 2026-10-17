#!/usr/bin/env python3
"""Check that everything `vendor/` carries over upstream Zed is a marked patch.

    scripts/vendor_check.py                       # fetch upstream at the pinned revision
    scripts/vendor_check.py --upstream ~/src/zed  # diff against a checkout already there
    scripts/vendor_check.py --keep                # keep the fetched tree and say where

Why this exists: the root `Cargo.toml` promises that diffing a vendored crate
against upstream at the pinned revision shows the whole of what we carry, with
every change marked `RULOGMAN PATCH` and every patched file named in the patch
list beside the `[patch]` table. Nothing kept that promise but care, and care
had already let a patched file (`gpui_windows/src/events.rs`) fall out of the
list. This is the check that keeps it instead.

What is compared: every crate the `[patch."https://github.com/zed-industries/zed"]`
table points into `vendor/`, against `crates/<name>` of the Zed monorepo at the
`rev` the workspace's `gpui` dependency pins. Both are read from the root
manifest, so a revision bump needs nothing changed here. `vendor/unicode-width`
comes from crates.io rather than from Zed and is not checked.

The rules, per file:

* `Cargo.toml` at a crate's root is skipped: the manifests are flattened on
  purpose (`workspace = true` resolved, examples and dev-dependencies dropped),
  so every line of one differs and none of it is a patch to the code.
  `LICENSE-*` at a crate's root is skipped too: upstream's is a symlink into
  the monorepo root, and the vendored copy is the file it points at.
* A file only upstream has to sit under `examples/`, `docs/`, `tests/` or
  `benches/` — the parts of a crate the flattening drops. A source file that
  went missing is a patch nothing can mark, so it is an error.
* A file only in `vendor/` has to contain the marker somewhere.
* A file in both is diffed (`git diff --no-index -U0`), and changes no more
  than `GAP` unchanged lines apart are taken as one patch — a marker comment
  above a block covers the block even where an untouched line splits the
  diff in two. Each such patch has to have the marker on one of its own
  vendored lines or on the line just above or below it, which is what covers a
  patch that only deletes. A follow-on edit far from its patch — an import, a
  field whose type the patch changed — needs a marker of its own; a short
  `// RULOGMAN PATCH: <which patch>` is enough.
* Every file that differs from upstream, or exists only in `vendor/`, has to be
  named in the root `Cargo.toml` as `<crate>/<path>` — the patch list is where
  a reader learns why each one exists.

Exit status: 0 when every patch is marked and listed, 1 when something is not
(each finding is printed with its file and vendored line), 2 when upstream
could not be fetched or the arguments are wrong.

Requires Python 3.11 (for `tomllib`) and `git`. Fetching takes one shallow,
blob-filtered, sparse clone of the four crate directories — a few megabytes,
not the monorepo — into a temporary directory that is removed afterwards.

CI could run it as one step after checkout, with nothing else installed:
`python3 scripts/vendor_check.py`. It needs network access to github.com.
"""

from __future__ import annotations

import argparse
import filecmp
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
UPSTREAM_URL = "https://github.com/zed-industries/zed"
MARKER = "RULOGMAN PATCH"

# Changes this many unchanged lines apart, or fewer, are one patch.
GAP = 3

# The parts of a crate the vendored copy is allowed to leave out.
DROPPED_DIRS = {"examples", "docs", "tests", "benches"}

HUNK = re.compile(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@")


@dataclass
class Finding:
    """One thing wrong, where it is, and what it is."""

    path: str
    line: int | None
    message: str

    def __str__(self) -> str:
        where = f"{self.path}:{self.line}" if self.line else self.path
        return f"{where}: {self.message}"


def manifest() -> tuple[str, dict[str, Path], str]:
    """The pinned revision, the vendored crates by name, and the manifest text."""
    text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    data = tomllib.loads(text)
    rev = data["workspace"]["dependencies"]["gpui"]["rev"]
    table = data.get("patch", {}).get(UPSTREAM_URL, {})
    crates = {
        name: ROOT / spec["path"]
        for name, spec in table.items()
        if isinstance(spec, dict) and "path" in spec
    }
    if not crates:
        sys.exit(f"no [patch.\"{UPSTREAM_URL}\"] path entries in Cargo.toml")
    return rev, crates, text


def git(*args: str, cwd: Path | None = None) -> str:
    """Runs git, failing loudly, and answers its standard output."""
    result = subprocess.run(
        ["git", *args], cwd=cwd, capture_output=True, text=True, check=False
    )
    if result.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)}: {result.stderr.strip()}")
    return result.stdout


def fetch(rev: str, names: list[str], into: Path) -> Path:
    """A sparse, shallow checkout of `crates/<name>` for each name at `rev`."""
    git("init", "-q", str(into))
    git("remote", "add", "origin", UPSTREAM_URL, cwd=into)
    git("sparse-checkout", "set", *(f"crates/{name}" for name in names), cwd=into)
    git("fetch", "-q", "--depth", "1", "--filter=blob:none", "origin", rev, cwd=into)
    git("checkout", "-q", "FETCH_HEAD", cwd=into)
    return into


def files(root: Path) -> set[str]:
    """Every file under `root`, symlinks included, as POSIX paths relative to it."""
    return {
        path.relative_to(root).as_posix()
        for path in root.rglob("*")
        if path.is_file() or path.is_symlink()
    }


def skipped(rel: str) -> bool:
    """Whether a crate-relative path is outside what this check compares."""
    return rel == "Cargo.toml" or ("/" not in rel and rel.startswith("LICENSE"))


def patches(upstream: Path, vendored: Path) -> list[tuple[int, int]]:
    """The changed spans of `vendored`, as 1-based `[start, end)` line ranges.

    A pure deletion is an empty range at the line it follows. Spans no more
    than `GAP` lines apart are fused into one.
    """
    result = subprocess.run(
        ["git", "diff", "--no-index", "--no-color", "--text", "-U0", "--",
         str(upstream), str(vendored)],
        capture_output=True, text=True, check=False,
    )
    if result.returncode not in (0, 1):
        raise RuntimeError(f"git diff {vendored}: {result.stderr.strip()}")
    spans: list[tuple[int, int]] = []
    for line in result.stdout.splitlines():
        match = HUNK.match(line)
        if not match:
            continue
        start = int(match.group(1))
        length = int(match.group(2)) if match.group(2) is not None else 1
        # `+N,0` names the line a deletion follows, not one it occupies.
        span = (start, start + length) if length else (start + 1, start + 1)
        if spans and span[0] - spans[-1][1] <= GAP:
            spans[-1] = (spans[-1][0], max(spans[-1][1], span[1]))
        else:
            spans.append(span)
    return spans


def check_crate(name: str, upstream: Path, vendored: Path, listing: str) -> tuple[list[Finding], int]:
    """Every finding for one crate, and how many patches it carries."""
    findings: list[Finding] = []
    count = 0
    theirs = {rel for rel in files(upstream) if not skipped(rel)}
    ours = {rel for rel in files(vendored) if not skipped(rel)}

    def listed(rel: str) -> None:
        if f"{name}/{rel}" not in listing:
            findings.append(Finding(f"vendor/{name}/{rel}", None,
                                    f"not named in the patch list in Cargo.toml as `{name}/{rel}`"))

    for rel in sorted(theirs - ours):
        if rel.split("/", 1)[0] not in DROPPED_DIRS:
            findings.append(Finding(f"vendor/{name}/{rel}", None,
                                    "upstream file missing from the vendored crate"))

    for rel in sorted(ours - theirs):
        count += 1
        text = (vendored / rel).read_text(encoding="utf-8", errors="replace")
        if MARKER not in text:
            findings.append(Finding(f"vendor/{name}/{rel}", None,
                                    f"file added over upstream without a `{MARKER}` marker"))
        listed(rel)

    for rel in sorted(ours & theirs):
        a, b = upstream / rel, vendored / rel
        if filecmp.cmp(a, b, shallow=False):
            continue
        lines = b.read_text(encoding="utf-8", errors="replace").splitlines()
        spans = patches(a, b)
        count += len(spans)
        for start, end in spans:
            # One line of slack each side, for a patch that only deletes and
            # for a marker comment sitting directly above a changed line.
            window = lines[max(start - 2, 0):min(end, len(lines))]
            if not any(MARKER in line for line in window):
                first = lines[start - 1].strip() if 0 < start <= len(lines) else "(deletion)"
                findings.append(Finding(f"vendor/{name}/{rel}", start,
                                        f"change without a `{MARKER}` marker: {first[:80]}"))
        listed(rel)
    return findings, count


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("--upstream", type=Path,
                        help="a Zed checkout at the pinned revision, instead of fetching one")
    parser.add_argument("--keep", action="store_true",
                        help="keep the fetched checkout and print its path")
    args = parser.parse_args()

    rev, crates, listing = manifest()
    temp: Path | None = None
    try:
        if args.upstream:
            upstream = args.upstream.expanduser().resolve()
            if not (upstream / "crates").is_dir():
                print(f"{upstream} has no crates/ directory", file=sys.stderr)
                return 2
            try:
                head = git("rev-parse", "HEAD", cwd=upstream).strip()
            except RuntimeError:
                head = None
                print(f"warning: {upstream} is not a git checkout; assuming it is at {rev}",
                      file=sys.stderr)
            if head is not None and head != rev:
                print(f"{upstream} is at {head}, but Cargo.toml pins {rev}", file=sys.stderr)
                return 2
        else:
            temp = Path(tempfile.mkdtemp(prefix="rugpui-vendor-check-"))
            print(f"fetching {UPSTREAM_URL} at {rev[:12]} ...", file=sys.stderr)
            try:
                upstream = fetch(rev, sorted(crates), temp)
            except RuntimeError as error:
                print(error, file=sys.stderr)
                return 2

        findings: list[Finding] = []
        total = 0
        for name, vendored in sorted(crates.items()):
            source = upstream / "crates" / name
            if not source.is_dir():
                findings.append(Finding(f"vendor/{name}", None, f"no crates/{name} upstream"))
                continue
            found, count = check_crate(name, source, vendored, listing)
            findings += found
            total += count
    finally:
        if temp is not None:
            if args.keep:
                print(f"upstream kept at {temp}", file=sys.stderr)
            else:
                shutil.rmtree(temp, ignore_errors=True)

    for finding in findings:
        print(finding)
    sys.stdout.flush()
    names = ", ".join(sorted(crates))
    if findings:
        print(f"\n{len(findings)} problem(s) in {total} patch(es) over {names}", file=sys.stderr)
        return 1
    print(f"ok: {total} patch(es) over {names}, all marked and listed", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
