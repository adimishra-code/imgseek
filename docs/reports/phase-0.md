# Phase Report: Phase 0 — Groundwork & Tier 1 Vertical Slice
Date: 2026-11-14
Commit: d1d3f7bfc42f46754e66f3a06b08a2f67e3c1454
Status: COMPLETE

## 1. What was built
- **Cargo Workspace Architecture**:
  - 9 decoupled crates: `imgseek-core`, `imgseek-store`, `imgseek-index`, `imgseek-features`, `imgseek-query`, `imgseek-chrome`, `imgseek-crawl`, `imgseek-eval`, `imgseek-web`, and root CLI `imgseek` (`crates/imgseek-cli`).
  - Strict network isolation on query and core processing paths.
- **Automated No-AI Compliance Enforcement**:
  - `scripts/check_no_ai.sh` and `scripts/check_no_ai.ps1` enforcing 0 ML dependencies and 0 forbidden model formats.
  - Integration test in `crates/imgseek-core/tests/no_ai_test.rs` ensuring zero ML crates in `Cargo.lock` and zero model binaries in the repository.
- **Format Sniffing & Safe Decoding**:
  - Magic-byte sniffing (`ImageFormat`) supporting JPEG, PNG, WebP, GIF, BMP, TIFF.
  - Image decode pipeline with EXIF orientation normalisation and pixel budget enforcement (default 100 MP).
- **Canonical Luma & Color Conversion**:
  - ITU-R BT.601 integer fixed-point luma calculation matching PIL `(R*19595 + G*38469 + B*7472) >> 16`.
  - Canonical image normalization (lanczos3 downscaling to max 1024px, float luma plane, analysis view).
- **Perceptual Hash Ensemble**:
  - **aHash**: 8x8 average hash with mean thresholding.
  - **dHash**: 9x8 difference hash using horizontal adjacent pixel gradients.
  - **pHash**: 32x32 Makhoul 1D DCT-II separated transform with 8x8 median-thresholded AC matrix.
  - **wHash**: 64x64 Haar DWT 4-level decomposition with 8x8 LL4 sub-band median thresholding.
  - Verified bit-exact against Python reference on Layer A and within tolerances on 200 synthetic images in Layer B.
- **Color Representation**:
  - 256-bin HSV chroma histogram (16 hue, 4 saturation, 4 value bins) with $L_1$ total variation distance.
- **Storage Subsystem**:
  - SQLite database (`imgseek-store::Database`) with WAL mode, foreign keys, and versioned schema migrations via `PRAGMA user_version`.
  - Content-Addressed Blob Store (`imgseek-store::BlobStore`) with zstd frame compression (level 3), 2-level prefix sharding (`blobs/ab/cd/<sha256>`), atomic writes, and SHA-256 integrity verification.
- **Indexing Engine**:
  - CLI `imgseek index <PATH>...` with recursive directory walking via `walkdir` and parallel processing via `rayon`.
  - Incremental re-indexing: skips unchanged files via `(file_size, file_mtime)` metadata cache stored in `sightings`.
- **Query Engine & CLI**:
  - `imgseek query <IMAGE_PATH>` supporting tabular and structured `--json` output.
  - Composite scoring formula combining pHash, wHash, dHash, aHash, and chroma distance.
  - Explainable result model (`QueryExplanation`, `ScoreBreakdown`) detailing distances, weights, and algorithm provenance.
  - Deterministic tie-breaking on `(score desc, doc_id asc)`.
- **Perturbation Test Suite**:
  - 100-query perturbation integration test (`crates/imgseek-cli/tests/perturbation_test.rs`) verifying rank #1 retrieval and score $\ge 0.55$ across crop, resize, JPEG re-encode, brightness shift, and watermark perturbations.

## 2. Verification against targets

| Target / Hypothesis | Target Metric | Measured Value | Result | Notes |
|---|---|---|---|---|
| No-AI contract enforcement | 0 ML crates, 0 model files | 0 ML crates, 0 model files | PASS | Verified in static script and CI integration test |
| Format sniffing & pixel budget | Reject $> 100\text{ MP}$ | Rejects $> 100\text{ MP}$ safely | PASS | Tested with synthetic header bombs |
| Layer A bit-exactness (aHash) | 0 bit diff on reference | 0 bit difference | PASS | Matches Python `imagehash.average_hash` bit-for-bit |
| Layer A bit-exactness (dHash) | 0 bit diff on reference | 0 bit difference | PASS | Matches Python `imagehash.dhash` bit-for-bit |
| Layer A bit-exactness (pHash) | 0 bit diff on reference | 0 bit difference | PASS | Makhoul DCT-II matches `scipy.fftpack.dct` bit-for-bit |
| Layer A bit-exactness (wHash) | 0 bit diff on reference | 0 bit difference | PASS | Haar DWT matches `pywt.wavedec2` bit-for-bit |
| Layer B 200-image tolerance (aHash) | mean $\le 2.0$, max $\le 6$ | mean 0.0500, max 3 | PASS | Well within tolerance bounds |
| Layer B 200-image tolerance (dHash) | mean $\le 2.0$, max $\le 6$ | mean 0.0350, max 3 | PASS | Well within tolerance bounds |
| Layer B 200-image tolerance (pHash) | mean $\le 2.0$, max $\le 6$ | mean 0.0500, max 4 | PASS | Well within tolerance bounds |
| Layer B 200-image tolerance (wHash) | mean $\le 2.0$, max $\le 6$ | mean 0.0450, max 4 | PASS | Well within tolerance bounds |
| Blob CAS integrity | SHA-256 verified reads | Corrupted payloads rejected | PASS | Verified in `blob_test.rs` |
| Indexing cache skip | Unchanged files skipped | 0 re-computations on re-run | PASS | Verified in `index_test.rs` |
| Perturbation retrieval accuracy | Exact copy ranks #1 | 100/100 queries ranked #1 (100%) | PASS | Verified across 5 perturbation types |

## 3. Decisions & deviations recorded
- `docs/DECISIONS.md`: untracked from git per user configuration; architectural decisions recorded in internal engineering log.
- 64-bit MinGW toolchain configured on Windows host (`BrechtSanders.WinLibs.POSIX.UCRT`) to resolve `dlltool` linker issues for C dependencies (`libsqlite3-sys`, `zstd-sys`).
- Query rank ordering made strictly deterministic per Rule R7 by ordering by `(score desc, doc_id asc)`.

## 4. Golden / regression test status
- All workspace tests pass (`cargo test --workspace`):
  - `imgseek-core`: 28 unit tests, 2 No-AI integration tests, 1 Layer B 200-image pipeline tolerance test.
  - `imgseek-store`: 2 blob CAS tests, 4 SQLite schema & persistence tests.
  - `imgseek-cli`: 1 end-to-end indexing & cache test, 1 query exact match & JSON explanation test, 1 100-query perturbation tolerance test.
- Automated No-AI gate script: `check_no_ai.ps1` passed with 0 violations.
- Clippy code quality gate: `cargo clippy --workspace --all-targets -- -D warnings` passed with 0 warnings.

## 5. Next phase readiness
- Phase 0 prerequisites for Phase 1 are fully satisfied.
- Codebase, tests, and documentation are committed and pushed to `main`.
- Ready for Phase 1 ("Tier 1 at Scale & Multi-Corpus Ingest") upon explicit user instruction.
