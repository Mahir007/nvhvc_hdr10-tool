#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Prints VERSION, PROFILE, PROFILE_DIR and SOURCE_DATE_EPOCH lines for $GITHUB_ENV. A release has
# its bare version, a pre-release adds +<short commit>, and a build without a version is
# 0.0.0-dev+<short commit>. Release candidates and releases build with Cargo's release profile,
# everything else with dev, the fast profile. PROFILE_DIR is the profile's folder in target.
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
case $version in
*-rc.*) profile=release dir=release ;;
*-*) profile=dev dir=debug ;;
*) profile=release dir=release ;;
esac
echo "VERSION=$version"
echo "PROFILE=$profile"
echo "PROFILE_DIR=$dir"
echo "SOURCE_DATE_EPOCH=$(git log -1 --format=%ct)"
