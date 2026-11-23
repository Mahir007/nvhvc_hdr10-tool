#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Prints VERSION and SOURCE_DATE_EPOCH lines for $GITHUB_ENV. A release has its bare version, a
# pre-release adds +<short commit>, and a build without a version is 0.0.0-dev+<short commit>.
# SOURCE_DATE_EPOCH is the commit's time.
#
# Usage: version.sh [VERSION]
set -eu
sha=$(git rev-parse --short=7 HEAD)
case ${1:-} in
'') version=0.0.0-dev+$sha ;;
*-*) version=$1+$sha ;;
*) version=$1 ;;
esac
echo "VERSION=$version"
echo "SOURCE_DATE_EPOCH=$(git log -1 --format=%ct)"
