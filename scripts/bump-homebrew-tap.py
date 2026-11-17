#!/usr/bin/env python3
"""Point the Homebrew tap's cask and CLI formula at a release.

Usage: scripts/bump-homebrew-tap.py <tap-dir> <version> <sums-dir>

<sums-dir> holds the release's .sha256 files for the DMG and the macOS and
Linux CLI archives. Only the version, URLs and sha256 lines change.
"""
import pathlib
import re
import sys

tap, version, sums = pathlib.Path(sys.argv[1]), sys.argv[2], pathlib.Path(sys.argv[3])
if not re.fullmatch(r"\d+\.\d+\.\d+", version):
    sys.exit(f"not a version: {version}")


def sha(asset: str) -> str:
    h = (sums / f"{asset}.sha256").read_text().split()[0].lower()
    if not re.fullmatch(r"[0-9a-f]{64}", h):
        sys.exit(f"bad checksum for {asset}")
    return h


cask = tap / "Casks/agasthy.rb"
s = cask.read_text()
s, n1 = re.subn(r'(?m)^(\s*version ")[^"]+(")', rf"\g<1>{version}\g<2>", s)
s, n2 = re.subn(
    r'(?m)^(\s*sha256 ")[0-9a-f]{64}(")',
    rf"\g<1>{sha(f'AGASTHY-{version}-apple-silicon.dmg')}\g<2>",
    s,
)
if (n1, n2) != (1, 1):
    sys.exit("Casks/agasthy.rb: expected one version and one sha256 line")
cask.write_text(s)

formula = tap / "Formula/agasthy-cli.rb"
s = formula.read_text()
for asset in ("agasthy-cli-macos-aarch64.tar.gz", "agasthy-cli-linux-x86_64.tar.gz"):
    pattern = (
        r'(url "https://github\.com/HMAKT99/Agasthy/releases/download/)v[^/]+(/'
        + re.escape(asset)
        + r'"\s*\n\s*sha256 ")[0-9a-f]{64}(")'
    )
    s, n = re.subn(pattern, rf"\g<1>v{version}\g<2>{sha(asset)}\g<3>", s)
    if n != 1:
        sys.exit(f"Formula/agasthy-cli.rb: expected one url + sha256 for {asset}")
formula.write_text(s)
print(f"tap points at {version}")
