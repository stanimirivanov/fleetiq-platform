#!/usr/bin/env bash
set -euo pipefail

receipt=$(bash scripts/images/authorization.sh)
mkdir -p target/images
printf '%s\n' "$receipt" > target/images/authorization.json
printf 'Publication prerequisites passed for %s. Manual dispatch authorization has been verified.\n' "$FLEETIQ_SOURCE_REVISION"
