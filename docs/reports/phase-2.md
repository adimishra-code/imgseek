# Phase Report: Phase 2 — The Browser Corpus: Chrome History, Downloads, Disk Cache
Date: 2026-11-17
Status: COMPLETE

## 1. What was built

- **Chrome Ingestion Specification (`docs/CHROME_INGEST.md`)**:
  - Comprehensive specification detailing Chromium storage architectures across Windows, macOS, and Linux.
  - Reverse binary layout of the Chromium Simple Cache (`_0` files, magic numbers, Stream 0 HTTP metadata, Stream 1 payload).
  - Explicit privacy rules: exclusion of non-HTTP schemes (`file://`, `data:`, `chrome://`, `blob:`), URL query parameter stripping when requested, configurable domain blocklists, and full purge compliance.
  - Temporal heuristic association algorithm ($\pm 15$ minutes window) linking cached image assets to parent visit pages.

- **Cross-Platform Profile Discovery (`imgseek-chrome::discovery`)**:
  - Discovers Chrome installations and profiles across:
    - Windows: `%LOCALAPPDATA%\Google\Chrome\User Data`
    - macOS: `~/Library/Application Support/Google/Chrome`
    - Linux: `~/.config/google-chrome`
  - Automatically identifies default and secondary profiles (`Default`, `Profile 1`, `Profile 2`, ...).
  - Validates profile directories via `Preferences` or `Local State` manifests.
  - Supports synthetic and custom profile directory overrides for reproducible testing.

- **Safe SQLite Snapshotting Subsystem (`imgseek-chrome::snapshot`)**:
  - Solves live SQLite file-lock contention when Chrome is actively running.
  - Atomically copies `History`, `History-wal`, and `History-shm` into an isolated temporary directory before opening.
  - Opens snapshots with read-only pragmas (`PRAGMA query_only = ON;`, `PRAGMA busy_timeout = 5000;`) to guarantee non-destructive reads.
  - RAII-based `HistorySnapshot` ensures complete cleanup of temporary snapshot files on drop.

- **Schema Drift Resilient History Parser (`imgseek-chrome::history`)**:
  - Dynamically inspects SQLite table columns using `PRAGMA table_info(downloads)` and `PRAGMA table_info(urls)` to gracefully adapt to different Chromium schema versions.
  - Robustly falls back when optional columns (e.g., `guid`, `mime_type`, `original_mime_type`) are missing in older or customized Chromium builds.
  - Accurately converts WebKit microsecond timestamps (microseconds since Jan 1, 1601 UTC) to standard Unix timestamps (seconds since Jan 1, 1970 UTC).
  - Extracts full download chains (`downloads` joined with `downloads_url_chains`), page URLs, visit times, and visit titles.

- **Downloaded Images Ingestion Pipeline (`imgseek-chrome::downloads`)**:
  - Validates image file existence and extracts MIME types and extensions.
  - Applies exclude filters against non-web schemes and user-specified domain patterns.
  - Full Tier 1 perceptual hashing: 64-bit pHash, wHash, dHash, aHash, 256-bit PDQ perceptual hash, PDQ quality metric, HSV average chroma, and dominant color palette.
  - Stores deduplicated image content in the content-addressed `BlobStore`.
  - Records sightings in the SQLite database under corpus `chrome-downloads`.

- **Chromium Simple Cache Reverse Binary Parser (`imgseek-chrome::simple_cache`)**:
  - Implements Chromium's Simple Cache reverse stream layout parser for `_0` files.
  - Validates `SimpleFileHeader` magic (`0xfcfb6d1ba7725c30`) and `SimpleFileEOF` magic (`0xf4fa6f45970d41d8`).
  - Reads Stream 0 from the end of the file, parsing HTTP response headers (status code, content type, date, ETag, Referer).
  - Extracts Stream 1 raw cached image payloads while verifying CRC32 checksums.
  - Sniffs image magic bytes (PNG, JPEG, WebP, GIF, BMP, AVIF, TIFF, ICO) to index valid images even when HTTP MIME headers are generic (`application/octet-stream`) or absent.

- **Page Association Heuristic & Provenance (`imgseek-chrome::association`)**:
  - Parses Chromium cache keys (e.g. `1/0/https://example.com/asset.png https://example.com`) to extract `top_frame_site`.
  - Cross-references cache entry timestamps (HTTP `Date` header or file modification time) with browser `visits` in SQLite.
  - Associates cached images with the closest visit page within a $\pm 15$ minute ($\pm 900$ s) temporal window.
  - Ingests cached assets under corpus `chrome-cache`, embedding rich provenance metadata in `extra_json` (`page_match`, `delta_seconds`, `top_frame_site`, `page_url`).

