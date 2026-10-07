#!/usr/bin/env bash
# Fail early, naming every missing secret, without printing any value.
# Usage: require-secrets.sh <context> NAME [NAME...]   (values come from the environment)
set -euo pipefail
context="$1"; shift
missing=()
for name in "$@"; do
  if [ -z "${!name:-}" ]; then missing+=("$name"); fi
done
if [ "${#missing[@]}" -gt 0 ]; then
  echo "::error title=Missing release secrets::${context} needs these secrets, which are not configured: ${missing[*]}. See deploy/self-update/README.md (CI release) for how to create them."
  exit 1
fi
echo "${context}: all ${#} required secrets are present."
