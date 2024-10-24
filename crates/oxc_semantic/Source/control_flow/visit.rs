use petgraph::{visit::EdgeRef, Direction, Graph};
use rustc_hash::FxHashSet;

use crate::BasicBlockId;

/// # Panics
pub fn neighbors_filtered_by_edge_weight<State:Default + Clone, NodeWeight, EdgeWeight, F, G>(
	graph:&Graph<NodeWeight, EdgeWeight>,
	node:BasicBlockId,
	edge_filter:&F,
	visitor:&mut G,
) -> Vec<State>
where
	F: Fn(&EdgeWeight) -> Option<State>,
	G: FnMut(&BasicBlockId, State) -> (State, bool), {
	let mut q = vec![];
	let mut final_states = vec![];
	let mut visited = FxHashSet::default();

	// for initial node
	let (new_state, keep_walking_this_path) = visitor(&node, Default::default());
	// if we will continue walking push this node
	if keep_walking_this_path {
		q.push((node, new_state));
	} else {
		final_states.push(new_state);
	}

	while let Some((graph_ix, state)) = q.pop() {
		let mut edges = 0;

		for edge in graph.edges_directed(graph_ix, Direction::Outgoing) {
			if visited.contains(&edge.target()) {
				continue;
			}
			if let Some(result_of_edge_filtering) = edge_filter(edge.weight()) {
				final_states.push(result_of_edge_filtering);
			} else {
				let target = edge.target();
				let (new_state, keep_walking_this_path) = visitor(&target, state.clone());
				visited.insert(target);
				if keep_walking_this_path {
					q.push((target, new_state.clone()));
				} else {
					final_states.push(new_state.clone());
				}
				edges += 1;
			}
		}

		if edges == 0 {
			final_states.push(state);
		}
	}

	final_states
}