use kitsune_p2p_dht_arc::DEFAULT_MIN_PEERS;

use crate::spacetime::Topology;

use super::{Arq, PeerView, PeerViewQ};

/// A Strategy for generating PeerViews.
/// The enum allows us to add new strategies over time.
#[derive(Debug, Clone, derive_more::From)]
pub enum PeerStrat {
    /// The quantized peer strat
    Quantized(ArqStrat),
}

#[cfg(feature = "test_utils")]
impl Default for PeerStrat {
    fn default() -> Self {
        ArqStrat::default().into()
    }
}

impl PeerStrat {
    /// Generate a view using this strategy.
    /// Ensures that only peers which are visible from `arc` are included.
    // TODO: this can be a space dimension, not the full topology
    pub fn view(&self, topo: Topology, peers: &[Arq]) -> PeerView {
        match self {
            Self::Quantized(s) => PeerViewQ::new(topo, s.clone(), peers.to_vec()).into(),
        }
    }
}

/// "Arq Resizing Strategy". Defines all parameters necessary to run the arq
/// resizing algorithm.
#[derive(Debug, Clone)]
pub struct ArqStrat {
    /// The minimum overall coverage the DHT seeks to maintain.
    /// A coverage of N means that any particular location of the DHT is covered
    /// by N nodes. You can also think of this as a "redundancy factor".
    ///
    /// The whole purpose of the arc resizing is for all agents to adjust
    /// their arcs so that at least this amount of coverage (redundancy) is obtained
    /// at all times.
    pub min_coverage: f64,

    /// A multiplicative factor of the min coverage which defines a max.
    /// coverage. We want coverage to be between the min and max coverage.
    /// This is expressed in terms of a value > 0 and < 1. For instance,
    /// a min coverage of 50 with a buffer of 0.2 implies a max coverage of 60.
    pub buffer: f64,

    /// If the difference between the arq's power and the median power of all
    /// peer arqs (including this one) is greater than this diff,
    /// then don't requantize:
    /// just keep growing or shrinking past the min/max chunks value.
    ///
    /// This parameter determines how likely it is for there to be a difference in
    /// chunk sizes between two agents' arqs. It establishes the tradeoff between
    /// the size of payloads that must be sent and the extra coordination or
    /// computation that must be performed to accomodate agents whose power is
    /// lower than ours.
    ///
    /// This parameter is also what allows an arq to shrink to zero in a
    /// reasonable number of steps. Without this limit on power diff, we would
    /// keep requantizing until the power was 0 before shrinking to the empty arc.
    /// We may shrink to zero if our neighborhood is significantly over-covered,
    /// which can happen if a number of peers decide to keep their coverage
    /// higher than the ideal equilibrium value.
    ///
    /// Note that this parameter does not guarantee that any agent's arq
    /// will have a power +/- this diff from our power, but we may decide to
    /// choose not to gossip with agents whose power falls outside the range
    /// defined by this diff. TODO: do this.
    pub max_power_diff: u8,

    /// If at any time the number of peers seen by a node is less than the
    /// extrapolated coverage scaled by this factor, then we assume that we need
    /// to grow our arc so that we can see more peers.
    /// In other words, we are "slacking" if at any time:
    ///     num_peers < extrapolated_coverage * slack_factor
    ///
    /// If this is set too high, it may prevent arcs from legitimately shrinking.
    /// If set too low, it will hamper the ability for extremely small arcs to
    /// reach a proper size
    pub slacker_ratio: f64,

    /// If the standard deviation of the powers of each arq in this view is
    /// greater than this threshold, then we might do something different when
    /// it comes to our decision to requantize. For now, just print a warning.
    ///
    /// TODO: this can probably be expressed in terms of `max_power_diff`.
    pub power_std_dev_threshold: f64,

    /// Settings to override the global arc settings, for instance to mandate
    /// an always full arc, or an always zero arc
    pub local_storage: LocalStorageConfig,
}

#[cfg(feature = "test_utils")]
impl Default for ArqStrat {
    fn default() -> Self {
        Self::standard(LocalStorageConfig::default())
    }
}

impl ArqStrat {
    /// Standard arq strat
    pub fn standard(local_storage: LocalStorageConfig) -> Self {
        Self {
            min_coverage: DEFAULT_MIN_PEERS as f64,
            // this buffer implies min-max chunk count of 8-16
            buffer: 0.143,
            power_std_dev_threshold: 1.0,
            max_power_diff: 2,
            slacker_ratio: 0.75,
            local_storage,
        }
    }

    /// The midline between min and max coverage
    pub fn midline_coverage(&self) -> f64 {
        (self.min_coverage + self.max_coverage()) / 2.0
    }

    /// The max coverage as expressed by the min coverage and the buffer
    pub fn max_coverage(&self) -> f64 {
        (self.min_coverage * (self.buffer + 1.0)).ceil()
    }

    /// The width of the buffer range
    pub fn buffer_width(&self) -> f64 {
        self.min_coverage * self.buffer
    }

    /// The lower bound of number of chunks to maintain in an arq.
    /// When the chunk count falls below this number, halve the chunk size.
    pub fn min_chunks(&self) -> u32 {
        self.chunk_count_threshold().ceil() as u32
    }

    /// The upper bound of number of chunks to maintain in an arq.
    /// When the chunk count exceeds this number, double the chunk size.
    ///
    /// This is expressed in terms of min_chunks because we want this value
    /// to always be odd -- this is because when growing the arq, we need to
    /// downshift the power, and we can only downshift losslessly if the count
    /// is even, and the most common case of exceeding the max_chunks is
    /// is to exceed the max_chunks by 1, which would be an even number.
    pub fn max_chunks(&self) -> u32 {
        let max_chunks = self.min_chunks() * 2 - 1;
        assert!(max_chunks % 2 == 1);
        max_chunks
    }

    /// The floor of the log2 of the max_chunks.
    /// For the default of 15, floor(log2(15)) = 3
    pub fn max_chunks_log2(&self) -> u8 {
        (self.max_chunks() as f64).log2().floor() as u8
    }

    /// The chunk count which gives us the quantization resolution appropriate
    /// for maintaining the buffer when adding/removing single chunks.
    /// Used in `min_chunks` and `max_chunks`.
    ///
    /// See this doc for rationale:
    /// https://hackmd.io/@hololtd/r1IAIbr5Y/https%3A%2F%2Fhackmd.io%2FK_fkBj6XQO2rCUZRRL9n2g
    fn chunk_count_threshold(&self) -> f64 {
        (self.buffer + 1.0) / self.buffer
    }

    /// Get a summary report of this strat in string format
    pub fn summary(&self) -> String {
        format!(
            "
        min coverage: {}
        max coverage: {}
        min chunks:   {}
        max chunks:   {}
        ",
            self.min_coverage,
            self.max_coverage(),
            self.min_chunks(),
            self.max_chunks()
        )
    }
}

/// Configure settings for arc storage.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct LocalStorageConfig {
    /// Setting to clamp all arcs to a given size
    pub arc_clamping: Option<ArqClamping>,
}

#[allow(clippy::derivable_impls)]
impl Default for LocalStorageConfig {
    fn default() -> Self {
        Self { arc_clamping: None }
    }
}

/// Instructions to clamp all arqs to a certain size, regardless of network conditions.
/// This allows the user to either be the ultimate freeloader, or the ultimate benefactor.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum ArqClamping {
    /// Clamp all arqs to be empty, and never grow them.
    Empty,
    /// Clamp all arqs to be full, and never shrink them.
    Full,
}
