#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Daniel Miguel Tejedor
# SPDX-License-Identifier: GPL-3.0-or-later
"""Make Vinilo.app self-contained and ad-hoc signed.

Usage: deploy_dylibs.py <Vinilo.app>

Copies every non-system library the binaries and the image loaders link to
(GTK and its stack, from Homebrew) into Contents/Frameworks, points every
reference at @rpath, and signs the result ad-hoc. The sidecar's Electron is
left alone: it is already a complete, signed app of its own.

Ad-hoc signing is not notarization: Gatekeeper still asks on first open.
"""
from __future__ import annotations

import re
import shutil
import subprocess
import sys
from pathlib import Path

SYSTEM = ("/usr/lib/", "/System/")


def run(*args: str) -> str:
    return subprocess.run(args, check=True, capture_output=True, text=True).stdout


def is_macho(path: Path) -> bool:
    if path.is_symlink() or not path.is_file():
        return False
    with path.open("rb") as handle:
        return handle.read(4) in (b"\xcf\xfa\xed\xfe", b"\xca\xfe\xba\xbe")


def dependencies(path: Path) -> list[str]:
    lines = run("otool", "-L", str(path)).splitlines()[1:]
    return [line.strip().split(" (compatibility")[0] for line in lines if line.strip()]


def rpaths(path: Path) -> list[str]:
    return re.findall(r"cmd LC_RPATH\n\s+cmdsize \d+\n\s+path (.+?) \(offset", run("otool", "-l", str(path)))


def resolve(ref: str, origin: Path, search: list[Path]) -> Path | None:
    """Where a reference points on this machine, if it is ours to copy."""
    if ref.startswith(SYSTEM):
        return None
    name = Path(ref).name
    if ref.startswith("@"):
        candidates = [origin.parent / name] + [d / name for d in search]
        for rp in rpaths(origin):
            base = rp.replace("@loader_path", str(origin.parent))
            candidates.append(Path(base) / name)
    else:
        candidates = [Path(ref)]
    for candidate in candidates:
        if candidate.is_file():
            return candidate.resolve()
    raise SystemExit(f"cannot find {ref} (needed by {origin})")


def main() -> None:
    app = Path(sys.argv[1]).resolve()
    contents = app / "Contents"
    frameworks = contents / "Frameworks"
    frameworks.mkdir(exist_ok=True)
    brew = Path(run("brew", "--prefix").strip())
    search = [brew / "lib"]

    roots = [p for p in (contents / "MacOS").iterdir() if is_macho(p)]
    roots += [p for p in (contents / "Resources" / "lib").rglob("*") if is_macho(p)]
    rpath_for = {p: "@executable_path/../Frameworks" for p in (contents / "MacOS").iterdir()}
    for p in roots:
        if p not in rpath_for:
            rpath_for[p] = "@loader_path/" + "/".join([".."] * (len(p.relative_to(contents).parts) - 1)) + "/Frameworks"

    queue = list(roots)
    copied: dict[str, Path] = {}
    while queue:
        binary = queue.pop()
        for ref in dependencies(binary):
            source = resolve(ref, binary, search)
            if source is None:
                continue
            name = source.name
            target = frameworks / name
            if name not in copied and source != target:
                shutil.copy2(source, target)
                target.chmod(0o755)
                run("install_name_tool", "-id", f"@rpath/{name}", str(target))
                copied[name] = target
                rpath_for[target] = "@loader_path"
                queue.append(target)
            if ref != f"@rpath/{name}":
                run("install_name_tool", "-change", ref, f"@rpath/{name}", str(binary))

    for binary, rpath in rpath_for.items():
        if not is_macho(binary):
            continue
        for old in rpaths(binary):
            if old != rpath:
                run("install_name_tool", "-delete_rpath", old, str(binary))
        if rpath not in rpaths(binary):
            run("install_name_tool", "-add_rpath", rpath, str(binary))

    for binary in [*copied.values(), *roots]:
        run("codesign", "--force", "--sign", "-", str(binary))
    run("codesign", "--force", "--sign", "-", str(app))
    print(f"bundled {len(copied)} libraries into {frameworks}")


if __name__ == "__main__":
    main()
