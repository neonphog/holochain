//! Module containing the HolochainP2p actor definition.
#![allow(clippy::too_many_arguments)]

use crate::event::GetRequest;
use crate::*;
use holochain_types::activity::AgentActivityResponse;
use holochain_types::prelude::ValidationReceiptBundle;
use kitsune_p2p::dependencies::kitsune_p2p_fetch::FetchContext;
use kitsune_p2p::dependencies::kitsune_p2p_fetch::OpHashSized;
use kitsune_p2p::gossip::sharded_gossip::KitsuneDiagnostics;
use kitsune_p2p_types::agent_info::AgentInfoSigned;

/// Holochain-specific FetchContext extension trait.
pub trait FetchContextExt {
    /// Applies the "request_validation_receipt" flag *if* the param is true
    /// otherwise, leaves the flag unchanged.
    fn with_request_validation_receipt(&self, request_validation_receipt: bool) -> Self;

    /// Returns true if the "request_validation_receipt" flag is set.
    fn has_request_validation_receipt(&self) -> bool;

    /// Applies the "countersigning_session" flag *if* the param is true
    /// otherwise, leaves the flag unchanged.
    fn with_countersigning_session(&self, countersigning_session: bool) -> Self;

    /// Returns true if the "countersigning_session" flag is set.
    fn has_countersigning_session(&self) -> bool;
}

const FLAG_REQ_VAL_RCPT: u32 = 1 << 0;
const FLAG_CNTR_SSN: u32 = 1 << 1;

impl FetchContextExt for FetchContext {
    fn with_request_validation_receipt(&self, request_validation_receipt: bool) -> Self {
        if request_validation_receipt {
            FetchContext(self.0 | FLAG_REQ_VAL_RCPT)
        } else {
            *self
        }
    }

    fn has_request_validation_receipt(&self) -> bool {
        self.0 & FLAG_REQ_VAL_RCPT > 0
    }

    fn with_countersigning_session(&self, countersigning_session: bool) -> Self {
        if countersigning_session {
            FetchContext(self.0 | FLAG_CNTR_SSN)
        } else {
            *self
        }
    }

    fn has_countersigning_session(&self) -> bool {
        self.0 & FLAG_CNTR_SSN > 0
    }
}

#[derive(Clone, Debug)]
/// Get options help control how the get is processed at various levels.
/// Fields tagged with ```[Network]``` are network-level controls.
/// Fields tagged with ```[Remote]``` are controls that will be forwarded to the
/// remote agent processing this `Get` request.
pub struct GetOptions {
    /// ```[Network]```
    /// How many remote nodes should we make requests of / aggregate.
    /// Set to `None` for a default "best-effort".
    pub remote_agent_count: Option<u8>,

    /// ```[Network]```
    /// Timeout to await responses for aggregation.
    /// Set to `None` for a default "best-effort".
    /// Note - if all requests time-out you will receive an empty result,
    /// not a timeout error.
    pub timeout_ms: Option<u64>,

    /// ```[Network]```
    /// We are interested in speed. If `true` and we have any results
    /// when `race_timeout_ms` is expired, those results will be returned.
    /// After `race_timeout_ms` and before `timeout_ms` the first result
    /// received will be returned.
    pub as_race: bool,

    /// ```[Network]```
    /// See `as_race` for details.
    /// Set to `None` for a default "best-effort" race.
    pub race_timeout_ms: Option<u64>,

    /// ```[Remote]```
    /// Whether the remote-end should follow redirects or just return the
    /// requested entry.
    pub follow_redirects: bool,

    /// ```[Remote]```
    /// Return all live actions even if there is deletes.
    /// Useful for metadata calls.
    pub all_live_actions_with_metadata: bool,

    /// ```[Remote]```
    /// The type of data this get request requires.
    pub request_type: GetRequest,
}

impl Default for GetOptions {
    fn default() -> Self {
        Self {
            remote_agent_count: None,
            timeout_ms: None,
            as_race: true,
            race_timeout_ms: None,
            follow_redirects: true,
            all_live_actions_with_metadata: false,
            request_type: Default::default(),
        }
    }
}

impl GetOptions {
    /// Using defaults is dangerous in a must_get as it can undermine determinism.
    /// We want refactors to explicitly consider this.
    pub fn must_get_options() -> Self {
        Self {
            remote_agent_count: None,
            timeout_ms: None,
            as_race: true,
            race_timeout_ms: None,
            // Never redirect as the returned value must always match the hash.
            follow_redirects: false,
            all_live_actions_with_metadata: false,
            // Redundant with retrieve_entry internals.
            request_type: GetRequest::Pending,
        }
    }
}

impl From<holochain_zome_types::entry::GetOptions> for GetOptions {
    fn from(_: holochain_zome_types::entry::GetOptions) -> Self {
        Self::default()
    }
}

