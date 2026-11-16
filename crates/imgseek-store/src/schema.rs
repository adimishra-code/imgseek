use rusqlite::Connection;

use crate::error::Result;

pub const CURRENT_SCHEMA_VERSION: u32 = 1;

pub const SCHEMA_V1: &str = r#"
CREATE TABLE IF NOT EXISTS corpora (
    corpus_id   TEXT PRIMARY KEY,
    kind        TEXT NOT NULL,
    root_uri    TEXT NOT NULL,
    created_at  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS docs (
    doc_id          INTEGER PRIMARY KEY AUTOINCREMENT,
    sha256          BLOB NOT NULL UNIQUE,
    byte_size       INTEGER NOT NULL,
    format          TEXT NOT NULL,
    width           INTEGER NOT NULL,
    height          INTEGER NOT NULL,
    ahash           INTEGER NOT NULL,
    dhash           INTEGER NOT NULL,
    phash           INTEGER NOT NULL,
    whash           INTEGER NOT NULL,
    chroma_blob_ref TEXT NOT NULL,
    keypoints_ref   TEXT,
    indexed_at      INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS sightings (
    sighting_id INTEGER PRIMARY KEY AUTOINCREMENT,
    doc_id      INTEGER NOT NULL REFERENCES docs(doc_id) ON DELETE CASCADE,
    corpus_id   TEXT NOT NULL REFERENCES corpora(corpus_id) ON DELETE CASCADE,
    origin_uri  TEXT NOT NULL,
    title       TEXT,
    discovered_at INTEGER NOT NULL,
    file_size   INTEGER,
    file_mtime  INTEGER,
    http_etag   TEXT,
    http_last_modified TEXT,
    extra_json  TEXT,
    UNIQUE(corpus_id, origin_uri)
);

CREATE TABLE IF NOT EXISTS index_state (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_docs_ahash ON docs(ahash);
CREATE INDEX IF NOT EXISTS idx_docs_dhash ON docs(dhash);
CREATE INDEX IF NOT EXISTS idx_docs_phash ON docs(phash);
CREATE INDEX IF NOT EXISTS idx_docs_whash ON docs(whash);
CREATE INDEX IF NOT EXISTS idx_sightings_doc_id ON sightings(doc_id);
CREATE INDEX IF NOT EXISTS idx_sightings_origin ON sightings(corpus_id, origin_uri);
CREATE INDEX IF NOT EXISTS idx_sightings_mtime ON sightings(file_mtime);
"#;

pub fn apply_pragmas(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA busy_timeout = 5000;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;",
    )?;
    Ok(())
}

pub fn migrate(conn: &mut Connection) -> Result<()> {
    let version: u32 = conn.query_row("PRAGMA user_version;", [], |row| row.get(0))?;

    if version < 1 {
        conn.execute_batch(SCHEMA_V1)?;
        conn.pragma_update(None, "user_version", 1)?;
    }

    Ok(())
}
