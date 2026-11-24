#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Print the app's metainfo for a build of VERSION.

Fills the template's <releases> from the version tags in HEAD's history, newest first, with
VERSION first when no tag has it. Pre-releases and dev builds are development releases. Run it
in a clone with the tags.

Usage: metainfo.py VERSION
"""

import re
import subprocess
import sys
from pathlib import Path

TEMPLATE = Path(__file__).resolve().parents[1] / "io.github.Mahir007.NvhvcHdr10Tool.metainfo.xml.in"
VERSION = re.compile(r"^(\d+)\.(\d+)\.(\d+)(?:-(alpha|beta|rc|dev)(?:\.(\d+))?)?(?:\+.*)?$")
STAGES = {"dev": -1, "alpha": 0, "beta": 1, "rc": 2, None: 3}


def git(*args):
    return subprocess.run(["git", *args], check=True, capture_output=True, text=True).stdout


def order(version):
    m = VERSION.match(version)
    return (int(m[1]), int(m[2]), int(m[3]), STAGES[m[4]], int(m[5] or 0))


def entry(version, date):
    kind = "development" if "-" in version.split("+")[0] else "stable"
    return f'    <release version="{version}" date="{date}" type="{kind}"/>'


def releases(version):
    found = {}
    for tag in git("tag", "--merged", "HEAD", "--list", "v*").split():
        if VERSION.match(tag[1:]) and "+" not in tag:
            found[tag[1:]] = git("log", "-1", "--format=%cs", tag).strip()
    if version.split("+")[0] not in found:
        found[version] = git("log", "-1", "--format=%cs", "HEAD").strip()
    lines = [entry(v, found[v]) for v in sorted(found, key=order, reverse=True)]
    return "\n".join(["  <releases>", *lines, "  </releases>"])


def main():
    if len(sys.argv) != 2 or not VERSION.match(sys.argv[1]):
        sys.exit(__doc__.strip().splitlines()[-1])
    template = TEMPLATE.read_text(encoding="utf-8")
    sys.stdout.write(template.replace("@RELEASES@", releases(sys.argv[1])))


if __name__ == "__main__":
    main()
