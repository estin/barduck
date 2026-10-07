#!/bin/sh
# Composite-source demo: parses `free -b`'s Mem and Swap rows and prints one
# ingest-shaped JSON array item per declared child, referenced from
# demo/config.toml's `memory` source.
free -b | awk '
/^Mem:/ {
  printf "[{\"source\":\"memory::mem\",\"value\":\"%.1f\"}", $3 / $2 * 100
}
/^Swap:/ {
  printf ",{\"source\":\"memory::swap\",\"value\":\"%.1f\"}]\n", $3 / $2 * 100
}'
