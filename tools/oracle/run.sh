#!/bin/sh
# Usage: tools/oracle/run.sh <mode> <seed> <x0> <z0> <x1> <z1> <out>
# Runs vanilla 1.8.9 worldgen from the MCP classes (see Oracle.java).
# JDK: $JAVA8_HOME, else .mc-tools/jdk8 (repo or ancestor), else PATH.
cd "$(dirname "$0")" || exit 1
J=""
for h in "$JAVA8_HOME" ../../.mc-tools/jdk8 ../../.mc-tools/jdk8/Contents/Home ../../../.mc-tools/jdk8 ../../../.mc-tools/jdk8/Contents/Home; do
  if [ -n "$h" ] && { [ -x "$h/bin/javac" ] || [ -x "$h/bin/javac.exe" ]; }; then J="$h/bin/"; break; fi
done
# cp.txt may be Windows-style (';' and backslashes); convert for the host.
case "$(uname -s)" in MINGW*|MSYS*|CYGWIN*) SEP=";" ;; *) SEP=":" ;; esac
CP=$(tr '\\' '/' < cp.txt | tr ';' "$SEP" | tr -d '\r\n')
mkdir -p out
[ out/Oracle.class -nt Oracle.java ] || "${J}javac" -nowarn -cp "$CP" -d out Oracle.java || exit 1
exec "${J}java" -cp "out$SEP$CP" Oracle "$@"
