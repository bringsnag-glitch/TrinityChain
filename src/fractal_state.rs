use crate::triangle::{Triangle, TriangleId};
use crate::error::ChainError;
use std::collections::HashMap;

/// Represents the complete fractal state: a set of triangles
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct FractalState {
    pub triangles: HashMap<TriangleId, Triangle>,
}

impl FractalState {
    pub fn new() -> Self {
        Self {
            triangles: HashMap::new(),
        }
    }

    /// Add a triangle to the state
    pub fn add_triangle(&mut self, triangle: Triangle) -> Result<(), ChainError> {
        if !triangle.is_valid() {
            return Err(ChainError::InvalidTransaction("Invalid triangle".to_string()));
        }
        if self.triangles.contains_key(&triangle.id) {
            return Err(ChainError::InvalidTransaction("Triangle already exists".to_string()));
        }
        self.triangles.insert(triangle.id, triangle);
        Ok(())
    }

    /// Remove a triangle
    pub fn remove_triangle(&mut self, id: &TriangleId) -> Result<Triangle, ChainError> {
        self.triangles.remove(id).ok_or_else(|| ChainError::TriangleNotFound(hex::encode(id)))
    }

    /// Get a triangle by ID
    pub fn get_triangle(&self, id: &TriangleId) -> Option<&Triangle> {
        self.triangles.get(id)
    }

    /// Validate the entire fractal state
    pub fn is_valid(&self) -> bool {
        // All triangles valid
        for triangle in self.triangles.values() {
            if !triangle.is_valid() {
                return false;
            }
        }
        // No orphan triangles (all non-root have parents)
        for triangle in self.triangles.values() {
            if triangle.level > 0 {
                if let Some(parent_id) = triangle.parent_id {
                    if !self.triangles.contains_key(&parent_id) {
                        return false;
                    }
                } else {
                    return false;
                }
            }
        }
        // Children consistency
        for triangle in self.triangles.values() {
            if triangle.is_split() {
                for child_id in &triangle.children_ids {
                    if let Some(child) = self.triangles.get(child_id) {
                        if child.parent_id != Some(triangle.id) {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// Calculate total energy in the system
    pub fn total_energy(&self) -> u64 {
        self.triangles.values().map(|t| t.energy).sum()
    }

    /// Calculate maximum unresolved depth (max level)
    pub fn max_unresolved_depth(&self) -> u64 {
        self.triangles.values().map(|t| t.level).max().unwrap_or(0)
    }

    /// Calculate total healed mass (sum of energy in triangles with level > 0 that are whole? Wait)
    /// Healed mass: perhaps sum of energy in triangles that have been healed, but since healed are recombined, maybe count the number of healed operations or something.
    /// For simplicity, total energy in non-leaf triangles or something.
    /// The spec: "maximum total healed mass"
    /// Perhaps mass healed is the energy in triangles that are at lower levels or something.
    /// For now, let's say total energy in whole triangles at level > 0
    pub fn total_healed_mass(&self) -> u64 {
        self.triangles.values()
            .filter(|t| t.level > 0 && t.is_whole())
            .map(|t| t.energy)
            .sum()
    }
}
