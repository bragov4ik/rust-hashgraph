use std::collections::{HashSet, VecDeque};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::trace;

use crate::{
    algorithm::event,
    common::{Directed, Reversable},
};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Jobs<TPayload, TGenesisPayload, TPeerId> {
    inner: Vec<event::SignedEvent<TPayload, TGenesisPayload, TPeerId>>,
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("The provided tip is unknown in this state. Hash: {:?}.", 0)]
    IncorrectTip(event::Hash),
    #[error("Unknown event. Hash: {:?}.", 0)]
    UnknownEvent(event::Hash),
}

impl<TPayload, TGenesisPayload, TPeerId> Jobs<TPayload, TGenesisPayload, TPeerId> {
    pub fn as_linear(&self) -> &Vec<event::SignedEvent<TPayload, TGenesisPayload, TPeerId>> {
        &self.inner
    }

    pub fn into_linear(self) -> Vec<event::SignedEvent<TPayload, TGenesisPayload, TPeerId>> {
        self.inner
    }

    /// Generate jobs for the peer to perform in order to achieve at least the same
    /// state as ours.
    pub(crate) fn generate<G, FKnows, FEvent>(
        known_state: G,
        peer_knows_event: FKnows,
        known_state_tips: impl Iterator<Item = event::Hash>,
        get_event: FEvent,
    ) -> Result<Self, Error>
    where
        G: Directed<NodeIdentifier = event::Hash, NodeIdentifiers = Vec<event::Hash>>,
        FKnows: Fn(&event::Hash) -> bool,
        FEvent: Fn(&event::Hash) -> Option<event::SignedEvent<TPayload, TGenesisPayload, TPeerId>>,
    {
        // We need topologically sorted subgraph of known state, that is unknown
        // to the peer. The sorting must be from the oldest to the newest events.
        //
        // To find it, we do a trick: we find the reverse topsort.
        // 1. By definition of topological sorting,
        //      "for every directed edge u -> v from vertex u to vertex v, u comes before v in the ordering"
        //      We find topsort for such graph.
        // 2. Then we reverse each edge in the graph, so for each edge
        //      u <- v we will have u before v in the same ordering.
        // 3. Then we reverse/flip the ordering itself, we will have that for each
        //      u <- v, v is before u in the ordering.
        // Thus, it is a topological sort by defenition. Let's find it!

        // If we treat each parent -> child relationship as reverse (p <- c),
        // we need to start from the nodes without any children (thus without any
        // incoming edge p <- c). The nodes are tips.
        //
        // We can already filter out tips known to the peer, since all of its ancestors
        // are known to the peer.

        // work with reversed events
        let reversed_state = known_state.reversed();

        // nodes without incoming edges
        let sources: Vec<event::Hash> = known_state_tips
            .filter_map(|h| match reversed_state.in_neighbors(&h) {
                Some(in_neighbors) => {
                    if in_neighbors.is_empty() {
                        Some(Ok(h))
                    } else {
                        None
                    }
                }
                None => Some(Err(Error::IncorrectTip(h))),
            })
            .collect::<Result<_, _>>()?;
        trace!("Have {} sources", sources.len());
        let unknown_sources = sources.into_iter().filter(|h| !peer_knows_event(h));

        // Now do topsort with stop at known events

        let mut to_visit = VecDeque::from_iter(unknown_sources);
        let mut to_visit_set = HashSet::new();
        trace!(
            "Starting to traverse from {} sources (filtered known sources)",
            to_visit.len()
        );
        // to check removed edges
        let mut visited = HashSet::with_capacity(to_visit.len());
        let mut sorted = Vec::with_capacity(to_visit.len());
        while let Some(next) = to_visit.pop_front() {
            if visited.contains(&next) {
                continue;
            }
            trace!(
                "Visiting {:?}; checking its out neighbors",
                &next.as_compact()
            );
            visited.insert(next.clone());
            for affected_neighbor in reversed_state
                .out_neighbors(&next)
                .ok_or_else(|| Error::UnknownEvent(next.clone()))?
            {
                if to_visit_set.contains(&affected_neighbor) {
                    trace!(
                        "Neighbor {:?} is already scheduled, skipping it",
                        &affected_neighbor.as_compact()
                    );
                    continue;
                }
                trace!("Checking neighbor {:?}", &affected_neighbor.as_compact());
                if peer_knows_event(&affected_neighbor) {
                    trace!("Neighbor is known to the peer, skipping");
                    continue;
                }
                if reversed_state
                    .in_neighbors(&affected_neighbor)
                    .ok_or_else(|| Error::UnknownEvent(next.clone()))?
                    .into_iter()
                    .all(|in_neighbor| visited.contains(&in_neighbor))
                {
                    trace!("All in neighbors were visited before");
                    if !visited.contains(&affected_neighbor) {
                        to_visit_set.insert(affected_neighbor.clone());
                        to_visit.push_back(affected_neighbor)
                    }
                }
            }
            sorted.push(next);
        }
        // note: no loop detection; we assume the graph already has no loops

        // Prepare the jobs
        trace!("Reversing the ordering to get the result");
        sorted.reverse();
        let jobs: Vec<event::SignedEvent<TPayload, TGenesisPayload, TPeerId>> = sorted
            .into_iter()
            .map(|hash| get_event(&hash).ok_or_else(|| Error::UnknownEvent(hash)))
            .collect::<Result<_, _>>()?;
        Ok(Jobs { inner: jobs })
    }
}
