#!/usr/bin/env bash
# Fails if any library source line has zero hits in the merged per-line (lcov) report.
# llvm-cov's file summary counts each compiled copy of generic code separately and can report a
# "missed" line that the per-line report shows as executed; this checks the per-line view.
set -euo pipefail
lcov="${1:-lcov.info}"
missed=$(awk -F'[:,]' '/^SF:/{file=$2} /^DA:/ && $3=="0" {print file":"$2}' "$lcov")
total=$(grep -c '^DA:' "$lcov")
if [ -n "$missed" ]; then
  echo "Uncovered lines:"; echo "$missed"; exit 1
fi
echo "Line coverage: 100% of ${total} lines"
