#![forbid(unsafe_code)]
//! TrinityChain node launcher

use trinitychain::node::Node;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize the authoritative node orchestrator and run it.
    let node = Node::init().await?;
    let node = std::sync::Arc::new(node);
    node.start().await?;
    Ok(())
}
