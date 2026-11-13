# imgseek

imgseek is a reverse image search engine that runs entirely on your own machine. It uses classical computer vision, signal processing, and information retrieval algorithms. It contains no neural networks, no machine learning models, and makes no network calls during query evaluation.

## What it does

- Exact and near-duplicate matching (Tier 1): finds identical or lightly modified images (resized, re-encoded, cropped, watermarked, or brightness-adjusted) using perceptual hashes and chroma histograms.
- Visual similarity and same-subject search (Tier 2): matches different photographs of the same physical object or scene across crops, rotations, and scale changes using local keypoints (ORB), a visual vocabulary tree, and geometric verification (RANSAC).
- Provenance tracking: links matched images back to their local file paths, Chrome browsing history, downloads, disk cache, or crawl sources.

## Building

Requires Rust (stable).

```sh
cargo build --release
```

## Running

```sh
# Initialize configuration
./target/release/imgseek config init

# Index a directory of images
./target/release/imgseek index ~/Pictures

# Query an image
./target/release/imgseek query photo.jpg
```

## Data and Storage

All state is stored locally under `.imgseek/`:
- `meta.db`: SQLite database storing document IDs, sighting metadata, and provenance.
- `blobs/`: Content-addressed storage (SHA-256) of indexed images.
- `index/`: Custom binary segment files and multi-index hash tables.

