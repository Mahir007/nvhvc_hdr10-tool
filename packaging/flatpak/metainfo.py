#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Print the app's metainfo for a build of VERSION.

Fills the template's <releases> from the version tags in HEAD's history, newest first, with
VERSION first when no tag has it. Pre-releases and dev builds are development releases. A
tagged release links to its GitHub release, and the tag's message after its first line becomes
its description: paragraphs, "- " lists, `code`, and links shown as their text. The screenshot
links point at HEAD's commit, and each screenshot must exist there. Run it in a clone with the
tags.

Usage: metainfo.py VERSION
"""

import html
import re
import subprocess
import sys
from pathlib import Path

REPO = "https://github.com/Mahir007/nvhvc_hdr10-tool"
ROOT = Path(__file__).resolve().parents[2]
TEMPLATE = ROOT / "packaging/io.github.Mahir007.NvhvcHdr10Tool.metainfo.xml.in"
VERSION = re.compile(r"^(\d+)\.(\d+)\.(\d+)(?:-(alpha|beta|rc|dev)(?:\.(\d+))?)?(?:\+.*)?$")
STAGES = {"dev": -1, "alpha": 0, "beta": 1, "rc": 2, None: 3}


def git(*args):
    return subprocess.run(
        ["git", *args], cwd=ROOT, check=True, capture_output=True, text=True
    ).stdout


def order(version):
    m = VERSION.match(version)
    return (int(m[1]), int(m[2]), int(m[3]), STAGES[m[4]], int(m[5] or 0))


def summary(tag):
    """The tag's message after its first line, as in the release notes."""
    # A lightweight tag's contents would be the commit's message.
    if git("cat-file", "-t", f"refs/tags/{tag}").strip() != "tag":
        return ""
    contents = git("tag", "-l", "--format=%(contents)", tag)
    signature = git("tag", "-l", "--format=%(contents:signature)", tag)
    if signature and contents.endswith(signature):
        contents = contents[: -len(signature)]
    return "\n".join(contents.splitlines()[1:]).strip()


def inline(text):
    text = html.escape(text, quote=False)
    text = re.sub(r"\[([^]]+)\]\([^)]+\)", r"\1", text)
    return re.sub(r"`([^`]+)`", r"<code>\1</code>", text)


def description(text):
    lines = []
    for block in re.split(r"\n\s*\n", text):
        rows = [row.strip() for row in block.splitlines() if row.strip()]
        if not rows:
            continue
        if rows[0].startswith(("- ", "* ")):
            items = []
            for row in rows:
                if row.startswith(("- ", "* ")):
                    items.append(row[2:])
                else:
                    items[-1] += " " + row
            lines.append("        <ul>")
            lines += [f"          <li>{inline(item)}</li>" for item in items]
            lines.append("        </ul>")
        else:
            lines.append(f"        <p>{inline(' '.join(rows))}</p>")
    return lines


def entry(version, date, tagged):
    kind = "development" if "-" in version.split("+")[0] else "stable"
    head = f'    <release version="{version}" date="{date}" type="{kind}"'
    if not tagged:
        return head + "/>"
    lines = [head + ">"]
    text = summary(f"v{version}")
    if text:
        lines += ["      <description>", *description(text), "      </description>"]
    lines += [f'      <url type="details">{REPO}/releases/tag/v{version}</url>', "    </release>"]
    return "\n".join(lines)


def releases(version):
    found = {}
    for tag in git("tag", "--merged", "HEAD", "--list", "v*").split():
        if VERSION.match(tag[1:]) and "+" not in tag:
            found[tag[1:]] = (git("log", "-1", "--format=%cs", tag).strip(), True)
    if version.split("+")[0] not in found:
        found[version] = (git("log", "-1", "--format=%cs", "HEAD").strip(), False)
    lines = [entry(v, *found[v]) for v in sorted(found, key=order, reverse=True)]
    return "\n".join(["  <releases>", *lines, "  </releases>"])


def main():
    if len(sys.argv) != 2 or not VERSION.match(sys.argv[1]):
        sys.exit(__doc__.strip().splitlines()[-1])
    template = TEMPLATE.read_text(encoding="utf-8")
    for path in re.findall(r"@COMMIT@/([^<]+)", template):
        if not (ROOT / path).is_file():
            sys.exit(f"{TEMPLATE.name} links to {path}, which doesn't exist")
    commit = git("rev-parse", "HEAD").strip()
    metainfo = template.replace("@RELEASES@", releases(sys.argv[1])).replace("@COMMIT@", commit)
    sys.stdout.write(metainfo)


if __name__ == "__main__":
    main()
