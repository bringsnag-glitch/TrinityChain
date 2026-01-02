#![forbid(unsafe_code)]
//! Miner CLI for TrinityChain - Clean TUI edition!

use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block as TuiBlock, Borders, Gauge, Paragraph, Sparkline},
    Terminal,
};
use std::env;
use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};
use tokio::time::sleep;
use trinitychain::blockchain::{Block, Blockchain};
use trinitychain::network::NetworkNode;
use trinitychain::persistence::Database;
use trinitychain::transaction::{CoinbaseTx, Transaction};

#[derive(Clone)]
struct MiningStats {
    blocks_mined: u64,
    chain_height: u64,
    uptime_secs: u64,
    avg_block_time: f64,
    difficulty: u64,
    total_earned: f64,
    max_supply: u64,
    blocks_to_halving: u64,
    halving_era: u64,
    current_hash_rate: f64,
    mining_status: String,
    network_peers: usize,
    last_block_hash: String,
    last_block_time: f64,
    recent_blocks: Vec<(u64, String, String)>, // (height, hash, parent_hash)
    hashrate_history: Vec<u64>,                // Last 20 hashrate samples
}

impl Default for MiningStats {
    fn default() -> Self {
        Self {
            blocks_mined: 0,
            chain_height: 0,
            uptime_secs: 0,
            avg_block_time: 0.0,
            difficulty: 1,
            total_earned: 0.0,
            max_supply: 420_000_000,
            blocks_to_halving: 210_000,
            halving_era: 0,
            current_hash_rate: 0.0,
            mining_status: "Starting...".to_string(),
            network_peers: 0,
            last_block_hash: "N/A".to_string(),
            last_block_time: 0.0,
            recent_blocks: Vec::new(),
            hashrate_history: vec![0; 20],
        }
    }
}

