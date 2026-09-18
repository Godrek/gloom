#!/bin/sh
# A small real build for capture to observe. It generates part of its own
# sources, compiles every translation unit from a build directory so that
# working directories matter, and links two executables whose membership
# differs. Nothing here knows it is being captured: the compiler is invoked as
# `clang` on PATH, which is where capture wraps it.
set -eu

mkdir -p build/generated

cat > build/generated/config.h <<'HEADER'
#define CAPTURE_FEATURE 1
HEADER

cat > build/generated/worker.c <<'SOURCE'
void helper(void);

void worker(void) { helper(); }
SOURCE

cd build

clang -c -g -O0 -DCAPTURED_BUILD=1 -Igenerated ../main.c -o main.o
clang -c -g -O0 -DCAPTURED_BUILD=1 -Igenerated generated/worker.c -o worker.o
clang -c -g -O0 -DCAPTURED_BUILD=1 -Igenerated ../support.c -o support.o
clang -c -g -O0 -DCAPTURED_BUILD=1 -Igenerated ../other.c -o other.o

clang main.o worker.o support.o -o server
clang other.o -o other-tool
