#!/usr/bin/env bash
# The telemetry check of ADR 0001 decision 4 (P5): fails when a built ONNX Runtime library, or a binary that links it,
# carries a way to send data to Microsoft. It reads bytes only (no disassembly, no tools beyond grep), so it runs the same
# on the Mac, the Linux image and Git Bash on the Windows runner.
#
#   apps/sighter/scripts/ort/check-telemetry.sh [--allow-telemetry-group] <file>...
#
# Fails (exit 1) on any of:
#   - the 1DS SDK (cpp_client_telemetry): its namespace, plain or mangled (Itanium and MSVC);
#   - curl or mbedTLS, which ONNX Runtime 1.30+ links only for 1DS uploads;
#   - a Microsoft telemetry collector host name;
#   - a TraceLogging provider in the Microsoft telemetry provider group (the group GUID's bytes, as stored in a provider's
#     traits). ONNX Runtime's public sources define that option as an all-zero group (TraceLoggingConfig.h), so our own
#     build must not contain it; DirectML.dll does, which is why the DirectML gate (P6) decides about that file, and why
#     checking it here needs --allow-telemetry-group to report instead of fail.
# Reports, without failing, the ETW provider names a Windows build of ONNX Runtime always compiles in (windows/telemetry.cc
# and logging/etw_sink.cc are built whatever the telemetry flag; with --no_telemetry the session uses the no-op provider).
# Exit 2 when a file is missing.
set -euo pipefail

allow_group=0
if [ "${1:-}" = "--allow-telemetry-group" ]; then allow_group=1; shift; fi
[ $# -gt 0 ] || { echo "usage: check-telemetry.sh [--allow-telemetry-group] <file>..." >&2; exit 2; }

# Fixed strings that must never be present.
forbidden=(
  "Microsoft::Applications::Events"
  "9Microsoft12Applications6Events"
  "@Events@Applications@Microsoft@@"
  "curl_easy_perform"
  "mbedtls_ssl_handshake"
  "events.data.microsoft.com"
)
# Reported only.
noted=(
  "Microsoft.ML.ONNXRuntime"
  "ONNXRuntimeTraceLoggingProvider"
  "Microsoft.Windows.AI.MachineLearning.Dml"
)
# {4f50731a-89cf-4782-b3e0-dce8c90476ba}, the Microsoft telemetry provider group, in its in-memory byte order.
group_guid=$'\x1a\x73\x50\x4f\xcf\x89\x82\x47\xb3\xe0\xdc\xe8\xc9\x04\x76\xba'

count() { LC_ALL=C grep -a -c -F -e "$1" "$2" || true; }

status=0
for f in "$@"; do
  if [ ! -f "$f" ]; then echo "check-telemetry: $f: missing" >&2; exit 2; fi
  bad=()
  for s in "${forbidden[@]}"; do
    [ "$(count "$s" "$f")" = 0 ] || bad+=("$s")
  done
  group=$(count "$group_guid" "$f")
  for s in "${noted[@]}"; do
    [ "$(count "$s" "$f")" = 0 ] || echo "check-telemetry: $f: note: contains ETW provider name '$s'"
  done
  if [ "$group" != 0 ]; then
    if [ "$allow_group" = 1 ]; then
      echo "check-telemetry: $f: note: a TraceLogging provider in the Microsoft telemetry group (the DirectML gate, P6, decides)"
    else
      bad+=("a TraceLogging provider in the Microsoft telemetry group")
    fi
  fi
  if [ ${#bad[@]} -gt 0 ]; then
    status=1
    for s in "${bad[@]}"; do echo "check-telemetry: $f: FAIL: $s" >&2; done
  else
    echo "check-telemetry: $f: passed (no 1DS, curl, mbedTLS, collector host or unapproved telemetry group)"
  fi
done
exit $status
