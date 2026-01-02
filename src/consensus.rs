use crate::fractal_state::FractalState;
use crate::transitions::Transition;

/// Criteria for canonical reality selection
#[derive(PartialEq)]
pub struct ConsensusCriteria {
    pub min_unresolved_depth: u64,
    pub max_healed_mass: u64,
    pub total_energy: u64,
}

impl ConsensusCriteria {
    pub fn from_state(state: &FractalState) -> Self {
        Self {
            min_unresolved_depth: state.max_unresolved_depth(),
            max_healed_mass: state.total_healed_mass(),
            total_energy: state.total_energy(),
        }
    }
}

impl PartialOrd for ConsensusCriteria {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        // Compare by min unresolved depth (lower is better)
        match self.min_unresolved_depth.cmp(&other.min_unresolved_depth) {
            std::cmp::Ordering::Equal => {
                // Then by max healed mass (higher is better)
                match other.max_healed_mass.cmp(&self.max_healed_mass) {
                    std::cmp::Ordering::Equal => {
                        // Then by total energy (must be equal for conservation)
                        Some(self.total_energy.cmp(&other.total_energy))
                    }
                    ord => Some(ord),
                }
            }
            ord => Some(ord),
        }
    }
}

/// Consensus engine for selecting canonical triangle set
pub struct Consensus;

impl Consensus {
    /// Evaluate competing states and select the canonical one
    pub fn select_canonical(states: Vec<FractalState>) -> FractalState {
        // Filter valid states
        let valid_states: Vec<_> = states.into_iter()
            .filter(|s| s.is_valid())
            .collect();

        if valid_states.is_empty() {
            panic!("No valid fractal states");
        }

        // Select the one with best criteria
        let mut best = &valid_states[0];
        let mut best_criteria = ConsensusCriteria::from_state(best);

        for state in &valid_states[1..] {
            let criteria = ConsensusCriteria::from_state(state);
            if criteria > best_criteria {
                best = state;
                best_criteria = criteria;
            }
        }

        best.clone()
    }

    /// Apply a set of transitions to a state
    pub fn apply_transitions(state: &mut FractalState, transitions: Vec<Transition>) -> Result<(), crate::error::ChainError> {
        for transition in transitions {
            match transition {
                Transition::Split(split) => {
                    if let Some(parent) = state.get_triangle(&split.parent_id).cloned() {
                        let new_triangles = crate::transitions::Transition::apply_split(&parent, &split.children)?;
                        state.remove_triangle(&split.parent_id)?;
                        for t in new_triangles {
                            state.add_triangle(t)?;
                        }
                    }
                }
                Transition::Heal(heal) => {
                    let children = [
                        state.get_triangle(&heal.children_ids[0]).cloned().unwrap(),
                        state.get_triangle(&heal.children_ids[1]).cloned().unwrap(),
                        state.get_triangle(&heal.children_ids[2]).cloned().unwrap(),
                    ];
                    let new_triangles = crate::transitions::Transition::apply_heal(&children, &heal.parent)?;
                    for id in &heal.children_ids {
                        state.remove_triangle(id)?;
                    }
                    for t in new_triangles {
                        state.add_triangle(t)?;
                    }
                }
            }
        }
        Ok(())
    }
}
