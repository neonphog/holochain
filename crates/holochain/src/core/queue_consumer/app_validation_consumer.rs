//! The workflow and queue consumer for sys validation

use super::*;
use crate::core::workflow::app_validation_workflow::app_validation_workflow;
use crate::core::workflow::app_validation_workflow::AppValidationWorkspace;
use crate::core::workflow::app_validation_workflow::ValidationDependencies;
use holochain_p2p::*;
use holochain_types::db_cache::DhtDbQueryCache;
use parking_lot::lock_api::Mutex;
use tracing::*;

/// Spawn the QueueConsumer for AppValidation workflow
#[instrument(skip(workspace, conductor, trigger_integration, network, dht_query_cache))]
pub fn spawn_app_validation_consumer(
    dna_hash: Arc<DnaHash>,
    workspace: AppValidationWorkspace,
    conductor: ConductorHandle,
    trigger_integration: TriggerSender,
    network: HolochainP2pDna,
    dht_query_cache: DhtDbQueryCache,
) -> TriggerSender {
    let (tx, rx) = TriggerSender::new();
    let workspace = Arc::new(workspace);
    let validation_dependencies = Arc::new(Mutex::new(ValidationDependencies::new()));

    super::queue_consumer_dna_bound(
        "app_validation_consumer",
        dna_hash.clone(),
        conductor.task_manager(),
        (tx.clone(), rx),
        move || {
            app_validation_workflow(
                dna_hash.clone(),
                workspace.clone(),
                trigger_integration.clone(),
                conductor.clone(),
                network.clone(),
                dht_query_cache.clone(),
                validation_dependencies.clone(),
            )
        },
    );
    tx
}
