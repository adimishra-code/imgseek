# imgseek Index Format Specification (v1)

This document defines the on-disk binary format of the `imgseek` search index.
All multi-byte numeric values are stored in **little-endian** byte order unless explicitly noted.
Floating-point values adhere to IEEE 754 (binary64).

---

## 1. Overview & Architectural Goals

1. **Memory-Mapped Reads (`mmap`)**: Queries access segments directly from disk cache without deserializing vectors or posting lists into heap memory.
2. **Immutable Segments**: Once written and verified, segment files are read-only. Updates and deletions never mutate existing segment data in place.
3. **Crash Safety via Atomic Manifest Swaps**: The active state of the index is determined entirely by an atomic pointer (`manifest.json`). Segment creation, merges, and purges occur in temporary files and only become visible when an updated manifest is atomically swapped into place.
4. **End-to-End Integrity Verification**: Every segment file ends with a structured footer containing record counts, offsets, and a 32-bit CRC32C checksum over all preceding bytes. Corrupted or partially written files are rejected on open.
5. **Deterministic File Layout**: Given identical input documents and parameters, segment generation produces byte-identical files.

---

## 2. Directory Layout

An `imgseek` index directory (configured via `index.dir`, defaulting to `.imgseek/index`) contains:

```
<index_dir>/
├── manifest.json              # Active manifest: generation, active segments, algo versions
├── manifest.json.tmp          # Temporary file used for atomic manifest write-and-rename
├── seg_00000001.idx           # Index segment: MIH chunk tables & postings
├── seg_00000001.data          # Data segment: packed 64-bit & 256-bit hashes, metadata
├── seg_00000001.tomb          # Tombstone bitset: deleted doc IDs for segment 1
├── seg_00000002.idx
├── seg_00000002.data
├── seg_00000002.tomb
└── ...
```

---

## 3. The Manifest (`manifest.json`)

`manifest.json` is a human-readable and machine-verifiable UTF-8 JSON document tracking the complete state of the index.

### 3.1 Schema

```json
{
  "format_version": 1,
  "generation": 42,
  "created_at_utc": "2026-11-14T19:22:00Z",
  "algo_versions": {
    "phash": 1,
    "whash": 1,
    "dhash": 1,
    "ahash": 1,
    "pdq": 1,
    "chroma": 1,
    "orb": 1
  },
  "segments": [
    {
      "segment_id": 1,
      "doc_count": 100000,
      "tombstone_count": 12,
      "idx_file": "seg_00000001.idx",
      "data_file": "seg_00000001.data",
      "tomb_file": "seg_00000001.tomb",
      "idx_checksum": 2489128371,
      "data_checksum": 1827391823,
      "min_doc_id": 1,
      "max_doc_id": 100000
    }
  ]
}
```

### 3.2 Atomic Swap Protocol

1. Prepare new segments (`seg_XXXX.idx.tmp`, `seg_XXXX.data.tmp`).
2. Write and `fsync` all segment files, then rename them to their permanent names.
3. Serialize the updated manifest to `manifest.json.tmp`.
4. Flush and `fsync` `manifest.json.tmp`.
5. Execute an atomic platform filesystem rename (`rename` / `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING`) replacing `manifest.json`.
6. Any process reading `manifest.json` observes either the previous complete state or the new complete state; half-written states are impossible.

---

## 4. Segment Files

A single segment comprises three sibling files:
1. `.data`: Packed fixed-width records for all documents in the segment.
2. `.idx`: Multi-Index Hash (MIH) chunk tables and compressed postings lists.
3. `.tomb`: Roaring / dense bitset identifying tombstoned (purged/deleted) `DocId`s.

### 4.1 Common File Header & Footer

Every `.idx` and `.data` file begins with a 32-byte header and ends with a 16-byte footer:

#### Header (32 bytes)
| Offset | Type | Field | Description |
|---|---|---|---|
| 0..8 | `[u8; 8]` | `magic` | Magic identifier: `"SEGKIDX\0"` or `"SEGKDAT\0"` |
| 8..12 | `u32` | `format_version` | Format version number (currently `1`) |
| 12..20 | `u64` | `segment_id` | Monotonically increasing segment identifier |
| 20..24 | `u32` | `doc_count` | Number of documents indexed in this segment |
| 24..32 | `u64` | `header_reserved` | Reserved for future flags (zero-filled) |

#### Footer (16 bytes)
| Offset | Type | Field | Description |
|---|---|---|---|
| 0..8 | `u64` | `payload_len` | Byte length of payload between header and footer |
| 8..12 | `u32` | `crc32c` | CRC32C of all preceding bytes (header + payload) |
| 12..16 | `[u8; 4]` | `footer_magic` | Magic end marker: `"\xDE\xAD\xBE\xEF"` |

---

### 4.2 Data Segment (`.data`)

The payload of `.data` contains fixed-width doc records indexed by segment-relative offset `(doc_id - min_doc_id)`:

