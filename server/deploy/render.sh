#!/bin/sh
# Renders a configuration template with the environment (envsubst), then
# runs the tool with it:  render.sh <tool> <template> [extra args...]
# Both iroh-relay and iroh-dns-server take the configuration with `-c`.
set -eu
tool="$1"; template="$2"; shift 2
envsubst < "$template" > /tmp/config.toml
exec "$tool" -c /tmp/config.toml "$@"
