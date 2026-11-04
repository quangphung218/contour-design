#!/usr/bin/env python3
"""Writes latest.json, the update manifest of one release, from its files.

  scripts/updater-manifest.py --tag v0.2.0-beta.7 --dir dist-release \\
      --out dist-release/latest.json [--notes release-notes.md] [--repo o/r]

The desktop app's Update now (apps/desktop/src/updater.rs) fetches
`releases/download/<tag>/latest.json` of the release the check found and
installs the build listed for its system, once the build's minisign
signature (its `.sig`, made by the Tauri CLI with TAURI_SIGNING_PRIVATE_KEY)
matches the key in tauri.conf.json.

Which file serves which system (tauri-plugin-updater looks up
`<os>-<arch>`):

  *.app.tar.gz  darwin-aarch64 and darwin-x86_64 (one universal bundle)
  *.AppImage    linux-x86_64 (the .deb updates through its package manager)
  *.msi         windows-x86_64

Each needs `<file>.sig` beside it; a file without one is left out with a
warning (the Windows bundle is best effort). Fails when nothing is signed,
when a system has two candidates, or when a signature was made for another
version (its trusted comment carries `version:`).

Options: --pub-date (RFC 3339, default now), --repo (default
quangphung218/contour-design).
"""

import argparse
import base64
import binascii
import datetime
import json
import re
import sys
import urllib.parse
from pathlib import Path

REPO = "quangphung218/contour-design"
SEMVER = re.compile(r"^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$")
# (file suffix, the systems it serves)
BUNDLES = [
    (".app.tar.gz", ["darwin-aarch64", "darwin-x86_64"]),
    (".AppImage", ["linux-x86_64"]),
    (".msi", ["windows-x86_64"]),
]


class Refused(Exception):
    pass


def warn(message):
    print(f"::warning::{message}")


def signed_version(signature, name):
    """The `version:` of a `.sig`'s trusted comment, or None."""
    try:
        text = base64.b64decode(signature, validate=True).decode("utf-8")
    except (binascii.Error, UnicodeDecodeError) as e:
        raise Refused(f"{name}.sig is not a Tauri signature (base64 of minisign's text): {e}")
    comment = next((line for line in text.splitlines() if line.startswith("trusted comment:")), None)
    if comment is None:
        raise Refused(f"{name}.sig has no trusted comment")
    fields = comment.removeprefix("trusted comment:").strip().split("\t")
    return next((f.removeprefix("version:") for f in fields if f.startswith("version:")), None)


def manifest(tag, files, notes, pub_date, repo=REPO):
    """latest.json for release `tag` from `files` ({name: .sig text or None})."""
    version = tag.removeprefix("v")
    if not SEMVER.match(version):
        raise Refused(f"{tag} is not v<semver>")
    platforms = {}
    for suffix, systems in BUNDLES:
        found = sorted(n for n in files if n.endswith(suffix))
        if len(found) > 1:
            raise Refused(f"more than one *{suffix}: {', '.join(found)}")
        if not found:
            warn(f"no *{suffix}: {', '.join(systems)} left out of latest.json")
            continue
        name = found[0]
        signature = files[name]
        if signature is None:
            warn(f"{name} has no .sig: {', '.join(systems)} left out of latest.json")
            continue
        signature = signature.strip()
        bound = signed_version(signature, name)
        if bound is not None and bound.removeprefix("v") != version:
            raise Refused(f"{name}.sig was made for version {bound}, not {version}")
        url = f"https://github.com/{repo}/releases/download/{urllib.parse.quote(tag)}/{urllib.parse.quote(name)}"
        for system in systems:
            platforms[system] = {"signature": signature, "url": url}
    if not platforms:
        raise Refused("no signed updater bundle: latest.json would list nothing")
    return {"version": version, "notes": notes, "pub_date": pub_date, "platforms": platforms}


def read_dir(path):
    """{name: .sig text or None} for the files of `path` that are not .sig."""
    names = {p.name for p in path.iterdir() if p.is_file()}
    return {
        n: (path / f"{n}.sig").read_text(encoding="utf-8") if f"{n}.sig" in names else None
        for n in names
        if not n.endswith(".sig")
    }


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--tag", required=True)
    ap.add_argument("--dir", required=True, type=Path)
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--notes", type=Path)
    ap.add_argument("--repo", default=REPO)
    ap.add_argument("--pub-date")
    args = ap.parse_args(argv)
    pub_date = args.pub_date or datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    notes = args.notes.read_text(encoding="utf-8").strip() if args.notes else ""
    try:
        out = manifest(args.tag, read_dir(args.dir), notes, pub_date, args.repo)
    except (Refused, OSError) as e:
        print(f"::error::updater-manifest: {e}")
        return 1
    args.out.write_text(json.dumps(out, indent=2) + "\n", encoding="utf-8")
    print(f"latest.json: {out['version']} for {', '.join(sorted(out['platforms']))}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
