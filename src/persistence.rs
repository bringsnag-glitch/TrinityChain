//! Database persistence layer for TrinityChain

use crate::blockchain::{Block, BlockHeader, Blockchain, TriangleState};
use crate::error::ChainError;
use crate::geometry::Triangle;
use crate::mempool::Mempool;
use crate::transaction::Transaction;
use rusqlite::{params, Connection};
use std::sync::Mutex;
use std::collections::HashMap;

/// Abstraction for persistence backends. Implementations should provide
/// atomic saving/loading of blockchain state and blocks.
pub trait Persistence: Send + Sync {
    fn save_blockchain_state(&self, block: &Block, state: &TriangleState, difficulty: u64) -> Result<(), ChainError>;
    fn load_blockchain(&self) -> Result<Blockchain, ChainError>;
    fn save_block(&self, block: &Block) -> Result<(), ChainError>;
    fn save_utxo_set(&self, state: &TriangleState) -> Result<(), ChainError>;
    fn load_utxo_set(&self) -> Result<TriangleState, ChainError>;
    fn save_difficulty(&self, difficulty: u64) -> Result<(), ChainError>;
}

pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    pub fn open(path: &str) -> Result<Self, ChainError> {
        let conn = Connection::open(path)
            .map_err(|e| ChainError::DatabaseError(format!("Failed to open database: {}", e)))?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS blocks (
                height INTEGER PRIMARY KEY,
                hash BLOB NOT NULL,
                previous_hash BLOB NOT NULL,
                timestamp INTEGER NOT NULL,
                difficulty INTEGER NOT NULL,
                nonce INTEGER NOT NULL,
                merkle_root BLOB NOT NULL,
                transactions TEXT NOT NULL
            )",
            [],
        )
        .map_err(|e| ChainError::DatabaseError(format!("Failed to create blocks table: {}", e)))?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS utxo_set (
                hash BLOB PRIMARY KEY,
                triangle_data TEXT NOT NULL
            )",
            [],
        )
        .map_err(|e| {
            ChainError::DatabaseError(format!("Failed to create utxo_set table: {}", e))
        })?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS metadata (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
            [],
        )
        .map_err(|e| {
            ChainError::DatabaseError(format!("Failed to create metadata table: {}", e))
        })?;

        Ok(Database { conn: Mutex::new(conn) })
    }

    pub fn save_block(&self, block: &Block) -> Result<(), ChainError> {
        let transactions_json = serde_json::to_string(&block.transactions).map_err(|e| {
            ChainError::DatabaseError(format!("Failed to serialize transactions: {}", e))
        })?;

        let conn = self.conn.lock().map_err(|_| ChainError::DatabaseError("Mutex poisoned".to_string()))?;
        conn.execute(
            "INSERT OR REPLACE INTO blocks (height, hash, previous_hash, timestamp, difficulty, nonce, merkle_root, transactions)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                block.header.height as i64,
                block.hash().to_vec(),
                block.header.previous_hash.to_vec(),
                block.header.timestamp,
                block.header.difficulty as i64,
                block.header.nonce as i64,
                block.header.merkle_root.to_vec(),
                transactions_json,
            ],
        ).map_err(|e| ChainError::DatabaseError(format!("Failed to save block: {}", e)))?;

        Ok(())
    }

    pub fn save_utxo_set(&self, state: &TriangleState) -> Result<(), ChainError> {
        // Use a transaction for atomic UTXO set update
        let conn_guard = self.conn.lock().map_err(|_| ChainError::DatabaseError("Mutex poisoned".to_string()))?;
        let tx = conn_guard.unchecked_transaction().map_err(|e| {
            ChainError::DatabaseError(format!("Failed to start transaction: {}", e))
        })?;

        tx.execute("DELETE FROM utxo_set", [])
            .map_err(|e| ChainError::DatabaseError(format!("Failed to clear utxo_set: {}", e)))?;

        for (hash, triangle) in &state.utxo_set {
            let triangle_json = serde_json::to_string(triangle).map_err(|e| {
                ChainError::DatabaseError(format!("Failed to serialize triangle: {}", e))
            })?;

            tx.execute(
                "INSERT INTO utxo_set (hash, triangle_data) VALUES (?1, ?2)",
                params![hash.to_vec(), triangle_json],
            )
            .map_err(|e| ChainError::DatabaseError(format!("Failed to save UTXO: {}", e)))?;
        }

        tx.commit().map_err(|e| {
            ChainError::DatabaseError(format!("Failed to commit transaction: {}", e))
        })?;

        Ok(())
    }

    pub fn load_utxo_set(&self) -> Result<TriangleState, ChainError> {
        let mut utxo_set = HashMap::new();

        let conn_guard = self.conn.lock().map_err(|_| ChainError::DatabaseError("Mutex poisoned".to_string()))?;
        let mut stmt = conn_guard
            .prepare("SELECT hash, triangle_data FROM utxo_set")
            .map_err(|e| ChainError::DatabaseError(format!("Failed to prepare query: {}", e)))?;

        let rows = stmt
            .query_map([], |row| {
                let hash_bytes: Vec<u8> = row.get(0)?;
                let triangle_json: String = row.get(1)?;
                Ok((hash_bytes, triangle_json))
            })
            .map_err(|e| ChainError::DatabaseError(format!("Failed to query UTXO set: {}", e)))?;

        for row_result in rows {
            let (hash_bytes, triangle_json) = row_result
                .map_err(|e| ChainError::DatabaseError(format!("Failed to read row: {}", e)))?;

            let mut hash = [0u8; 32];
            hash.copy_from_slice(&hash_bytes);

            let triangle: Triangle = serde_json::from_str(&triangle_json).map_err(|e| {
                ChainError::DatabaseError(format!("Failed to deserialize triangle: {}", e))
            })?;

            utxo_set.insert(hash, triangle);
        }

        Ok(TriangleState {
            utxo_set,
            address_balances: HashMap::new(), // Will be rebuilt by caller
        })
    }

    pub fn save_difficulty(&self, difficulty: u64) -> Result<(), ChainError> {
        let conn = self.conn.lock().map_err(|_| ChainError::DatabaseError("Mutex poisoned".to_string()))?;
        conn.execute(
            "INSERT OR REPLACE INTO metadata (key, value) VALUES ('difficulty', ?1)",
            params![difficulty.to_string()],
        )
        .map_err(|e| ChainError::DatabaseError(format!("Failed to save difficulty: {}", e)))?;

        Ok(())
    }

    /// Atomically saves a block and the associated blockchain state
    /// This ensures database consistency by wrapping all operations in a transaction
    pub fn save_blockchain_state(
        &self,
        block: &Block,
        state: &TriangleState,
        difficulty: u64,
    ) -> Result<(), ChainError> {
        let conn_guard = self.conn.lock().map_err(|_| ChainError::DatabaseError("Mutex poisoned".to_string()))?;
        let tx = conn_guard.unchecked_transaction().map_err(|e| {
            ChainError::DatabaseError(format!("Failed to start transaction: {}", e))
        })?;

        // Save block
        let transactions_json = serde_json::to_string(&block.transactions).map_err(|e| {
            ChainError::DatabaseError(format!("Failed to serialize transactions: {}", e))
        })?;

        tx.execute(
            "INSERT OR REPLACE INTO blocks (height, hash, previous_hash, timestamp, difficulty, nonce, merkle_root, transactions)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                block.header.height as i64,
                block.hash().to_vec(),
                block.header.previous_hash.to_vec(),
                block.header.timestamp,
                block.header.difficulty as i64,
                block.header.nonce as i64,
                block.header.merkle_root.to_vec(),
                transactions_json,
            ],
        ).map_err(|e| ChainError::DatabaseError(format!("Failed to save block: {}", e)))?;

        // Save UTXO set
        tx.execute("DELETE FROM utxo_set", [])
            .map_err(|e| ChainError::DatabaseError(format!("Failed to clear utxo_set: {}", e)))?;

        for (hash, triangle) in &state.utxo_set {
            let triangle_json = serde_json::to_string(triangle).map_err(|e| {
                ChainError::DatabaseError(format!("Failed to serialize triangle: {}", e))
            })?;

            tx.execute(
                "INSERT INTO utxo_set (hash, triangle_data) VALUES (?1, ?2)",
                params![hash.to_vec(), triangle_json],
            )
            .map_err(|e| ChainError::DatabaseError(format!("Failed to save UTXO: {}", e)))?;
        }

        // Save difficulty
        tx.execute(
            "INSERT OR REPLACE INTO metadata (key, value) VALUES ('difficulty', ?1)",
            params![difficulty.to_string()],
        )
        .map_err(|e| ChainError::DatabaseError(format!("Failed to save difficulty: {}", e)))?;

        // Commit all changes atomically
        tx.commit().map_err(|e| {
            ChainError::DatabaseError(format!("Failed to commit transaction: {}", e))
        })?;

        Ok(())
    }

    pub fn load_blockchain(&self) -> Result<Blockchain, ChainError> {
        let conn_guard = self.conn.lock().map_err(|_| ChainError::DatabaseError("Mutex poisoned".to_string()))?;
        let mut stmt = conn_guard.prepare(
            "SELECT height, previous_hash, timestamp, difficulty, nonce, merkle_root, transactions
             FROM blocks ORDER BY height ASC"
        ).map_err(|e| ChainError::DatabaseError(format!("Failed to prepare query: {}", e)))?;

        let blocks_iter = stmt
            .query_map([], |row| {
                let transactions_json: String = row.get(6)?;
                let transactions: Vec<Transaction> = serde_json::from_str(&transactions_json)
                    .map_err(|_e| rusqlite::Error::InvalidQuery)?;

                let height: i64 = row.get(0)?;
                let timestamp: i64 = row.get(2)?;
                let difficulty: i64 = row.get(3)?;
                let nonce: i64 = row.get(4)?;
                let previous_hash_vec: Vec<u8> = row.get(1)?;
                let merkle_root_vec: Vec<u8> = row.get(5)?;

                let mut previous_hash = [0u8; 32];
                previous_hash.copy_from_slice(&previous_hash_vec);
                let mut merkle_root = [0u8; 32];
                merkle_root.copy_from_slice(&merkle_root_vec);

                Ok(Block {
                    header: BlockHeader {
                        height: height as u64,
                        previous_hash,
                        timestamp: timestamp as u64,
                        difficulty: difficulty as u32,
                        nonce: nonce as u64,
                        merkle_root,
                    },
                    transactions,
                })
            })
            .map_err(|e| ChainError::DatabaseError(format!("Failed to query blocks: {}", e)))?;

        let mut blocks = Vec::new();
        for block_result in blocks_iter {
            blocks.push(
                block_result.map_err(|e| {
                    ChainError::DatabaseError(format!("Failed to load block: {}", e))
                })?,
            );
        }

        if blocks.is_empty() {
            return Blockchain::new([0; 32], 0);
        }

        let mut utxo_set = HashMap::new();
        let conn_guard = self.conn.lock().map_err(|_| ChainError::DatabaseError("Mutex poisoned".to_string()))?;
        let mut stmt = conn_guard
            .prepare("SELECT hash, triangle_data FROM utxo_set")
            .map_err(|e| {
                ChainError::DatabaseError(format!("Failed to prepare UTXO query: {}", e))
            })?;

        let utxo_iter = stmt
            .query_map([], |row| {
                let hash_vec: Vec<u8> = row.get(0)?;
                let mut hash = [0u8; 32];
                hash.copy_from_slice(&hash_vec);
                let triangle_json: String = row.get(1)?;
                let triangle: Triangle = serde_json::from_str(&triangle_json)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?;
                Ok((hash, triangle))
            })
            .map_err(|e| ChainError::DatabaseError(format!("Failed to query UTXOs: {}", e)))?;

        for utxo_result in utxo_iter {
            let (hash, triangle) = utxo_result
                .map_err(|e| ChainError::DatabaseError(format!("Failed to load UTXO: {}", e)))?;
            utxo_set.insert(hash, triangle);
        }

        // Load difficulty from metadata, but verify against actual blocks
        let conn_guard = self.conn.lock().map_err(|_| ChainError::DatabaseError("Mutex poisoned".to_string()))?;
        let metadata_difficulty: u32 = conn_guard
            .query_row(
                "SELECT value FROM metadata WHERE key = 'difficulty'",
                [],
                |row| {
                    let val: String = row.get(0)?;
                    Ok(val.parse::<u32>().unwrap_or(2))
                },
            )
            .unwrap_or(2);

        // IMPORTANT: Use the difficulty from the most recent block as source of truth
        // The metadata might be stale due to crashes or non-atomic writes
        let actual_difficulty = blocks
            .last()
            .map(|block| block.header.difficulty)
            .unwrap_or(2);

        // If there's a mismatch, warn and use the actual block difficulty
        let difficulty = if metadata_difficulty != actual_difficulty && !blocks.is_empty() {
            eprintln!("⚠️  Warning: Metadata difficulty ({}) doesn't match last block difficulty ({}). Using block data.",
                      metadata_difficulty, actual_difficulty);
            eprintln!("   Updating metadata to match...");
            // Fix the metadata - errors here are non-critical since we're using actual_difficulty anyway
            if let Err(e) = conn_guard.execute(
                "INSERT OR REPLACE INTO metadata (key, value) VALUES ('difficulty', ?1)",
                params![actual_difficulty.to_string()],
            ) {
                eprintln!("⚠️  Warning: Failed to update difficulty metadata: {}", e);
            }
            actual_difficulty
        } else {
            actual_difficulty
        };

        let mut state = self.load_utxo_set()?;
        state.rebuild_address_balances();

        let blockchain = Blockchain {
            blocks,
            difficulty,
            mempool: Mempool::new(),
            state,
            persistence: Box::new(InMemoryPersistence::new()),
        };

        Ok(blockchain)
    }
}

