#!/bin/sh
# The definition of "may land". Until the cargo workspace exists there is
# nothing to check; the first ticket replaces this with the full gate.
set -e
test -f Cargo.toml || { echo "no cargo workspace yet: nothing to check"; exit 0; }
echo "gate.sh has not been written for the workspace yet" >&2
exit 1
