use sha2::{Digest, Sha256};
use serde::{Deserialize, Serialize};

/// Content-addressed hash for triangle ID
pub type TriangleId = [u8; 32];

/// Deterministic hash function
pub fn hash_data(data: &[u8]) -> TriangleId {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

/// Represents a triangle in the fractal state machine.
/// A triangle is either whole (no children) or split into exactly three children.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Triangle {
    /// Content-addressed ID (hash of the triangle's deterministic fields)
    pub id: TriangleId,
    /// Integer level (depth in the fractal)
    pub level: u64,
    /// Parent triangle ID (None if level == 0)
    pub parent_id: Option<TriangleId>,
    /// Children IDs (exactly 3 if split, empty if whole)
    pub children_ids: Vec<TriangleId>,
    /// Deterministic hash of contained state
    pub state_root: TriangleId,
    /// Conserved scalar quantity
    pub energy: u64,
    /// Derived cohesion score
    pub cohesion_score: u64,
}

impl Triangle {
    /// Create a new root triangle (level 0)
    pub fn new_root(initial_energy: u64, initial_state: &[u8]) -> Self {
        let state_root = hash_data(initial_state);
        let mut triangle = Triangle {
            id: [0; 32], // placeholder
            level: 0,
            parent_id: None,
            children_ids: Vec::new(),
            state_root,
            energy: initial_energy,
            cohesion_score: Self::calculate_cohesion(0, initial_energy),
        };
        triangle.id = triangle.calculate_id();
        triangle
    }

    /// Calculate the content-addressed ID
    pub fn calculate_id(&self) -> TriangleId {
        let mut data = Vec::new();
        data.extend_from_slice(&self.level.to_le_bytes());
        if let Some(parent) = self.parent_id {
            data.extend_from_slice(&parent);
        }
        for child in &self.children_ids {
            data.extend_from_slice(child);
        }
        data.extend_from_slice(&self.state_root);
        data.extend_from_slice(&self.energy.to_le_bytes());
        data.extend_from_slice(&self.cohesion_score.to_le_bytes());
        hash_data(&data)
    }

    /// Check if triangle is whole (not split)
    pub fn is_whole(&self) -> bool {
        self.children_ids.is_empty()
    }

    /// Check if triangle is split
    pub fn is_split(&self) -> bool {
        self.children_ids.len() == 3
    }

    /// Calculate cohesion score (derived metric)
    /// For simplicity, cohesion = energy / (level + 1)
    pub fn calculate_cohesion(level: u64, energy: u64) -> u64 {
        if level == 0 {
            energy
        } else {
            energy / (level + 1)
        }
    }

    /// Validate triangle invariants
    pub fn is_valid(&self) -> bool {
        // ID must match calculated
        if self.id != self.calculate_id() {
            return false;
        }
        // Level >= 0
        if self.level == 0 && self.parent_id.is_some() {
            return false;
        }
        if self.level > 0 && self.parent_id.is_none() {
            return false;
        }
        // Children: 0 or exactly 3
        if self.children_ids.len() != 0 && self.children_ids.len() != 3 {
            return false;
        }
        // Cohesion score must be correct
        if self.cohesion_score != Self::calculate_cohesion(self.level, self.energy) {
            return false;
        }
        true
    }
}