// Implement the Persistence trait for the rusqlite-backed Database
impl Persistence for Database {
    fn save_blockchain_state(&self, block: &Block, state: &TriangleState, difficulty: u64) -> Result<(), ChainError> {
        Database::save_blockchain_state(self, block, state, difficulty)
    }

    fn load_blockchain(&self) -> Result<Blockchain, ChainError> {
        Database::load_blockchain(self)
    }

    fn save_block(&self, block: &Block) -> Result<(), ChainError> {
        Database::save_block(self, block)
    }

    fn save_utxo_set(&self, state: &TriangleState) -> Result<(), ChainError> {
        Database::save_utxo_set(self, state)
    }

    fn load_utxo_set(&self) -> Result<TriangleState, ChainError> {
        Database::load_utxo_set(self)
    }

    fn save_difficulty(&self, difficulty: u64) -> Result<(), ChainError> {
        Database::save_difficulty(self, difficulty)
    }
}

/// Simple in-memory persistence implementation useful for tests and ephemeral runs.
#[derive(Clone, Default)]
pub struct InMemoryPersistence {
    pub blocks: std::sync::Arc<std::sync::Mutex<Vec<Block>>>,
    pub state: std::sync::Arc<std::sync::Mutex<TriangleState>>,
    pub difficulty: std::sync::Arc<std::sync::Mutex<u32>>,
}

