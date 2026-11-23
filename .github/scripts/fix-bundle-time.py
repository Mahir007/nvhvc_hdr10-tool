#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Set a Flatpak bundle's creation time to its commit's time, so the same build gives the same file.

`flatpak build-bundle` stores the time it ran. The bundle is one GVariant (an OSTree static delta
superblock), and the commit inside it already has a fixed time from `build-export --timestamp`.
Needs PyGObject.

Usage: fix-bundle-time.py BUNDLE
"""

import sys

import gi

gi.require_version("GLib", "2.0")
from gi.repository import GLib

# OSTREE_STATIC_DELTA_SUPERBLOCK_FORMAT: metadata, timestamp, from, to, commit, parts, fallbacks.
SUPERBLOCK = GLib.VariantType.new("(a{sv}tayay(a{sv}aya(say)sstayay)aya(uayttay)a(yaytt))")
TIMESTAMP, COMMIT = 1, 4
COMMIT_TIMESTAMP = 5


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__.strip().splitlines()[-1], file=sys.stderr)
        return 1
    path = sys.argv[1]
    with open(path, "rb") as f:
        data = f.read()
    bundle = GLib.Variant.new_from_bytes(SUPERBLOCK, GLib.Bytes.new(data), False)
    if not bundle.is_normal_form() or bundle.get_data_as_bytes().get_data() != data:
        print(f"{path}: not a Flatpak bundle", file=sys.stderr)
        return 1
    children = [bundle.get_child_value(i) for i in range(bundle.n_children())]
    # Both times are stored big-endian, so the commit's can be copied as it is.
    commit_time = children[COMMIT].get_child_value(COMMIT_TIMESTAMP).get_uint64()
    children[TIMESTAMP] = GLib.Variant.new_uint64(commit_time)
    fixed = GLib.Variant.new_tuple(*children)
    with open(path, "wb") as f:
        f.write(fixed.get_data_as_bytes().get_data())
    return 0


if __name__ == "__main__":
    sys.exit(main())
