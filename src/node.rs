use crate::config::load_config;
use crate::persistence::{Database, InMemoryPersistence, Persistence};
use crate::blockchain::Blockchain;
use crate::mempool::Mempool;
use crate::network::NetworkNode;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn, error};
use axum::{routing::get, Router, Json};
use serde_json::json;

pub struct Node {
    pub config: crate::config::Config,
    pub persistence: Box<dyn Persistence>,
    pub blockchain: Arc<RwLock<Blockchain>>,
    pub mempool: Arc<RwLock<Mempool>>,
    pub network: Arc<NetworkNode>,
}

impl Node {
    pub async fn init() -> Result<Self, Box<dyn std::error::Error>> {
        // Load and validate config
        let config = load_config()?;

        tracing_subscriber::fmt::init();
        info!("Starting TrinityChain node (network_id = {})", config.network.network_id);

        // Setup persistence
        let persistence: Box<dyn Persistence> = match Database::open(&config.database.path) {
            Ok(db) => Box::new(db),
            Err(e) => {
                warn!("Failed to open DB at {}: {}. Falling back to in-memory persistence.", config.database.path, e);
                Box::new(InMemoryPersistence::new())
            }
        };

        // Load or create blockchain
        let blockchain = match persistence.load_blockchain() {
            Ok(chain) => chain,
            Err(e) => {
                warn!("Failed to load blockchain from persistence: {}. Creating new chain.", e);
                // Create with default genesis miner address from config
                let addr_bytes = hex::decode(&config.miner.beneficiary_address).unwrap_or(vec![0u8;32]);
                let mut addr = [0u8;32];
                for (i, b) in addr_bytes.iter().take(32).enumerate() { addr[i]=*b; }
                Blockchain::new(addr, 1).map_err(|e| format!("Failed to create blockchain: {}", e))?
            }
        };

        let blockchain = Arc::new(RwLock::new(blockchain));
        let mempool = Arc::new(RwLock::new(Mempool::new()));

        // Network
        let network = Arc::new(NetworkNode::new(blockchain.clone()));

        Ok(Self { config, persistence, blockchain, mempool, network })
    }

    pub async fn start(self: Arc<Self>) -> Result<(), Box<dyn std::error::Error>> {
        // Start network listener
        let p2p_port = self.config.network.p2p_port;
        let net = self.network.clone();
        tokio::spawn(async move {
            if let Err(e) = net.clone().start_server(p2p_port).await {
                error!("P2P server failed: {}", e);
            }
        });

        // Bootstrap peers
        for peer in &self.config.network.bootstrap_peers {
            let parts: Vec<&str> = peer.split(':').collect();
            if parts.len() == 2 {
                let host = parts[0].to_string();
                if let Ok(port) = parts[1].parse::<u16>() {
                    let net2 = self.network.clone();
                    tokio::spawn(async move {
                        let _ = net2.connect_peer(host, port).await;
                    });
                }
            }
        }

        // Start API server
        let api_port = self.config.network.api_port;
        let node = self.clone();
        tokio::spawn(async move {
            if let Err(e) = Node::start_api(node, api_port).await {
                error!("API server failed: {}", e);
            }
        });

        // Start miner loop if enabled
        if self.config.miner.enabled {
            let bc = self.blockchain.clone();
            let mp = self.mempool.clone();
            let pers = self.persistence.as_ref();
            let net = self.network.clone();
            let min_peers = self.config.network.min_peers;
            tokio::spawn(async move {
                loop {
                    // Basic gating: require sufficient peers and non-empty mempool
                    let peer_count = 0usize; // best-effort; network exposes peer listing elsewhere
                    if peer_count < min_peers as usize {
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        continue;
                    }

                    if mp.read().await.is_empty() {
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        continue;
                    }

                    // Create a candidate block
                    let (height, prev_hash, difficulty) = {
                        let chain = bc.read().await;
                        let last = chain.blocks.last();
                        let height = last.map(|b| b.header.height + 1).unwrap_or(0);
                        let prev_hash = last.map(|b| b.hash()).unwrap_or([0u8;32]);
                        let difficulty = chain.difficulty;
                        (height, prev_hash, difficulty)
                    };

                    let txs = mp.read().await.get_transactions_by_fee(50);
                    let mut txs_with_coinbase = vec![];
                    // coinbase reward area: small constant for dev mining
                    let reward = crate::geometry::Coord::from_num(1.0);
                    let beneficiary = {
                        let mut addr = [0u8;32];
                        let bytes = hex::decode(&"00").unwrap_or_default();
                        for (i,b) in bytes.iter().take(32).enumerate() { addr[i]=*b; }
                        addr
                    };
                    txs_with_coinbase.push(crate::transaction::Transaction::Coinbase(crate::transaction::types::CoinbaseTx{ reward_area: reward, beneficiary_address: beneficiary, nonce: 0 }));
                    txs_with_coinbase.extend(txs);

                    let mut block = crate::blockchain::core::chain::Block::new(height, prev_hash, difficulty, txs_with_coinbase);
                    match crate::miner::mine_block(block) {
                        Ok(mined) => {
                            info!("Mined new block at height {}", mined.header.height);
                            // apply to chain
                            if let Err(e) = bc.write().await.apply_block(mined.clone()) {
                                warn!("Failed to apply mined block: {}", e);
                            } else {
                                // persist
                                let _ = pers.save_blockchain_state(&mined, &bc.read().await.state, bc.read().await.difficulty as u64);
                            }
                        }
                        Err(e) => {
                            warn!("Mining failed: {}", e);
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            });
        }

        // Node main loop - health logging
        loop {
            info!("Node running: chain height = {}", self.blockchain.read().await.blocks.len());
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        }
    }

    async fn start_api(node: Arc<Self>, port: u16) -> Result<(), Box<dyn std::error::Error>> {
        let blockchain = node.blockchain.clone();
        let mempool = node.mempool.clone();
        let network = node.network.clone();

        let app = Router::new()
            .route("/status", get(move || {
                let bc = blockchain.clone();
                async move {
                    let height = bc.read().await.blocks.len();
                    Json(json!({"status":"ok","chain_height":height}))
                }
            }))
            .route("/peers", get(move || {
                let net = network.clone();
                async move {
                    // Return peer list (best-effort)
                    Json(json!({"peers":[] as Vec<String>}))
                }
            }))
            .route("/chain", get(move || {
                let bc = blockchain.clone();
                async move {
                    let headers: Vec<_> = bc.read().await.blocks.iter().map(|b| {
                        json!({"height": b.header.height, "hash": hex::encode(b.hash())})
                    }).collect();
                    Json(json!({"headers": headers}))
                }
            }))
            .route("/mempool", get(move || {
                let mp = mempool.clone();
                async move {
                    let txs = mp.read().await.transactions.len();
                    Json(json!({"tx_count": txs}))
                }
            }))
            .route("/mine/control", get(move || {
                Json(json!({"mining":"controlled"}))
            }));

        let addr = std::net::SocketAddr::from(([0,0,0,0], port));
        info!("Starting API server on {}", addr);
        axum::Server::bind(&addr).serve(app.into_make_service()).await?;
        Ok(())
    }
}