impl InMemoryPersistence {
    pub fn new() -> Self {
        Self {
            blocks: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            state: std::sync::Arc::new(std::sync::Mutex::new(TriangleState::new())),
            difficulty: std::sync::Arc::new(std::sync::Mutex::new(2)),
        }
    }
}

impl Persistence for InMemoryPersistence {
    fn save_blockchain_state(&self, block: &Block, state: &TriangleState, difficulty: u64) -> Result<(), ChainError> {
        let mut blocks = self.blocks.lock().map_err(|_| ChainError::DatabaseError("Mutex poisoned".to_string()))?;
        blocks.retain(|b| b.header.height != block.header.height);
        blocks.push(block.clone());

        let mut st = self.state.lock().map_err(|_| ChainError::DatabaseError("Mutex poisoned".to_string()))?;
        *st = state.clone();

        let mut diff = self.difficulty.lock().map_err(|_| ChainError::DatabaseError("Mutex poisoned".to_string()))?;
        *diff = difficulty as u32;

        Ok(())
    }

    fn load_blockchain(&self) -> Result<Blockchain, ChainError> {
        let blocks = self.blocks.lock().map_err(|_| ChainError::DatabaseError("Mutex poisoned".to_string()))?;
        if blocks.is_empty() {
            return Blockchain::new([0; 32], 2);
        }
        let state = self.state.lock().map_err(|_| ChainError::DatabaseError("Mutex poisoned".to_string()))?;
        let diff = *self.difficulty.lock().map_err(|_| ChainError::DatabaseError("Mutex poisoned".to_string()))?;

        let blockchain = Blockchain {
            blocks: blocks.clone(),
            difficulty: diff,
            mempool: Mempool::new(),
            state: state.clone(),
            persistence: Box::new(self.clone()),
        };
        Ok(blockchain)
    }

