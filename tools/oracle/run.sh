#!/bin/sh
# Usage: tools/oracle/run.sh <mode> <seed> <x0> <z0> <x1> <z1> <out>
# Runs vanilla 1.8.9 worldgen from the MCP classes (see Oracle.java).
cd "$(dirname "$0")"
J8="/c/Program Files/Eclipse Adoptium/jdk-8.0.472.8-hotspot/bin"
CP=$(cat cp.txt)
[ out/Oracle.class -nt Oracle.java ] || "$J8/javac" -nowarn -cp "$CP" -d out Oracle.java
exec "$J8/java" -cp "out;$CP" Oracle "$@"