/// Get metadata from the DHT.
/// Fields tagged with ```[Network]``` are network-level controls.
/// Fields tagged with ```[Remote]``` are controls that will be forwarded to the
/// remote agent processing this `GetLinks` request.
#[derive(Clone, Debug)]
pub struct GetMetaOptions {
    /// ```[Network]```
    /// How many remote nodes should we make requests of / aggregate.
    /// Set to `None` for a default "best-effort".
    pub remote_agent_count: Option<u8>,

    /// ```[Network]```
    /// Timeout to await responses for aggregation.
    /// Set to `None` for a default "best-effort".
    /// Note - if all requests time-out you will receive an empty result,
    /// not a timeout error.
    pub timeout_ms: Option<u64>,

    /// ```[Network]```
    /// We are interested in speed. If `true` and we have any results
    /// when `race_timeout_ms` is expired, those results will be returned.
    /// After `race_timeout_ms` and before `timeout_ms` the first result
    /// received will be returned.
    pub as_race: bool,

    /// ```[Network]```
    /// See `as_race` for details.
    /// Set to `None` for a default "best-effort" race.
    pub race_timeout_ms: Option<u64>,

    /// ```[Remote]```
    /// Tells the remote-end which metadata to return
    pub metadata_request: MetadataRequest,
}

impl Default for GetMetaOptions {
    fn default() -> Self {
        Self {
            remote_agent_count: None,
            timeout_ms: None,
            as_race: true,
            race_timeout_ms: None,
            metadata_request: MetadataRequest::default(),
        }
    }
}

#[derive(Debug, Clone, Default)]
/// Get links from the DHT.
/// Fields tagged with ```[Network]``` are network-level controls.
/// Fields tagged with ```[Remote]``` are controls that will be forwarded to the
/// remote agent processing this `GetLinks` request.
pub struct GetLinksOptions {
    /// ```[Network]```
    /// Timeout to await responses for aggregation.
    /// Set to `None` for a default "best-effort".
    /// Note - if all requests time-out you will receive an empty result,
    /// not a timeout error.
    pub timeout_ms: Option<u64>,
    /// Whether to fetch links from the network or return only
    /// locally available links. Defaults to fetching links from network.
    pub get_options: holochain_zome_types::entry::GetOptions,
}

#[derive(Debug, Clone)]
/// Get agent activity from the DHT.
/// Fields tagged with ```[Network]``` are network-level controls.
/// Fields tagged with ```[Remote]``` are controls that will be forwarded to the
/// remote agent processing this `GetLinks` request.
pub struct GetActivityOptions {
    /// ```[Network]```
    /// Timeout to await responses for aggregation.
    /// Set to `None` for a default "best-effort".
    /// Note - if all requests time-out you will receive an empty result,
    /// not a timeout error.
    pub timeout_ms: Option<u64>,
    /// Number of times to retry getting records in parallel.
    /// For a small dht a large parallel get can overwhelm a single
    /// agent and it can be worth retrying the records that didn't
    /// get found.
    pub retry_gets: u8,
    /// ```[Remote]```
    /// Include the all valid activity actions in the response.
    /// If this is false the call becomes a lightweight response with
    /// just the chain status and highest observed action.
    /// This is useful when you want to ask an authority about the
    /// status of a chain but do not need all the actions.
    pub include_valid_activity: bool,
    /// Include any rejected actions in the response.
    pub include_rejected_activity: bool,
    /// Include the full signed actions and hashes in the response
    /// instead of just the hashes.
    pub include_full_actions: bool,
}

impl Default for GetActivityOptions {
    fn default() -> Self {
        Self {
            timeout_ms: None,
            retry_gets: 0,
            include_valid_activity: true,
            include_rejected_activity: false,
            include_full_actions: false,
        }
    }
}

type MaybeDnaHash = Option<DnaHash>;