    fn save_block(&self, block: &Block) -> Result<(), ChainError> {
        let mut blocks = self.blocks.lock().map_err(|_| ChainError::DatabaseError("Mutex poisoned".to_string()))?;
        blocks.retain(|b| b.header.height != block.header.height);
        blocks.push(block.clone());
        Ok(())
    }

    fn save_utxo_set(&self, state: &TriangleState) -> Result<(), ChainError> {
        let mut st = self.state.lock().map_err(|_| ChainError::DatabaseError("Mutex poisoned".to_string()))?;
        *st = state.clone();
        Ok(())
    }

    fn load_utxo_set(&self) -> Result<TriangleState, ChainError> {
        let st = self.state.lock().map_err(|_| ChainError::DatabaseError("Mutex poisoned".to_string()))?;
        Ok(st.clone())
    }

    fn save_difficulty(&self, difficulty: u64) -> Result<(), ChainError> {
        let mut diff = self.difficulty.lock().map_err(|_| ChainError::DatabaseError("Mutex poisoned".to_string()))?;
        *diff = difficulty as u32;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::Address;

    fn create_test_address(s: &str) -> Address {
        let mut address = [0u8; 32];
        let bytes = s.as_bytes();
        address[..bytes.len()].copy_from_slice(bytes);
        address
    }

    #[test]
    fn test_database_open() {
        let db = Database::open(":memory:").unwrap();
        assert!(db.conn.lock().unwrap().is_autocommit());
    }

    #[test]
    fn test_save_and_load_blockchain() {
        // Use a lightweight difficulty (0) so genesis mining is instantaneous in tests.
        let pers = InMemoryPersistence::new();
        let addr = create_test_address("miner");

        // Create a new blockchain backed by our in-memory persistence with difficulty 0
        let chain = Blockchain::new_with_persistence(addr, 0, Box::new(pers.clone())).expect("create chain");

        // Persisted state should now be loadable from the persistence backend
        let loaded = pers.load_blockchain().expect("load chain");

        assert_eq!(loaded.blocks.len(), chain.blocks.len());
        assert_eq!(loaded.difficulty, chain.difficulty);
    }
}
