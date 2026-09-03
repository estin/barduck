#!/bin/sh
# Example bash-source script. Referenced from demo/config.toml with a
# relative path, resolved against the config file's directory (demo/),
# not wherever the daemon happened to be launched from.
uptime | sed -E 's/.*load average[s]?: ([0-9.]+).*/\1/'
