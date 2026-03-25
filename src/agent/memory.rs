use super::MemoryStore;
use rusqlite::{params, Connection, Result};

impl MemoryStore {
    /// Creates a new MemoryStore connected to the given database name.
    /// If an empty string or ":memory:" is provided, it creates an in-memory database.
    pub fn new(db_name: &str) -> Result<Self> {
        // Register the sqlite-vec extension for future connections
        unsafe {
            rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
                sqlite_vec::sqlite3_vec_init as *const (),
            )));
        }

        let conn = if db_name.is_empty() || db_name == ":memory:" {
            Connection::open_in_memory()?
        } else {
            let db_path = std::path::PathBuf::from(&*crate::globals::CONFIG_DIR).join(db_name);
            Connection::open(db_path)?
        };

        // Initialize the tables
        conn.execute(
            "CREATE TABLE IF NOT EXISTS memories (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                text TEXT NOT NULL
            )",
            [],
        )?;

        // Initialize the vector virtual table
        // We assume 4096 as the default embedding size (e.g. for OpenAI models)
        conn.execute(
            "CREATE VIRTUAL TABLE IF NOT EXISTS memories_vec USING vec0(
                embedding float[4096] distance_metric=cosine
            )", // TODO: make embedding dim configurable, currently hard-coded to qwen3-embedding-8b
            [],
        )?;

        Ok(MemoryStore { conn })
    }

    /// Adds a new memory with its associated text and embedding vector.
    /// Returns the ID of the newly inserted memory.
    pub fn add_memory(&self, text: &str, embedding: &[f32]) -> Result<i64> {
        // Insert into the normal table
        self.conn
            .execute("INSERT INTO memories (text) VALUES (?1)", params![text])?;

        let id = self.conn.last_insert_rowid();

        // `sqlite-vec` stores vectors natively as a continuous slice of bytes.
        // `bytemuck` safely casts our `&[f32]` array into `&[u8]` so `rusqlite` can bind it as a BLOB.
        let bytes: &[u8] = bytemuck::cast_slice(embedding);

        self.conn.execute(
            "INSERT INTO memories_vec (rowid, embedding) VALUES (?1, ?2)",
            params![id, bytes],
        )?;

        Ok(id)
    }

    /// Removes a memory by its ID.
    pub fn remove_memory(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM memories WHERE id = ?1", params![id])?;

        self.conn
            .execute("DELETE FROM memories_vec WHERE rowid = ?1", params![id])?;

        Ok(())
    }

    /// Searches for `top_k` closest memories to the provided embedding vector.
    /// Returns a list of matching memory texts along with their cosine similarities.
    pub fn search_memory(
        &self,
        embedding: &[f32],
        top_k: usize,
    ) -> Result<Vec<(i64, String, f32)>> {
        let bytes: &[u8] = bytemuck::cast_slice(embedding);

        let mut stmt = self.conn.prepare(
            r#"
            SELECT 
                memories.id,
                memories.text,
                (1.0 - vec_distance_cosine(memories_vec.embedding, ?1)) AS similarity
            FROM memories_vec
            JOIN memories ON memories.id = memories_vec.rowid
            WHERE memories_vec.embedding MATCH ?1
                AND k = ?2
            ORDER BY similarity DESC
            "#,
        )?;

        let results = stmt
            .query_map(params![bytes, top_k as i64], |row| {
                let id: i64 = row.get(0)?;
                let text: String = row.get(1)?;
                let similarity: f32 = row.get(2)?;
                Ok((id, text, similarity))
            })?
            .collect::<Result<Vec<_>>>()?;

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_memory() {
        let store = MemoryStore::new(":memory:").expect("Failed to create memory store");
        let dummy_embedding = vec![0.1f32; 4096];

        let id1 = store
            .add_memory(
                "The quick brown fox jumps over the lazy dog",
                &dummy_embedding,
            )
            .expect("Failed to add memory 1");

        let id2 = store
            .add_memory("Lorem ipsum dolor sit amet", &dummy_embedding)
            .expect("Failed to add memory 2");

        assert_eq!(id1, 1);
        assert_eq!(id2, 2);

        let count: i64 = store
            .conn
            .query_row("SELECT count(*) FROM memories", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 2);

        let count_vec: i64 = store
            .conn
            .query_row("SELECT count(*) FROM memories_vec", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count_vec, 2);
    }

    #[test]
    fn test_remove_memory() {
        let store = MemoryStore::new(":memory:").expect("Failed to create memory store");
        let dummy_embedding = vec![0.1f32; 4096];

        store
            .add_memory("Memory to retain", &dummy_embedding)
            .expect("Failed to add memory 1");

        let id2 = store
            .add_memory("Memory to remove", &dummy_embedding)
            .expect("Failed to add memory 2");

        store.remove_memory(id2).expect("Failed to remove memory");

        let count: i64 = store
            .conn
            .query_row("SELECT count(*) FROM memories", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);

        let count_vec: i64 = store
            .conn
            .query_row("SELECT count(*) FROM memories_vec", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count_vec, 1);
    }

    #[test]
    fn test_search_memory() {
        let store = MemoryStore::new(":memory:").expect("Failed to create memory store");
        let dummy_embedding = vec![0.1f32; 4096];

        store
            .add_memory("Lorem ipsum dolor sit amet", &dummy_embedding)
            .expect("Failed to add memory");

        let results = store
            .search_memory(&dummy_embedding, 5)
            .expect("Search failed");
        assert_eq!(results.len(), 1);

        let (id, text, similarity) = &results[0];
        assert_eq!(*id, 1);
        assert_eq!(text, "Lorem ipsum dolor sit amet");
        assert!(*similarity > 0.99);
    }
}
