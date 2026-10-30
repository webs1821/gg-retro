#!/bin/sh
set -eu
# An ignored, optional SQLx MySQL advisory must never become an active dependency.
graph=$(cargo tree --locked --workspace --prefix none --format '{p}')
if printf '%s\n' "$graph" | grep -Eq '^rsa v|^sqlx-mysql v'; then
  echo 'RSA/MySQL became active; the RustSec exception must be removed.' >&2
  exit 1
fi
cargo audit