ghost_actor::ghost_chan! {
    /// The HolochainP2pSender struct allows controlling the HolochainP2p
    /// actor instance.
    pub chan HolochainP2p<HolochainP2pError> {
        /// The p2p module must be informed at runtime which dna/agent pairs it should be tracking.
        fn join(dna_hash: DnaHash, agent_pub_key: AgentPubKey, maybe_agent_info: Option<AgentInfoSigned>, initial_arq: Option<crate::dht::Arq>) -> ();

        /// If a cell is disabled, we'll need to \"leave\" the network module as well.
        fn leave(dna_hash: DnaHash, agent_pub_key: AgentPubKey) -> ();

        /// Invoke a zome function on a remote node (if you have been granted the capability).
        fn call_remote(
            dna_hash: DnaHash,
            from_agent: AgentPubKey,
            signature: Signature,
            to_agent: AgentPubKey,
            zome_name: ZomeName,
            fn_name: FunctionName,
            cap_secret: Option<CapSecret>,
            payload: ExternIO,
            nonce: Nonce256Bits,
            expires_at: Timestamp,
        ) -> SerializedBytes;

        /// Invoke a zome function on a remote node (if you have been granted the capability).
        /// This is a fire-and-forget operation, a best effort will be made
        /// to forward the signal, but if the conductor network is overworked
        /// it may decide not to deliver some of the signals.
        fn send_remote_signal(
            dna_hash: DnaHash,
            from_agent: AgentPubKey,
            to_agent_list: Vec<(Signature, AgentPubKey)>,
            zome_name: ZomeName,
            fn_name: FunctionName,
            cap: Option<CapSecret>,
            payload: ExternIO,
            nonce: Nonce256Bits,
            expires_at: Timestamp,
        ) -> ();

        /// Publish data to the correct neighborhood.
        fn publish(
            dna_hash: DnaHash,
            request_validation_receipt: bool,
            countersigning_session: bool,
            basis_hash: holo_hash::OpBasis,
            source: AgentPubKey,
            op_hash_list: Vec<OpHashSized>,
            timeout_ms: Option<u64>,
            reflect_ops: Option<Vec<DhtOp>>,
        ) -> ();

        /// Publish a countersigning op.
        fn publish_countersign(
            dna_hash: DnaHash,
            flag: bool,
            basis_hash: holo_hash::OpBasis,
            op: DhtOp,
        ) -> ();

        /// Get an entry from the DHT.
        fn get(
            dna_hash: DnaHash,
            dht_hash: holo_hash::AnyDhtHash,
            options: GetOptions,
        ) -> Vec<WireOps>;

        /// Get metadata from the DHT.
        fn get_meta(
            dna_hash: DnaHash,
            dht_hash: holo_hash::AnyDhtHash,
            options: GetMetaOptions,
        ) -> Vec<MetadataSet>;

        /// Get links from the DHT.
        fn get_links(
            dna_hash: DnaHash,
            link_key: WireLinkKey,
            options: GetLinksOptions,
        ) -> Vec<WireLinkOps>;

        /// Get a count of links from the DHT.
        fn count_links(
            dna_hash: DnaHash,
            query: WireLinkQuery,
        ) -> CountLinksResponse;

        /// Get agent activity from the DHT.
        fn get_agent_activity(
            dna_hash: DnaHash,
            agent: AgentPubKey,
            query: ChainQueryFilter,
            options: GetActivityOptions,
        ) -> Vec<AgentActivityResponse<ActionHash>>;

        /// A remote node is requesting agent activity from us.
        fn must_get_agent_activity(
            dna_hash: DnaHash,
            author: AgentPubKey,
            filter: holochain_zome_types::chain::ChainFilter,
        ) -> Vec<MustGetAgentActivityResponse>;

        /// Send a validation receipt to a remote node.
        fn send_validation_receipts(dna_hash: DnaHash, to_agent: AgentPubKey, receipts: ValidationReceiptBundle) -> ();

        /// New data has been integrated and is ready for gossiping.
        fn new_integrated_data(dna_hash: DnaHash) -> ();

        /// Check if any local agent in this space is an authority for a hash.
        fn authority_for_hash(dna_hash: DnaHash, basis: OpBasis) -> bool;

        /// Messages between agents negotiation a countersigning session.
        fn countersigning_session_negotiation(
            dna_hash: DnaHash,
            agents: Vec<AgentPubKey>,
            message: event::CountersigningSessionNegotiationMessage,
        ) -> ();

        /// Dump network metrics.
        fn dump_network_metrics(
            dna_hash: MaybeDnaHash,
        ) -> String;

        /// Dump network stats.
        fn dump_network_stats() -> String;

        /// Get struct for diagnostic data
        fn get_diagnostics(dna_hash: DnaHash) -> KitsuneDiagnostics;
    }
}

/// Convenience type for referring to the HolochainP2p GhostSender
pub type HolochainP2pRef = ghost_actor::GhostSender<HolochainP2p>;

/// Extension trait for converting `GhostSender<HolochainP2p>` into HolochainP2pDna
pub trait HolochainP2pRefToDna {
    /// Partially apply dna_hash && agent_pub_key to this sender,
    /// binding it to a specific dna context.
    fn into_dna(self, dna_hash: DnaHash, chc: Option<ChcImpl>) -> crate::HolochainP2pDna;

    /// Clone and partially apply dna_hash && agent_pub_key to this sender,
    /// binding it to a specific dna context.
    fn to_dna(&self, dna_hash: DnaHash, chc: Option<ChcImpl>) -> crate::HolochainP2pDna;
}

impl HolochainP2pRefToDna for HolochainP2pRef {
    fn into_dna(self, dna_hash: DnaHash, chc: Option<ChcImpl>) -> crate::HolochainP2pDna {
        crate::HolochainP2pDna {
            sender: self,
            dna_hash: Arc::new(dna_hash),
            chc,
        }
    }

    fn to_dna(&self, dna_hash: DnaHash, chc: Option<ChcImpl>) -> crate::HolochainP2pDna {
        self.clone().into_dna(dna_hash, chc)
    }
}
