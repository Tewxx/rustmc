#!/bin/sh
# Shared paths for the Forge compatibility suite. Source me: . tools/forgetest/env.sh
# Every value can be overridden from the environment.
FT_HERE=$(cd "$(dirname "${BASH_SOURCE:-$0}")" && pwd)
FT_REPO=$(cd "$FT_HERE/../.." && pwd)
# Shared untracked inputs live in the main checkout, not in worktrees.
: "${RUSTMC_MAIN:=$(cd "$(git -C "$FT_REPO" rev-parse --path-format=absolute --git-common-dir)/.." && pwd)}"
: "${PRISM:=/Volumes/SSD/PrismLauncher}"
: "${JDK8:=$RUSTMC_MAIN/.mc-tools/jdk8/Contents/Home}"
[ -x "$JDK8/bin/javac" ] || JDK8="$RUSTMC_MAIN/.mc-tools/jdk8"
: "${MCP:=$RUSTMC_MAIN/MCP-919}"
: "${FORGE_SERVER_DIR:=$RUSTMC_MAIN/.mc-tools/forge-server}"
: "${GOLDEN_DIR:=$RUSTMC_MAIN/golden/forge}"
: "${FT_BUILD:=$FT_REPO/tests/forge/build}"      # gitignored: built jars, oracle runs
: "${FT_PORT:=25599}"
FORGE_VER=1.8.9-11.15.1.2318-1.8.9
FORGE_JAR="$PRISM/libraries/net/minecraftforge/forge/$FORGE_VER/forge-$FORGE_VER-universal.jar"
PL="$PRISM/libraries"
# Libraries Forge 1.8.9 + vanilla 1.8.9 need at run time (from Prism's meta for net.minecraft 1.8.9 + net.minecraftforge 11.15.1.2318 + org.lwjgl 2.9.4-nightly).
RUNTIME_LIBS="
com/mojang/netty/1.8.8/netty-1.8.8.jar
oshi-project/oshi-core/1.1/oshi-core-1.1.jar
net/java/dev/jna/jna/3.4.0/jna-3.4.0.jar
net/java/dev/jna/platform/3.4.0/platform-3.4.0.jar
com/ibm/icu/icu4j-core-mojang/51.2/icu4j-core-mojang-51.2.jar
net/sf/jopt-simple/jopt-simple/4.6/jopt-simple-4.6.jar
com/paulscode/codecjorbis/20101023/codecjorbis-20101023.jar
com/paulscode/codecwav/20101023/codecwav-20101023.jar
com/paulscode/libraryjavasound/20101123/libraryjavasound-20101123.jar
com/paulscode/librarylwjglopenal/20100824/librarylwjglopenal-20100824.jar
com/paulscode/soundsystem/20120107/soundsystem-20120107.jar
io/netty/netty-all/4.0.23.Final/netty-all-4.0.23.Final.jar
com/google/guava/guava/17.0/guava-17.0.jar
org/apache/commons/commons-lang3/3.3.2/commons-lang3-3.3.2.jar
commons-io/commons-io/2.4/commons-io-2.4.jar
commons-codec/commons-codec/1.9/commons-codec-1.9.jar
com/google/code/gson/gson/2.2.4/gson-2.2.4.jar
com/mojang/authlib/1.5.21/authlib-1.5.21.jar
com/mojang/realms/1.7.59/realms-1.7.59.jar
org/apache/commons/commons-compress/1.8.1/commons-compress-1.8.1.jar
org/apache/httpcomponents/httpclient/4.3.3/httpclient-4.3.3.jar
commons-logging/commons-logging/1.1.3/commons-logging-1.1.3.jar
org/apache/httpcomponents/httpcore/4.3.2/httpcore-4.3.2.jar
org/apache/logging/log4j/log4j-api/2.0-beta9-fixed/log4j-api-2.0-beta9-fixed.jar
org/apache/logging/log4j/log4j-core/2.0-beta9-fixed/log4j-core-2.0-beta9-fixed.jar
net/java/jinput/jinput/2.0.5/jinput-2.0.5.jar
net/java/jutils/jutils/1.0.0/jutils-1.0.0.jar
org/lwjgl/lwjgl/lwjgl/2.9.4-nightly-20150209/lwjgl-2.9.4-nightly-20150209.jar
org/lwjgl/lwjgl/lwjgl_util/2.9.4-nightly-20150209/lwjgl_util-2.9.4-nightly-20150209.jar
net/minecraft/launchwrapper/1.12/launchwrapper-1.12.jar
org/ow2/asm/asm-all/5.0.3/asm-all-5.0.3.jar
jline/jline/2.13/jline-2.13.jar
com/typesafe/akka/akka-actor_2.11/2.3.3/akka-actor_2.11-2.3.3.jar
com/typesafe/config/1.2.1/config-1.2.1.jar
org/scala-lang/scala-actors-migration_2.11/1.1.0/scala-actors-migration_2.11-1.1.0.jar
org/scala-lang/scala-compiler/2.11.1/scala-compiler-2.11.1.jar
org/scala-lang/plugins/scala-continuations-library_2.11/1.0.2/scala-continuations-library_2.11-1.0.2.jar
org/scala-lang/plugins/scala-continuations-plugin_2.11.1/1.0.2/scala-continuations-plugin_2.11.1-1.0.2.jar
org/scala-lang/scala-library/2.11.1/scala-library-2.11.1.jar
org/scala-lang/scala-parser-combinators_2.11/1.0.1/scala-parser-combinators_2.11-1.0.1.jar
org/scala-lang/scala-reflect/2.11.1/scala-reflect-2.11.1.jar
org/scala-lang/scala-swing_2.11/1.0.1/scala-swing_2.11-1.0.1.jar
org/scala-lang/scala-xml_2.11/1.0.2/scala-xml_2.11-1.0.2.jar
lzma/lzma/0.0.1/lzma-0.0.1.jar
java3d/vecmath/1.5.2/vecmath-1.5.2.jar
net/sf/trove4j/trove4j/3.0.3/trove4j-3.0.3.jar
"
ft_libs() { # prints ':'-joined absolute library paths
  out=""
  for r in $RUNTIME_LIBS; do out="$out:$PL/$r"; done
  echo "${out#:}"
}
FT_JAVAC="$JDK8/bin/javac"
FT_JAVA="${FT_JAVA:-$JDK8/bin/java}"
