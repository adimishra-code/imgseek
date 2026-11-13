#!/usr/bin/env bash
set -euo pipefail

echo "Running query-path network isolation verification..."

ISOLATED_CRATES=("imgseek-core" "imgseek-features" "imgseek-index" "imgseek-store" "imgseek-query")
DENY_NET_REGEX='^(reqwest|hyper|ureq|isahc|surf|attohttpc|curl)$'

FAILED=0
for crate in "${ISOLATED_CRATES[@]}"; do
    TREE_CRATES=$(cargo tree -p "$crate" --prefix none 2>/dev/null | awk '{print $1}' | sort -u || true)
    HAS_TOKIO=0
    while IFS= read -r dep; do
        if [[ -z "$dep" ]]; then
            continue
        fi
        if [[ "$dep" == "tokio" ]]; then
            HAS_TOKIO=1
        fi
        if echo "$dep" | grep -Eq "$DENY_NET_REGEX"; then
            echo "ERROR: Query-path crate $crate depends on network crate: $dep" >&2
            FAILED=1
        fi
    done <<< "$TREE_CRATES"

    # Also check tokio net feature if tokio is present
    if [ "$HAS_TOKIO" -eq 1 ]; then
        TOKIO_FEATURES=$(cargo tree -p "$crate" -e features -i tokio 2>/dev/null || true)
        if echo "$TOKIO_FEATURES" | grep -Eq '\bnet\b'; then
            echo "ERROR: Query-path crate $crate enables tokio net feature" >&2
            FAILED=1
        fi
    fi
done

if [ "$FAILED" -ne 0 ]; then
    echo "FAILED: Query path isolation violated." >&2
    exit 1
fi

echo "Query isolation check passed: query crates have zero network dependencies."
