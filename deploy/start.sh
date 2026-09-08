#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
: "${DOMAIN:?Set DOMAIN to the DNS name pointing at this host}"
if [[ ! -f secrets/keys.json ]]; then
  echo 'Create secrets/keys.json using server/create_key.py first.' >&2
  exit 1
fi
docker compose up -d --build
