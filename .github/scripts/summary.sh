#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Prints a Markdown table of archives with their size and SHA-256, for $GITHUB_STEP_SUMMARY.
#
# Usage: summary.sh FILE...
set -eu
echo '| Archive | Size | SHA-256 |'
echo '| --- | --- | --- |'
for file; do
    size=$(wc -c < "$file" | awk '{ split("B KiB MiB GiB", u); s = $1; i = 1
        while (s >= 1024 && i < 4) { s /= 1024; i++ }
        printf(i == 1 ? "%d %s" : "%.1f %s", s, u[i]) }')
    hash=$(sha256sum "$file" | cut -d' ' -f1)
    echo "| $(basename "$file") | $size | \`$hash\` |"
done
