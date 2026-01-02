/// Economics module: rules for energy and cohesion in the triangle mechanics
use crate::triangle::Triangle;

/// Calculate the cost of splitting a triangle
/// Splitting increases system tension, becomes progressively expensive
pub fn split_cost(triangle: &Triangle) -> u64 {
    // Cost increases with level: base cost + level * factor
    100 + triangle.level * 50
}

/// Calculate the benefit of healing
/// Healing reduces system tension
pub fn heal_benefit(triangle: &Triangle) -> u64 {
    // Benefit decreases with level or something
    50
}

/// Check if a split is economically viable
pub fn can_split(triangle: &Triangle) -> bool {
    triangle.energy > split_cost(triangle)
}

/// Check if a heal is favored
pub fn should_heal(_triangle: &Triangle) -> bool {
    // Always favor healing
    true
}
