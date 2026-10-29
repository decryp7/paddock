#!/bin/bash
# Usage: disfn.sh <binary> '<demangled function name>' > out.s   (then diff the output of two builds)
# Disassembles one function of an unstripped binary by its address range (llvm-nm -C, llvm-objdump from the toolchain's
# llvm-tools), with addresses replaced by ADDR so two builds compare (docs/research/2026-10-29-link-time.md).
bin=$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/^host: //p')/bin
read -r start stop < <("$bin/llvm-nm" -n -C --defined-only "$1" | awk -v name="$2" '{a=$1; $1="";$2=""; sub(/^  /,""); if (found) {print s, a; exit} if ($0==name) {found=1; s=a}}')
[ -n "${start:-}" ] || { echo "not found: $2" >&2; exit 1; }
"$bin/llvm-objdump" -d -C --no-show-raw-insn --no-leading-addr --start-address=0x"$start" --stop-address=0x"$stop" "$1" | tail -n +5 \
  | sed -E 's/0x[0-9a-f]+/ADDR/g'
