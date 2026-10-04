#!/bin/sh
# Builds Portier and puts the new binary where the launchd agent runs it, then restarts it.
#
# Usage: contrib/launchd/deploy.sh [agent label] [install folder]
#
# The binary and its data live outside the source tree on purpose. macOS ties the permission
# to read an external volume to the exact binary, so a service run from a build folder on
# such a volume stops at a permission dialog after every rebuild.
set -eu

LABEL="${1:-org.example.portier}"
TARGET="${2:-$HOME/Library/Application Support/Portier}"
cd "$(dirname "$0")/../.."

(cd web && npm run build)
cargo build --release

mkdir -p "$TARGET"
# Copied under another name first: the running program keeps its file until it is replaced.
cp target/release/portier "$TARGET/portier.new"
mv "$TARGET/portier.new" "$TARGET/portier"

launchctl kickstart -k "gui/$(id -u)/$LABEL"
echo "Installed in $TARGET and restarted $LABEL."
