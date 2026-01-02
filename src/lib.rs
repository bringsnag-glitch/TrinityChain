//! TrinityChain - A geometric blockchain where value is represented as triangles
//!
//! # Architecture
//!
//! The crate is organized into logical modules:
//!
//! ## Core Blockchain
//! - [`blockchain`] - Main blockchain logic and validation
//! - [`transaction`] - Transaction types and operations
//! - [`block`] - Block structure and validation
//! - [`mempool`] - Transaction mempool
//!
//! ## Geometric System  
//! - [`geometry`] - Triangle primitives and calculations
//! - [`fees`] - Fee calculations (geometric)
//!
//! ## Consensus
//! - [`miner`] - Proof-of-work mining
//!
//! ## Cryptography
//! - [`crypto`] - Signatures and verification (secp256k1)
//! - [`security`] - Security utilities
//!
//! ## State Management
//! - [`wallet`] - Wallet operations and UTXO selection
//! - [`hdwallet`] - HD wallet (BIP-39/BIP-32)
//! - [`persistence`] - Database layer (SQLite)
//! - [`cache`] - Caching utilities
//!
//! ## Networking & Integration
//! - [`network`] - P2P networking
//! - [`discovery`] - Peer discovery
//! - [`sync`] - Chain synchronization
//!
//! ## Configuration & Utilities
//! - [`config`] - Configuration management
//! - [`error`] - Error types
//! - [`cli`] - CLI utilities
//! - [`addressbook`] - Address book management

#![forbid(unsafe_code)]

// ============================================================================
// Core Blockchain
// ============================================================================
pub mod blockchain;
pub mod mempool;
pub mod transaction;

// ============================================================================
// Geometric System
// ============================================================================
pub mod fees;
pub mod geometry;

// ============================================================================
// Consensus & Mining
// ============================================================================
pub mod miner;

// ============================================================================
// Cryptography & Security
// ============================================================================
pub mod crypto;
pub mod security;

// ============================================================================
// State Management
// ============================================================================
pub mod cache;
pub mod hdwallet;
pub mod persistence;
pub mod wallet;

// ============================================================================
// Networking
// ============================================================================
pub mod discovery;
pub mod network;
pub mod sync;

// ============================================================================
// Integration
// ============================================================================
#[cfg(feature = "api")]
pub mod api;

// ============================================================================
// Configuration & Utilities
// ============================================================================
pub mod addressbook;
pub mod cli;
pub mod config;
pub mod error;
pub mod node;
