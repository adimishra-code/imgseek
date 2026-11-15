# Chrome / Chromium Ingestion Specification

This document specifies how `imgseek` locates, snapshots, parses, and ingests historical image provenance from local Chromium-based browsers without network I/O.

---

## 1. Profile Discovery

Chrome organizes browsing state in a **User Data** root directory containing one or more profile folders:
- Default profile: `Default`
- Additional profiles: `Profile 1`, `Profile 2`, ...

### Default User Data Paths per OS
- **Windows**: `%LOCALAPPDATA%\Google\Chrome\User Data\` (e.g. `C:\Users\<user>\AppData\Local\Google\Chrome\User Data`)
- **macOS**: `~/Library/Application Support/Google/Chrome/`
- **Linux**: `~/.config/google-chrome/` (and fallback `~/.config/chromium/`)

### Discovery CLI Flags
- `--user-data-dir <PATH>`: Explicitly overrides the User Data root directory.
- `--profile <NAME|PATH>`: Selects a specific profile name (e.g. `Default`, `Profile 1`) or an absolute/relative path to a profile directory.

---

## 2. Safe Database Snapshotting (Read-Only)

Chrome keeps its SQLite databases open with active Write-Ahead Logging (`WAL` mode). Opening the live database directly will cause sharing violations or SQLite lock contention.

### Protocol
1. **Never write to, lock, or modify live profile files.**
2. When reading `History`:
   - Copy `History`, `History-wal`, and `History-shm` (and `History-journal` if present) to an isolated, temporary scratch directory.
   - On Windows, open source handles with sharing permissions:
     `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE`.
   - If copying fails due to sharing violations, emit the user-facing diagnostic:
     *"Close Chrome and retry, or use --copy-from <dir>"*.
3. Open the copied SQLite database with `PRAGMA query_only = ON;` and `PRAGMA busy_timeout = 5000;`.
4. Delete the temporary snapshot directory upon completion or process exit.

---

## 3. History Database Parsing

### Schema Drift Tolerance
Chrome schemas drift across versions. Column definitions are feature-detected using:
`PRAGMA table_info(<table_name>);`
Unknown or omitted columns degrade gracefully with a logged warning, never causing process panics.

### Timestamp Conversion (WebKit Time)
Chrome stores timestamps as 64-bit integers representing microseconds since January 1, 1601 UTC.
Conversion to Unix epoch seconds:
$$\text{unix\_seconds} = \frac{\text{webkit\_microseconds}}{1,000,000} - 11,644,473,600$$

### Target Tables
- `urls`: `id`, `url`, `title`, `visit_count`, `typed_count`, `last_visit_time`
- `visits`: `id`, `url_id`, `visit_time`, `from_visit`, `transition`
- `downloads`: `id`, `target_path`, `start_time`, `end_time`, `received_bytes`, `total_bytes`, `state`, `mime_type`, `tab_url`, `referrer`
- `downloads_url_chains`: `id`, `chain_index`, `url`
- `keyword_search_terms`: `keyword_id`, `url_id`, `lower_term`, `term`

---

## 4. Download Ingestion (Exact Provenance)

When inspecting rows from `downloads`:
1. Check if `mime_type` starts with `image/` or file extension is an image (`.jpg`, `.jpeg`, `.png`, `.webp`, `.gif`, `.bmp`, `.tiff`).
2. If `target_path` still exists on disk and is a valid file:
   - Ingest image into `imgseek` storage.
   - Record sighting with `source_kind = 'chrome_download'`.
   - Record exact provenance: `page_url = tab_url` (or `referrer`), `seen_at = start_time`.

---

## 5. Chromium Simple Cache (`Cache_Data`)

### Directory Structure
Located in `<profile>/Cache/Cache_Data/` (or `<profile>/Cache/` on older versions).
Entry files are named `<16-hex-key-hash>_0`:
- `_0`: Combined HTTP response headers (stream 0) and response body (stream 1).
- `_1`: HTTP metadata or side-data.
- `_s`: Sparse files (range requests) — **ignored / skipped**.
- `index`, `index-dir`: Index files — ignored; entries are discovered by directory scan.

### Binary Entry Layout (`_0` file)
```
[SimpleFileHeader: 24 bytes]
  u64 magic: 0xfcfb6d1ba7725c30
  u32 version
  u32 key_length
  u32 key_hash
  u32 padding
[Key Bytes: key_length bytes]
[Stream 1: Response Body (Image Payload)]
[SimpleFileEOF for Stream 1: 24 bytes]
  u64 magic: 0xf4fa6f45970d41d8
  u32 flags
  u32 data_crc32
  u32 stream_size
  u32 padding
[Stream 0: HTTP Headers (Pickle / NUL-separated ASCII)]
[Optional 32-byte SHA-256 of the key, if flags indicate FLAG_HAS_KEY_SHA256]
[SimpleFileEOF for Stream 0: 24 bytes]
  u64 magic: 0xf4fa6f45970d41d8
  u32 flags
  u32 data_crc32
  u32 stream_size
  u32 padding
```

### Parsing from the End
1. Read the final 24 bytes of the file and verify `EOF` magic `0xf4fa6f45970d41d8`.
2. Extract `stream0_size` and flags from Stream 0 EOF.
3. Compute Stream 0 start offset. The preceding 24 bytes are Stream 1's EOF.
4. Verify Stream 1 EOF magic `0xf4fa6f45970d41d8`.
5. Extract `stream1_size` and verify body starts at `24 + key_length`.
6. Compute CRC32 of stream 1 data and verify against `data_crc32`.
7. Extract the key bytes and parse resource URL (last space-separated token in modern partitioned keys, e.g. `1/0/_dk_... <top_site> <resource_url>`).
8. Scan headers leniently for `Content-Type`, `Date`, and `Last-Modified`.
9. Verify payload with format sniffing (`imgseek_core::sniff_format`). Skip non-images or compressed `gzip`/`br` streams.

---

## 6. Page Association Heuristic

When an image is recovered from cache:
- Key gives `resource_url` and optional `top_frame_site`.
- Find the visit in `urls` / `visits` where host matches `top_frame_site` and `visit_time` is within $\pm 15$ minutes of the cache entry's `Date` (or file modification time).
- Sighting is recorded with `extra_json`:
  ```json
  {
    "page_match": "heuristic",
    "delta_seconds": 142,
    "top_frame_site": "https://example.com"
  }
  ```
- If no visit matches within window, store image URL with empty page association.

---

## 7. Privacy, Corpora & Purge Guarantees

1. All browser-derived records are assigned to corpus `chrome_history` by default.
2. URLs matching user-configured `exclude_patterns` (e.g. `*private*`, bank domains) are skipped during indexing.
3. Incognito browsing data is never present in `History` or disk cache and is never probed.
4. `imgseek purge --corpus chrome_history`:
   - Purges all document entries and sightings belonging to `chrome_history`.
   - Rewrites index segments to completely eliminate tombstoned records.
   - Deletes cached blobs.
   - Executes SQLite `VACUUM` to guarantee zero residual browsing records remain.
