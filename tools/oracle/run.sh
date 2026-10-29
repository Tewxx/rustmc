#!/bin/sh
# Usage: tools/oracle/run.sh <mode> <seed> <x0> <z0> <x1> <z1> <out>
# Runs vanilla 1.8.9 worldgen from the MCP classes (see Oracle.java).
cd "$(dirname "$0")"
# JAVA8_HOME, else the JDK that tools/setup.sh installs, else the old Windows path.
J8="${JAVA8_HOME:-../../.mc-tools/jdk8}"
[ -d "$J8/Contents/Home" ] && J8="$J8/Contents/Home"
[ -d "$J8/bin" ] || J8="/c/Program Files/Eclipse Adoptium/jdk-8.0.472.8-hotspot"
J8="$J8/bin"
SEP=";"; [ "$(uname -s)" = Darwin ] || [ "$(uname -s)" = Linux ] && SEP=":"
CP=$(tr -d '\r\n' < cp.txt | tr '\\;' '/'"$SEP")
[ out/Oracle.class -nt Oracle.java ] || "$J8/javac" -nowarn -cp "$CP" -d out Oracle.java
exec "$J8/java" -cp "out$SEP$CP" Oracle "$@"