fn format_number(num: u64) -> String {
    let num_str = num.to_string();
    let mut result = String::new();
    let chars: Vec<char> = num_str.chars().collect();

    for (i, &ch) in chars.iter().enumerate() {
        if i > 0 && (chars.len() - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(ch);
    }
    result
}

fn format_hash(hash: &str) -> String {
    if hash.len() > 20 {
        format!("{}...{}", &hash[..10], &hash[hash.len() - 10..])
    } else {
        hash.to_string()
    }
}

fn draw_ui(f: &mut ratatui::Frame, stats: &MiningStats, beneficiary: &str) {
    let size = f.size();

    // Create main layout
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3),  // Title
            Constraint::Length(8),  // Mining Status (increased for network peers)
            Constraint::Length(10), // Stats
            Constraint::Length(6),  // Supply Progress
            Constraint::Length(6),  // Hashrate Graph
            Constraint::Length(12), // Blockchain Tree
            Constraint::Min(0),     // Bottom padding
        ])
        .split(size);

    // Title - Centered and bold
    let title = Paragraph::new(vec![Line::from(vec![
        Span::styled("⛏️   ", Style::default().fg(Color::Yellow)),
        Span::styled(
            "TRINITY CHAIN MINER",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("   ⛏️", Style::default().fg(Color::Yellow)),
    ])])
    .block(
        TuiBlock::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    )
    .style(Style::default().fg(Color::White))
    .alignment(Alignment::Center);
    f.render_widget(title, chunks[0]);

    // Mining Status Box
    let status_text = vec![
        Line::from(vec![
            Span::styled("Status: ", Style::default().fg(Color::Gray)),
            Span::styled(
                &stats.mining_status,
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Beneficiary: ", Style::default().fg(Color::Gray)),
            Span::styled(format_hash(beneficiary), Style::default().fg(Color::Yellow)),
        ]),
        Line::from(vec![
            Span::styled("Hashrate: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{:.2} H/s", stats.current_hash_rate),
                Style::default().fg(Color::Magenta),
            ),
        ]),
        Line::from(vec![
            Span::styled("Last Block: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format_hash(&stats.last_block_hash),
                Style::default().fg(Color::Green),
            ),
        ]),
        Line::from(vec![
            Span::styled("Last Block Time: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{:.2}s", stats.last_block_time),
                Style::default().fg(Color::Blue),
            ),
        ]),
        Line::from(vec![
            Span::styled("Network Peers: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{}", stats.network_peers),
                Style::default().fg(Color::Cyan),
            ),
        ]),
    ];
    let status = Paragraph::new(status_text).block(
        TuiBlock::default()
            .borders(Borders::ALL)
            .title("⚡ Mining Status")
            .border_style(Style::default().fg(Color::Green)),
    );
    f.render_widget(status, chunks[1]);

    // Stats Box - Bigger numbers
    let stats_text = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("     Blocks Mined: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!(" {} ", stats.blocks_mined),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            ),
            Span::styled("   Chain Height: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!(" {} ", stats.chain_height),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("     Total Earned: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!(" {:.0} TRC ", stats.total_earned),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("     Difficulty: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{}", stats.difficulty),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::styled("  │  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Reward: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{} TRC", 1000 / (1 << stats.halving_era)),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  │  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Era: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{}", stats.halving_era),
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
    ];
    let stats_widget = Paragraph::new(stats_text).block(
        TuiBlock::default()
            .borders(Borders::ALL)
            .title("📊 Statistics")
            .border_style(Style::default().fg(Color::Blue)),
    );
    f.render_widget(stats_widget, chunks[2]);

    // Supply Progress
    let gauge = Gauge::default()
        .block(
            TuiBlock::default()
                .borders(Borders::ALL)
                .title("💎 Token Supply Progress")
                .border_style(Style::default().fg(Color::Magenta)),
        )
        .gauge_style(Style::default().fg(Color::Magenta).bg(Color::Black))
        .label(format!(
            "{} / {}",
            format_number(stats.chain_height * 1000),
            format_number(stats.max_supply)
        ));
    f.render_widget(gauge, chunks[3]);

    // Hashrate Graph
    let hashrate_sparkline = Sparkline::default()
        .block(
            TuiBlock::default()
                .borders(Borders::ALL)
                .title(format!(
                    "⚡ Hashrate Monitor: {:.2} H/s (Last 20 samples)",
                    stats.current_hash_rate
                ))
                .border_style(Style::default().fg(Color::Yellow)),
        )
        .data(&stats.hashrate_history)
        .style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
    f.render_widget(hashrate_sparkline, chunks[4]);

    // Blockchain Tree - Real Parent-Child Relationships
    let mut tree_lines = vec![Line::from("")];

    if stats.recent_blocks.is_empty() {
        tree_lines.push(Line::from(vec![Span::styled(
            "   Waiting for blocks...",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        )]));
    } else {
        // Show last 5 blocks in tree format
        let blocks_to_show = stats.recent_blocks.iter().rev().take(5).collect::<Vec<_>>();

        for (i, (height, hash, parent_hash)) in blocks_to_show.iter().enumerate() {
            let is_latest = i == 0;
            let color = if is_latest {
                Color::Green
            } else if i == 1 {
                Color::Cyan
            } else {
                Color::Gray
            };

            // Block node
            tree_lines.push(Line::from(vec![
                Span::styled("      ", Style::default()),
                Span::styled("▲", Style::default().fg(color).add_modifier(Modifier::BOLD)),
                Span::styled(
                    format!(" #{}", height),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
            ]));

            // Hash
            tree_lines.push(Line::from(vec![
                Span::styled("     ", Style::default()),
                Span::styled("╱ ╲", Style::default().fg(color)),
                Span::styled(
                    format!("  {}", format_hash(hash)),
                    Style::default().fg(color),
                ),
            ]));

            if i < blocks_to_show.len() - 1 {
                // Connection to parent
                tree_lines.push(Line::from(vec![Span::styled(
                    "      │",
                    Style::default().fg(Color::DarkGray),
                )]));
                tree_lines.push(Line::from(vec![
                    Span::styled("      │", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format!("  parent: {}", format_hash(parent_hash)),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
            }
        }
    }

    let tree = Paragraph::new(tree_lines).block(
        TuiBlock::default()
            .borders(Borders::ALL)
            .title("🌳 Blockchain Tree (Parent → Child)")
            .border_style(Style::default().fg(Color::Magenta)),
    );
    f.render_widget(tree, chunks[5]);

    // Footer
    let footer_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[6]);

    let help = Paragraph::new(vec![Line::from(vec![
        Span::styled("Press ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            "'q'",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" to quit", Style::default().fg(Color::DarkGray)),
    ])]);
    f.render_widget(help, footer_chunks[0]);
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Enforce that the authoritative node runs mining by default.
    if std::env::var("TRINITY_STANDALONE").unwrap_or_default() != "1" {
        eprintln!("This miner binary should be managed by `trinity-node`. To run standalone, set TRINITY_STANDALONE=1");
        return Ok(());
    }
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Usage: trinity-miner <beneficiary_address> [--threads <N>]");
        return Ok(());
    }
    let beneficiary_address = args[1].clone();

    let mut threads: usize = 1;
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--threads" || args[i] == "-t" {
            if i + 1 < args.len() {
                if let Ok(n) = args[i + 1].parse::<usize>() {
                    threads = n.max(1);
                }
            }
            i += 2;
        } else {
            i += 1;
        }
    }

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let stats = Arc::new(Mutex::new(MiningStats::default()));
    let stats_clone = Arc::clone(&stats);
    let beneficiary_clone = beneficiary_address.clone();

    // Create and start network node
    let db_for_network = Database::open("trinitychain.db").expect("Failed to open database");
    let chain_for_network = db_for_network
        .load_blockchain()
        .unwrap_or_else(|_| Blockchain::new([0; 32], 1).unwrap());
    let network = Arc::new(NetworkNode::new(Arc::new(RwLock::new(chain_for_network))));
    let network_clone = network.clone();

    // Start network server in background
    tokio::spawn(async move {
        let port = 8333; // Default P2P port
        println!("🌐 Starting P2P network on port {}...", port);
        if let Err(e) = network_clone.start_server(port).await {
            eprintln!("❌ Network error: {}", e);
        }
    });

    // Spawn mining task
    let mining_handle = tokio::spawn(async move {
        mining_loop(beneficiary_clone, threads, stats_clone, Some(network)).await;
    });

    // UI loop
    loop {
        // Check for quit key
        if event::poll(Duration::from_millis(100)).unwrap_or(false) {
            if let Event::Key(key) = event::read().unwrap() {
                if key.code == KeyCode::Char('q') {
                    break;
                }
            }
        }

        // Draw UI
        let stats_lock = stats.lock().await.clone();
        terminal
            .draw(|f| {
                draw_ui(f, &stats_lock, &beneficiary_address);
            })
            .ok();

        tokio::time::sleep(Duration::from_millis(250)).await;
    }

    // Cleanup
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    mining_handle.abort();

    Ok(())
}

async fn mining_loop(
    beneficiary_address: String,
    _threads: usize,
    stats: Arc<Mutex<MiningStats>>,
    network: Option<Arc<NetworkNode>>,
) {
    let db = Database::open("trinitychain.db").expect("Failed to open database");
    let mut chain = db
        .load_blockchain()
        .unwrap_or_else(|_| Blockchain::new([0; 32], 1).unwrap());

    let start_time = Instant::now();
    let mut blocks_mined = 0;

    loop {
        chain = db.load_blockchain().unwrap_or_else(|_| chain.clone());

        let last_block = match chain.blocks.last() {
            Some(block) => block,
            None => {
                sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        let new_height = last_block.header.height + 1;
        let difficulty = chain.difficulty;

        let mut address = [0u8; 32];
        hex::decode_to_slice(&beneficiary_address, &mut address).unwrap();
        let coinbase_tx = Transaction::Coinbase(CoinbaseTx {
            reward_area: trinitychain::geometry::Coord::from_num(1000),
            beneficiary_address: address,
            nonce: 0,
        });

        let mut new_block =
            Block::new(new_height, last_block.hash(), difficulty, vec![coinbase_tx]);

        if new_block.header.timestamp <= last_block.header.timestamp {
            new_block.header.timestamp = last_block.header.timestamp + 1;
        }

        // Update status
        {
            let mut s = stats.lock().await;
            s.mining_status = format!("Mining block #{}...", new_height);
            s.difficulty = difficulty as u64;
        }

        let mine_start = Instant::now();
        let new_block = match trinitychain::miner::mine_block(new_block) {
            Ok(b) => b,
            Err(_) => {
                sleep(Duration::from_secs(1)).await;
                continue;
            }
        };
        let mine_duration = mine_start.elapsed().as_secs_f64();
        let hash_hex = hex::encode(new_block.hash());

        if chain.apply_block(new_block.clone()).is_err() {
            sleep(Duration::from_secs(10)).await;
            continue;
        }

        // Broadcast block to network
        if let Some(ref network) = network {
            network.broadcast_block(&new_block).await;
        }

        if let Err(_e) = db.save_blockchain_state(&new_block, &chain.state, chain.difficulty as u64)
        {
            // Handle error silently
        }

        blocks_mined += 1;
        let elapsed = start_time.elapsed();

        // Update stats
        {
            let current_height = new_height;
            let halving_era = current_height / 210_000;
            let blocks_to_halving = ((halving_era + 1) * 210_000).saturating_sub(current_height);

            let parent_hash_hex = hex::encode(new_block.header.previous_hash);

            let mut s = stats.lock().await;
            s.blocks_mined = blocks_mined;
            s.chain_height = current_height;
            s.uptime_secs = elapsed.as_secs();
            s.avg_block_time = elapsed.as_secs_f64() / blocks_mined as f64;
            s.total_earned = blocks_mined as f64 * 1000.0;
            s.blocks_to_halving = blocks_to_halving;
            s.halving_era = halving_era;
            s.mining_status = format!("✓ Block #{} mined!", new_height);
            s.last_block_hash = hash_hex.clone();
            s.last_block_time = mine_duration;

            // Add to blockchain tree
            s.recent_blocks
                .push((current_height, hash_hex, parent_hash_hex));
            // Keep only last 10 blocks
            if s.recent_blocks.len() > 10 {
                s.recent_blocks.remove(0);
            }
        }

        sleep(Duration::from_millis(500)).await;
    }
}
