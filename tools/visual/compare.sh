#!/bin/bash
# Re-render rustmc scenes and print the diff table (vanilla shots must exist; see vanilla.sh).
cd "$(dirname "$0")/../.."
tools/visual/run_rustmc.sh "$@" > /dev/null
tools/visual/diff.py "$2"
