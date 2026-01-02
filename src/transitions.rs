use crate::triangle::{Triangle, TriangleId};
use crate::error::ChainError;

/// Represents a valid state transition
#[derive(Debug, Clone)]
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub enum Transition {
    Split(SplitTransition),
    Heal(HealTransition),
}

/// Split transition: one whole triangle -> three child triangles
#[derive(Debug, Clone)]
pub struct SplitTransition {
    pub parent_id: TriangleId,
    pub children: [Triangle; 3],
}

/// Heal transition: three sibling triangles -> one parent triangle
#[derive(Debug, Clone)]
pub struct HealTransition {
    pub children_ids: [TriangleId; 3],
    pub parent: Triangle,
}

impl Transition {
    /// Validate a split transition
    pub fn validate_split(parent: &Triangle, children: &[Triangle; 3]) -> Result<(), ChainError> {
        // Parent must be whole
        if !parent.is_whole() {
            return Err(ChainError::InvalidTransaction("Parent triangle is not whole".to_string()));
        }
        // Children must have correct level
        for child in children {
            if child.level != parent.level + 1 {
                return Err(ChainError::InvalidTransaction("Child level incorrect".to_string()));
            }
            if child.parent_id != Some(parent.id) {
                return Err(ChainError::InvalidTransaction("Child parent_id incorrect".to_string()));
            }
            if !child.is_whole() {
                return Err(ChainError::InvalidTransaction("Child must be whole".to_string()));
            }
        }
        // Energy conservation: sum of children energy == parent energy
        let total_child_energy: u64 = children.iter().map(|c| c.energy).sum();
        if total_child_energy != parent.energy {
            return Err(ChainError::InvalidTransaction("Energy not conserved in split".to_string()));
        }
        // Deterministic partitioning: each child gets energy / 3
        let expected_energy = parent.energy / 3;
        for child in children {
            if child.energy != expected_energy {
                return Err(ChainError::InvalidTransaction("Energy not deterministically partitioned".to_string()));
            }
        }
        // State root: for simplicity, children inherit parent's state_root
        for child in children {
            if child.state_root != parent.state_root {
                return Err(ChainError::InvalidTransaction("State root not inherited".to_string()));
            }
        }
        Ok(())
    }

    /// Validate a heal transition
    pub fn validate_heal(children: &[Triangle; 3], parent: &Triangle) -> Result<(), ChainError> {
        // All children must be siblings (same parent, same level)
        let parent_id = children[0].parent_id;
        let level = children[0].level;
        for child in children {
            if child.parent_id != parent_id || child.level != level {
                return Err(ChainError::InvalidTransaction("Children are not siblings".to_string()));
            }
            if !child.is_whole() {
                return Err(ChainError::InvalidTransaction("Child is not whole".to_string()));
            }
        }
        // Parent must have correct level
        if parent.level != level - 1 {
            return Err(ChainError::InvalidTransaction("Parent level incorrect".to_string()));
        }
        if parent.parent_id != children[0].parent_id {
            return Err(ChainError::InvalidTransaction("Parent parent_id incorrect".to_string()));
        }
        // Parent must be whole
        if !parent.is_whole() {
            return Err(ChainError::InvalidTransaction("Parent is not whole".to_string()));
        }
        // Energy conservation
        let total_child_energy: u64 = children.iter().map(|c| c.energy).sum();
        if parent.energy != total_child_energy {
            return Err(ChainError::InvalidTransaction("Energy not conserved in heal".to_string()));
        }
        // State root: deterministic recombination, e.g., hash of children state_roots
        let mut combined_state = Vec::new();
        for child in children {
            combined_state.extend_from_slice(&child.state_root);
        }
        let expected_state_root = crate::triangle::hash_data(&combined_state);
        if parent.state_root != expected_state_root {
            return Err(ChainError::InvalidTransaction("State root not deterministically recombined".to_string()));
        }
        Ok(())
    }

    /// Apply a split transition to the fractal state
    pub fn apply_split(parent: &Triangle, children: &[Triangle; 3]) -> Result<Vec<Triangle>, ChainError> {
        Self::validate_split(parent, children)?;
        // Return the new triangles: remove parent, add children
        let mut new_triangles = vec![children[0].clone(), children[1].clone(), children[2].clone()];
        // Note: parent is removed implicitly
        Ok(new_triangles)
    }

    /// Apply a heal transition
    pub fn apply_heal(children: &[Triangle; 3], parent: &Triangle) -> Result<Vec<Triangle>, ChainError> {
        Self::validate_heal(children, parent)?;
        // Return the new triangle: remove children, add parent
        Ok(vec![parent.clone()])
    }
}