```
[32-byte Header]
[Record 0 (64 bytes)]
[Record 1 (64 bytes)]
...
[Record N-1 (64 bytes)]
[16-byte Footer]
```

#### Record Structure (64 bytes per document)
| Offset | Type | Field | Description |
|---|---|---|---|
| 0..4 | `u32` | `doc_id` | Global dense document ID |
| 4..12 | `u64` | `phash` | 64-bit DCT pHash |
| 12..20 | `u64` | `whash` | 64-bit DWT wHash |
| 20..28 | `u64` | `dhash` | 64-bit difference hash |
| 28..36 | `u64` | `ahash` | 64-bit average hash |
| 36..68 | `[u8; 32]`| `pdq` | 256-bit PDQ hash |
| 68..70 | `u16` | `pdq_quality` | PDQ confidence/quality metric (0..100) |
| 70..72 | `u16` | `width` | Image pixel width |
| 72..74 | `u16` | `height` | Image pixel height |
| 74..106| `[u8; 32]`| `chroma_ref` | Content-addressed SHA-256 reference for chroma blob |

*(Total record size: 106 bytes, or aligned to 128 bytes with zero padding for direct SIMD/cache-line aligned indexing).*

---

### 4.3 Index Segment (`.idx`)

The `.idx` file stores Multi-Index Hash (MIH) tables.

#### Multi-Index Hashing (MIH) Principle
To find all hashes within Hamming distance $\tau$:
- For a 64-bit hash (pHash, dHash, wHash), divide the 64 bits into $m = 4$ chunks of $b = 16$ bits each.
- By the **pigeonhole principle**, if the total Hamming distance between two 64-bit hashes is $\le \tau$, then in at least one 16-bit chunk the distance is $\le \lfloor \tau / 4 \rfloor$.
- For exact/near-duplicate search ($\tau \le 12$), $\lfloor 12 / 4 \rfloor = 3$. We only probe 16-bit keys within Hamming distance $r \le 3$. At $r = 0$, exact chunk lookup requires 0 neighbor probes.
- For 256-bit PDQ, divide into $m = 16$ chunks of $b = 16$ bits each. If $\tau \le 31$, then $\lfloor 31 / 16 \rfloor = 1$. Probing requires at most radius 1 neighbors (16 probes) across chunk tables!

#### Chunk Layout
Each chunk table $C_k$ ($k \in 0..m-1$) maps every possible 16-bit value ($2^{16} = 65,536$ buckets) to a list of matching `doc_id`s.

```
[32-byte Header]
[Chunk Table Directory: offsets to 4 tables for phash, 4 for dhash, 4 for whash, 16 for pdq]
[Bucket Offsets & Lengths Table: 65,536 entries of (u32 offset, u32 len) per chunk]
[Compressed Postings Data Stream]
[16-byte Footer]
```

---

## 5. Postings Compression

Posting lists (lists of matching `DocId`s within a chunk bucket) are sorted in ascending order:
`[doc_0, doc_1, doc_2, ..., doc_{k-1}]`

### 5.1 Delta Encoding
Compute first-order differences:
$\Delta_0 = \text{doc}_0$
$\Delta_i = \text{doc}_i - \text{doc}_{i-1} - 1 \quad (\forall i \ge 1)$
Since `doc_id`s are strictly increasing, $\Delta_i \ge 0$.

### 5.2 Frame of Reference (FOR) + Varint Packing
- **Full Blocks (128 integers)**:
  - Find $b = \lceil \log_2(\max(\Delta) + 1) \rceil$ (the bit-width needed for the block).
  - Store a 1-byte header containing $b$.
  - Bit-pack the 128 integers tightly using $128 \times b$ bits.
- **Tail Elements (< 128 integers)**:
  - Encoded using standard LEB128 / Variable-byte (`varint`) encoding.
  - A continuation bit (MSB) in each byte indicates if further bytes follow; remaining 7 bits store payload.

---

## 6. Tombstones & Purge Guarantees

- **Tombstone Bitset (`.tomb`)**:
  - A dense byte array where bit $i$ is set if relative doc $(min\_doc\_id + i)$ has been marked for deletion.
  - Queries check this bitset using fast bitwise operations (`tombstones[idx >> 3] & (1 << (idx & 7))`) and discard matching candidates before evaluation.
- **Physical Segment Purge (`imgseek purge`)**:
  - Tombstones hide deleted items during queries.
  - When `imgseek purge` or segment compaction runs, affected segments are rewritten from scratch excluding all tombstoned records, and the old segment files are unlinked.
  - No deleted hash, image reference, or metadata remains in segment storage.

---

## 7. Verification (`imgseek verify`)

The `imgseek verify` command runs against all segments referenced in `manifest.json`:
1. Checks magic bytes and format version in headers.
2. Recomputes CRC32C over the header and payload and compares it against the footer CRC32C.
3. Validates that all postings offsets point within the valid postings stream bounds.
4. Validates that decompressed posting lists are strictly monotonically increasing.
5. Emits an integrity report with total doc count, active doc count, and segment health.