- **CLI Subcommands (`imgseek-cli::chrome`, `imgseek-cli::purge`)**:
  - `imgseek index chrome`: Full indexing command with options:
    - `--profile <NAME_OR_PATH>`: Target specific profile or custom path.
    - `--skip-cache`: Ingest only downloaded files.
    - `--skip-downloads`: Ingest only disk cache items.
    - `--time-window-hours <HOURS>`: Restrict ingestion to recent browser activity.
    - `--exclude-domains <CSV>`: Skip specified sensitive domains.
  - `imgseek chrome report`: Summarizes indexed browser profiles, total download sightings, disk cache sightings, page association ratios, and top origin domains.
  - `imgseek purge --corpus <NAME>`: Rigorous unindexing and privacy removal command:
    - Purges all sightings for the specified corpus (`chrome-downloads` or `chrome-cache`).
    - Removes unreferenced content-addressed blobs from disk (`BlobStore::delete`).
    - Appends tombstones and triggers segment compaction in the search index (`imgseek-index`).
    - Executes SQLite `VACUUM` to physically reclaim disk space.

- **Fuzzing & Robustness Test Suite (`crates/imgseek-chrome/tests/fuzz_cache_test.rs`)**:
  - Property-based testing using `proptest`.
  - 500 arbitrary byte slices fuzzed against `parse_cache_entry_bytes` and `parse_cache_item` with zero crashes or panics.
  - Bit-flip mutation and truncation testing on valid cache entries to ensure graceful CRC32 and magic byte failure modes.
  - Fuzzing of cache key parsers, HTTP header parsers, and HTTP IMF-fixdate parsers.
  - Corrupt SQLite database fuzzing verifying graceful error propagation.

---

## 2. Verification against targets

| Target / Hypothesis | Target Metric | Measured Value | Result | Notes |
|---|---|---|---|---|
| Profile discovery | Detects default & secondary profiles | Validated on Windows, macOS, Linux structures | **PASS** | Auto-detects `Default` and `Profile *` |
| History lock contention | Non-blocking with live Chrome | 100% non-blocking | **PASS** | Snapshot isolation with WAL/SHM copying |
| Schema drift resilience | Tolerates missing columns / versions | 100% passing on drifted schemas | **PASS** | Dynamic PRAGMA column querying with safe fallbacks |
| Simple cache parsing | Robust reverse binary reading | 100% valid CRC32 & magic checks | **PASS** | Validates `0xfcfb6d1ba7725c30` & `0xf4fa6f45970d41d8` |
| Image format sniffing | Identifies image without/bad MIME | Sniffs PNG, JPEG, WebP, GIF, BMP, etc. | **PASS** | Magic byte fallback sniffer |
| Page association | Matches visit within $\pm 15$ min window | Accurately associates nearest visit | **PASS** | Rejects outside window; tracks delta in `extra_json` |
| CLI chrome indexing | Working `index chrome` command | End-to-end indexing passes | **PASS** | Verified via CLI integration tests |
| CLI chrome reporting | Working `chrome report` command | Formatted terminal report | **PASS** | Domain aggregation & sighting breakdown |
| Complete corpus purge | 100% removal of records & unlinked blobs | 0 sightings, 0 orphan blobs, compacted index | **PASS** | Verified by `purge_test.rs` integration test |
| Cache parser fuzzing | 0 crashes / panics on corrupt bytes | 0 crashes / panics (500 proptests + bit flips) | **PASS** | Graceful error propagation |
| Offline network isolation | Zero network I/O during operations | 0 network crates in query path | **PASS** | Verified by static audit & `offline_query_test.rs` |
| Automated No-AI ban | 0 ML crates, 0 model files | 0 ML crates, 0 model files | **PASS** | Enforced by `check_no_ai.ps1` and `no_ai_test.rs` |

---

## 3. Test Suite Summary

- **Total Workspace Tests**: Over 100 unit, integration, and fuzz tests passing cleanly (`cargo test --workspace`):
  - `imgseek-chrome`: 17 unit tests, 6 property/fuzz tests (cache parser, headers, dates, keys, bit mutations, corrupt DBs).
  - `imgseek` CLI: `chrome_test` (e2e chrome index & report), `purge_test` (e2e purge, unlinking, vacuum), `index_test` (e2e directory index), `perturbation_test` (100 query perturbations), `query_test` (exact & json match).
  - `imgseek-core`: 35 unit tests, Layer B 200-image tolerance test, No-AI ban tests.
  - `imgseek-index`: 24 unit tests, `compaction_verify_test`, `determinism_test`, `mih_differential_test`.
  - `imgseek-store`: `blob_test`, `schema_test`.
  - `imgseek-query`: `offline_query_test`.
  - `imgseek-eval`: `scale_benchmark_test`.
- **Fast Gates**:
  - `cargo fmt --check`: PASSED.
  - `cargo clippy --workspace --all-targets -- -D warnings`: PASSED (0 warnings).
  - `powershell -File .\scripts\check_no_ai.ps1`: PASSED (0 ML crates, 0 model files).

---

## 4. Next Phase Readiness

- **Phase 2 is 100% COMPLETE**.
- All Phase 2 deliverables (Steps 2.01 through 2.11) are implemented, tested, committed, and pushed to GitHub.
- Browser corpus ingestion for Chrome history, downloads, and disk cache is fully functional, robust against corruption and live locks, and strictly privacy-respecting with full purge support.
- Ready to proceed to **Phase 3 ("Tier 2: Same-Subject Search — FAST-9, rBRIEF, Visual Vocabulary, Inverted Index, RANSAC")** upon explicit user request.
