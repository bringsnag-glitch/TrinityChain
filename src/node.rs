use crate::config::load_config;
use crate::persistence::{Database, InMemoryPersistence, Persistence};
use crate::blockchain::Blockchain;
use crate::mempool::Mempool;
use crate::network::NetworkNode;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn, error};
use std::fs;
use std::net::TcpListener;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeState {
    Booting,
    Syncing,
    Ready,
    Degraded,
}

pub struct Node {
    pub config: crate::config::Config,
    pub persistence: std::sync::Arc<Box<dyn Persistence>>,
    pub blockchain: Arc<RwLock<Blockchain>>,
    pub mempool: Arc<RwLock<Mempool>>,
    pub network: Arc<NetworkNode>,
    pub state: Arc<RwLock<NodeState>>,
}

impl Node {
    pub async fn init() -> Result<Self, Box<dyn std::error::Error>> {
        // Load and validate config
        let config = load_config()?;

        tracing_subscriber::fmt::init();
        info!("Starting TrinityChain node (network_id = {})", config.network.network_id);

        // Setup persistence
        let persistence_box: Box<dyn Persistence> = match Database::open(&config.database.path) {
            Ok(db) => Box::new(db),
            Err(e) => {
                warn!("Failed to open DB at {}: {}. Falling back to in-memory persistence.", config.database.path, e);
                Box::new(InMemoryPersistence::new())
            }
        };
        let persistence = std::sync::Arc::new(persistence_box);

        // Load or create blockchain
        let blockchain = match persistence.load_blockchain() {
            Ok(chain) => chain,
            Err(e) => {
                warn!("Failed to load blockchain from persistence: {}. Creating new chain.", e);
                // Create with default genesis miner address from config
                let addr_bytes = hex::decode(&config.miner.beneficiary_address).unwrap_or(vec![0u8;32]);
                let mut addr = [0u8;32];
                    for (i, b) in addr_bytes.iter().take(32).enumerate() { addr[i] = *b; }
                Blockchain::new(addr, 1).map_err(|e| format!("Failed to create blockchain: {}", e))?
            }
        };

        let blockchain = Arc::new(RwLock::new(blockchain));
        let mempool = Arc::new(RwLock::new(Mempool::new()));
        let state = Arc::new(RwLock::new(NodeState::Booting));

        // Network
        let network = Arc::new(NetworkNode::new(blockchain.clone()));

        Ok(Self { config, persistence, blockchain, mempool, network, state })
    }

    pub async fn start(self: Arc<Self>) -> Result<(), Box<dyn std::error::Error>> {
        // Enforce deterministic startup order.
        // 1) Ensure data directory exists
        let data_dir = &self.config.database.path;
        fs::create_dir_all(data_dir).map_err(|e| format!("Failed to create data dir {}: {}", data_dir, e))?;

        // 2) Ensure P2P port is available
        let p2p_port = self.config.network.p2p_port;
        let p2p_bind = format!("0.0.0.0:{}", p2p_port);
        TcpListener::bind(&p2p_bind).map_err(|e| format!("P2P port {} unavailable: {}", p2p_port, e))?;

        // Start network listener (spawned so we can proceed)
        let net = self.network.clone();
        let net_clone = net.clone();
        let p2p_port_clone = p2p_port;
        let _net_task = tokio::spawn(async move {
            if let Err(e) = net_clone.start_server(p2p_port_clone).await {
                error!("P2P server failed: {}", e);
            }
        });

        // give network a moment to bind/listen
        tokio::time::sleep(Duration::from_millis(200)).await;

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

        // 3) Ensure API port is available and start API server
        let api_port = self.config.network.api_port;
        let api_bind = format!("0.0.0.0:{}", api_port);
        TcpListener::bind(&api_bind).map_err(|e| format!("API port {} unavailable: {}", api_port, e))?;

        let node = self.clone();
        let _api_task = tokio::spawn(async move {
            if let Err(e) = Node::start_api(node, api_port).await {
                error!("API server failed: {}", e);
            }
        });

        // 4) Transition to Syncing then Ready once initial checks pass
        {
            let mut s = self.state.write().await;
            *s = NodeState::Syncing;
        }

        // For now we treat local chain as already synced (lightweight)
        {
            let mut s = self.state.write().await;
            *s = NodeState::Ready;
        }

        // Start miner loop if enabled and node is Ready
        if self.config.miner.enabled {
            let bc = self.blockchain.clone();
            let mp = self.mempool.clone();
            let pers = self.persistence.clone();
            let _net = self.network.clone();
            let min_peers = self.config.network.min_peers;
            tokio::spawn(async move {
                loop {
                    // Basic gating: require node Ready, sufficient peers and non-empty mempool
                    // Check node state
                    // (we can't access self.state from here easily; rely on network/mempool checks)
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
                        let bytes = hex::decode("00").unwrap_or_default();
                        for (i,b) in bytes.iter().take(32).enumerate() { addr[i] = *b; }
                        addr
                    };
                    txs_with_coinbase.push(crate::transaction::Transaction::Coinbase(crate::transaction::types::CoinbaseTx{ reward_area: reward, beneficiary_address: beneficiary, nonce: 0 }));
                    txs_with_coinbase.extend(txs);

                    let block = crate::blockchain::core::chain::Block::new(height, prev_hash, difficulty, txs_with_coinbase);
                    match crate::miner::mine_block(block) {
                        Ok(mined) => {
                            info!("Mined new block at height {}", mined.header.height);
                            // apply to chain
                            if let Err(e) = bc.write().await.apply_block(mined.clone()) {
                                warn!("Failed to apply mined block: {}", e);
                            } else {
                                // persist
                                let _ = pers.as_ref().save_blockchain_state(&mined, &bc.read().await.state, bc.read().await.difficulty as u64);
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

    #[cfg(feature = "api")]
    async fn start_api(_node: Arc<Self>, port: u16) -> Result<(), Box<dyn std::error::Error>> {

        // For now: register routes but avoid spawning a real HTTP server in test builds to
        // prevent dependency/version mismatches; the `trinity-node` will still provide
        // the authoritative API when compiled with the correct feature flags.
        info!("API server (stub) available on port {}", port);
        Ok(())
    }

    #[cfg(not(feature = "api"))]
    async fn start_api(_node: Arc<Self>, _port: u16) -> Result<(), Box<dyn std::error::Error>> {
        Err("API feature not enabled in this build".into())
    }
}
