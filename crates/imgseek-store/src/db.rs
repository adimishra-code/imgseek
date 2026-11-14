use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::Result;
use crate::models::{Corpus, Doc, IndexRecord, NewDoc, NewSighting, Sighting, SightingMetaMap};
use crate::schema::{apply_pragmas, migrate};

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init(conn)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        apply_pragmas(&conn)?;
        migrate(&mut conn)?;
        Ok(Self { conn })
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    pub fn insert_corpus(&self, corpus: &Corpus) -> Result<()> {
        self.conn.execute(
            "INSERT INTO corpora (corpus_id, kind, root_uri, created_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(corpus_id) DO UPDATE SET
                kind = excluded.kind,
                root_uri = excluded.root_uri;",
            params![
                corpus.corpus_id,
                corpus.kind,
                corpus.root_uri,
                corpus.created_at
            ],
        )?;
        Ok(())
    }

    pub fn get_corpus(&self, corpus_id: &str) -> Result<Option<Corpus>> {
        let res = self
            .conn
            .query_row(
                "SELECT corpus_id, kind, root_uri, created_at
                 FROM corpora WHERE corpus_id = ?1;",
                params![corpus_id],
                |row| {
                    Ok(Corpus {
                        corpus_id: row.get(0)?,
                        kind: row.get(1)?,
                        root_uri: row.get(2)?,
                        created_at: row.get(3)?,
                    })
                },
            )
            .optional()?;
        Ok(res)
    }

    pub fn insert_doc(&self, doc: &NewDoc) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO docs (
                sha256, byte_size, format, width, height,
                ahash, dhash, phash, whash,
                chroma_blob_ref, keypoints_ref, indexed_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12);",
            params![
                doc.sha256.as_slice(),
                doc.byte_size,
                doc.format,
                doc.width,
                doc.height,
                doc.ahash as i64,
                doc.dhash as i64,
                doc.phash as i64,
                doc.whash as i64,
                doc.chroma_blob_ref,
                doc.keypoints_ref,
                doc.indexed_at,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_doc_by_id(&self, doc_id: i64) -> Result<Option<Doc>> {
        let res = self
            .conn
            .query_row(
                "SELECT doc_id, sha256, byte_size, format, width, height,
                        ahash, dhash, phash, whash,
                        chroma_blob_ref, keypoints_ref, indexed_at
                 FROM docs WHERE doc_id = ?1;",
                params![doc_id],
                Self::row_to_doc,
            )
            .optional()?;
        Ok(res)
    }

    pub fn get_doc_by_sha256(&self, sha256: &[u8; 32]) -> Result<Option<Doc>> {
        let res = self
            .conn
            .query_row(
                "SELECT doc_id, sha256, byte_size, format, width, height,
                        ahash, dhash, phash, whash,
                        chroma_blob_ref, keypoints_ref, indexed_at
                 FROM docs WHERE sha256 = ?1;",
                params![sha256.as_slice()],
                Self::row_to_doc,
            )
            .optional()?;
        Ok(res)
    }

    pub fn get_all_docs(&self) -> Result<Vec<Doc>> {
        let mut stmt = self.conn.prepare(
            "SELECT doc_id, sha256, byte_size, format, width, height,
                    ahash, dhash, phash, whash,
                    chroma_blob_ref, keypoints_ref, indexed_at
             FROM docs ORDER BY doc_id ASC;",
        )?;
        let doc_iter = stmt.query_map([], Self::row_to_doc)?;
        let mut docs = Vec::new();
        for doc in doc_iter {
            docs.push(doc?);
        }
        Ok(docs)
    }

    pub fn get_docs_by_corpus(&self, corpus_id: &str) -> Result<Vec<Doc>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT d.doc_id, d.sha256, d.byte_size, d.format, d.width, d.height,
                    d.ahash, d.dhash, d.phash, d.whash,
                    d.chroma_blob_ref, d.keypoints_ref, d.indexed_at
             FROM docs d
             JOIN sightings s ON d.doc_id = s.doc_id
             WHERE s.corpus_id = ?1
             ORDER BY d.doc_id ASC;",
        )?;
        let doc_iter = stmt.query_map(params![corpus_id], Self::row_to_doc)?;
        let mut docs = Vec::new();
        for doc in doc_iter {
            docs.push(doc?);
        }
        Ok(docs)
    }

    pub fn get_sightings_by_doc_id(&self, doc_id: i64) -> Result<Vec<Sighting>> {
        let mut stmt = self.conn.prepare(
            "SELECT sighting_id, doc_id, corpus_id, origin_uri, title,
                    discovered_at, file_size, file_mtime,
                    http_etag, http_last_modified
             FROM sightings WHERE doc_id = ?1 ORDER BY sighting_id ASC;",
        )?;
        let rows = stmt.query_map(params![doc_id], |row| {
            Ok(Sighting {
                sighting_id: row.get(0)?,
                doc_id: row.get(1)?,
                corpus_id: row.get(2)?,
                origin_uri: row.get(3)?,
                title: row.get(4)?,
                discovered_at: row.get(5)?,
                file_size: row.get(6)?,
                file_mtime: row.get(7)?,
                http_etag: row.get(8)?,
                http_last_modified: row.get(9)?,
            })
        })?;
        let mut sightings = Vec::new();
        for s in rows {
            sightings.push(s?);
        }
        Ok(sightings)
    }

    pub fn insert_batch(&mut self, records: &[IndexRecord]) -> Result<(usize, usize)> {
        let tx = self.conn.transaction()?;
        let mut docs_inserted = 0;
        let mut sightings_inserted = 0;

        for rec in records {
            let existing_doc_id: Option<i64> = tx
                .query_row(
                    "SELECT doc_id FROM docs WHERE sha256 = ?1;",
                    params![rec.sha256.as_slice()],
                    |r| r.get(0),
                )
                .optional()?;

            let doc_id = match existing_doc_id {
                Some(id) => id,
                None => {
                    tx.execute(
                        "INSERT INTO docs (
                            sha256, byte_size, format, width, height,
                            ahash, dhash, phash, whash,
                            chroma_blob_ref, keypoints_ref, indexed_at
                         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12);",
                        params![
                            rec.sha256.as_slice(),
                            rec.byte_size,
                            rec.format,
                            rec.width,
                            rec.height,
                            rec.ahash as i64,
                            rec.dhash as i64,
                            rec.phash as i64,
                            rec.whash as i64,
                            rec.chroma_blob_ref,
                            rec.keypoints_ref,
                            rec.indexed_at,
                        ],
                    )?;
                    docs_inserted += 1;
                    tx.last_insert_rowid()
                }
            };

            tx.execute(
                "INSERT INTO sightings (
                    doc_id, corpus_id, origin_uri, title,
                    discovered_at, file_size, file_mtime,
                    http_etag, http_last_modified
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, NULL)
                 ON CONFLICT(corpus_id, origin_uri) DO UPDATE SET
                    doc_id = excluded.doc_id,
                    title = excluded.title,
                    discovered_at = excluded.discovered_at,
                    file_size = excluded.file_size,
                    file_mtime = excluded.file_mtime;",
                params![
                    doc_id,
                    rec.corpus_id,
                    rec.origin_uri,
                    rec.title,
                    rec.discovered_at,
                    rec.file_size,
                    rec.file_mtime,
                ],
            )?;
            sightings_inserted += 1;
        }

        tx.commit()?;
        Ok((docs_inserted, sightings_inserted))
    }

    pub fn insert_or_replace_sighting(&self, sighting: &NewSighting) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO sightings (
                doc_id, corpus_id, origin_uri, title,
                discovered_at, file_size, file_mtime,
                http_etag, http_last_modified
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(corpus_id, origin_uri) DO UPDATE SET
                doc_id = excluded.doc_id,
                title = excluded.title,
                discovered_at = excluded.discovered_at,
                file_size = excluded.file_size,
                file_mtime = excluded.file_mtime,
                http_etag = excluded.http_etag,
                http_last_modified = excluded.http_last_modified;",
            params![
                sighting.doc_id,
                sighting.corpus_id,
                sighting.origin_uri,
                sighting.title,
                sighting.discovered_at,
                sighting.file_size,
                sighting.file_mtime,
                sighting.http_etag,
                sighting.http_last_modified,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_sighting(&self, corpus_id: &str, origin_uri: &str) -> Result<Option<Sighting>> {
        let res = self
            .conn
            .query_row(
                "SELECT sighting_id, doc_id, corpus_id, origin_uri, title,
                        discovered_at, file_size, file_mtime,
                        http_etag, http_last_modified
                 FROM sightings WHERE corpus_id = ?1 AND origin_uri = ?2;",
                params![corpus_id, origin_uri],
                |row| {
                    Ok(Sighting {
                        sighting_id: row.get(0)?,
                        doc_id: row.get(1)?,
                        corpus_id: row.get(2)?,
                        origin_uri: row.get(3)?,
                        title: row.get(4)?,
                        discovered_at: row.get(5)?,
                        file_size: row.get(6)?,
                        file_mtime: row.get(7)?,
                        http_etag: row.get(8)?,
                        http_last_modified: row.get(9)?,
                    })
                },
            )
            .optional()?;
        Ok(res)
    }

    pub fn get_corpus_sightings_metadata(&self, corpus_id: &str) -> Result<SightingMetaMap> {
        let mut stmt = self.conn.prepare(
            "SELECT origin_uri, file_size, file_mtime
             FROM sightings WHERE corpus_id = ?1;",
        )?;
        let rows = stmt.query_map(params![corpus_id], |row| {
            Ok((row.get::<_, String>(0)?, (row.get(1)?, row.get(2)?)))
        })?;

        let mut map = std::collections::HashMap::new();
        for row in rows {
            let (uri, meta) = row?;
            map.insert(uri, meta);
        }
        Ok(map)
    }

    pub fn set_index_state(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO index_state (key, value)
             VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value;",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get_index_state(&self, key: &str) -> Result<Option<String>> {
        let res = self
            .conn
            .query_row(
                "SELECT value FROM index_state WHERE key = ?1;",
                params![key],
                |row| row.get(0),
            )
            .optional()?;
        Ok(res)
    }

    fn row_to_doc(row: &rusqlite::Row) -> rusqlite::Result<Doc> {
        let sha256_blob: Vec<u8> = row.get(1)?;
        let mut sha256 = [0u8; 32];
        if sha256_blob.len() == 32 {
            sha256.copy_from_slice(&sha256_blob);
        }

        let ahash_i64: i64 = row.get(6)?;
        let dhash_i64: i64 = row.get(7)?;
        let phash_i64: i64 = row.get(8)?;
        let whash_i64: i64 = row.get(9)?;

        Ok(Doc {
            doc_id: row.get(0)?,
            sha256,
            byte_size: row.get(2)?,
            format: row.get(3)?,
            width: row.get(4)?,
            height: row.get(5)?,
            ahash: ahash_i64 as u64,
            dhash: dhash_i64 as u64,
            phash: phash_i64 as u64,
            whash: whash_i64 as u64,
            chroma_blob_ref: row.get(10)?,
            keypoints_ref: row.get(11)?,
            indexed_at: row.get(12)?,
        })
    }
}
