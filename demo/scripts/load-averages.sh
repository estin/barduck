#!/bin/sh
# Demonstrates a composite source (spec: source-configuration — Composite
# source children): parses `uptime`'s three load averages and prints one
# ingest-shaped JSON array item per declared child, referenced from
# demo/config.toml's `load-averages` source.
uptime | sed -E 's/.*load average[s]?: *//' | tr -d ',' | awk '{
  printf "[{\"source\":\"load-averages::1m\",\"value\":\"%s\"},{\"source\":\"load-averages::5m\",\"value\":\"%s\"},{\"source\":\"load-averages::15m\",\"value\":\"%s\"}]\n", $1, $2, $3
}'
