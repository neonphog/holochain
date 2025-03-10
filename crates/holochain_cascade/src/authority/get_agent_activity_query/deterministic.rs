//! Query for `deterministic_get_agent_activity`, designed for use in
//! validation callbacks.
//!
//! This is a deterministic version of `get_agent_activity`, designed such that
//! there can only be one possible valid result which satisfies the query
//! criteria, so if you get back a result, you can verify that it is correct
//! and safely use it in your own validation. If you don't get a value back,
//! you cannot proceed with validation.
//!
//! - The agent authority will fully validate Actions, so it's OK to pass the
//!   full actions to Wasm
//! - Must return a contiguous range of Actions so that the requestor can
//!   ensure that the data is valid (TODO we're skipping the actual validation
//!   on the requestor side for now).

use holo_hash::*;
use holochain_p2p::event::GetActivityOptions;
use holochain_sqlite::rusqlite::*;
use holochain_state::{
    prelude::*,
    query::{row_blob_and_hash_to_action, QueryData},
};
use std::{fmt::Debug, sync::Arc};

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct DeterministicGetAgentActivityQuery {
    agent: AgentPubKey,
    filter: DeterministicGetAgentActivityFilter,
    options: GetActivityOptions,
}

impl DeterministicGetAgentActivityQuery {
    pub fn new(
        agent: AgentPubKey,
        filter: DeterministicGetAgentActivityFilter,
        options: GetActivityOptions,
    ) -> Self {
        Self {
            agent,
            filter,
            options,
        }
    }
}

#[derive(Debug)]
pub struct DeterministicGetAgentActivityQueryState {
    chain: Vec<Judged<SignedAction>>,
    prev_action: Option<ActionHash>,
}

impl Query for DeterministicGetAgentActivityQuery {
    type Item = Judged<SignedActionHashed>;
    type State = DeterministicGetAgentActivityQueryState;
    type Output = DeterministicGetAgentActivityResponse;

    fn query(&self) -> String {
        "
            SELECT H.blob, H.hash, D.validation_status FROM Action AS H
            JOIN DhtOp as D
            ON D.action_hash = H.hash
            WHERE H.author = :author
            AND D.type = :op_type
            AND D.validation_status IS NOT NULL
            AND D.when_integrated IS NOT NULL
            AND (:hash_low IS NULL OR H.seq >= (SELECT seq FROM Action WHERE hash = :hash_low))
            AND H.seq <= (SELECT seq FROM Action WHERE hash = :hash_high)
            ORDER BY H.seq DESC
        "
        .to_string()
    }

    fn params(&self) -> Vec<holochain_state::query::Params> {
        (named_params! {
            ":author": self.agent,
            ":hash_low": self.filter.range.0,
            ":hash_high": self.filter.range.1,
            ":op_type": ChainOpType::RegisterAgentActivity,
        })
        .to_vec()
    }

    fn init_fold(&self) -> StateQueryResult<Self::State> {
        Ok(DeterministicGetAgentActivityQueryState {
            chain: Vec::new(),
            prev_action: Some(self.filter.range.1.clone()),
        })
    }

    fn as_filter(&self) -> Box<dyn Fn(&QueryData<Self>) -> bool> {
        todo!()
    }

    fn fold(&self, mut state: Self::State, item: Self::Item) -> StateQueryResult<Self::State> {
        let (shh, status) = item.into();
        let SignedActionHashed {
            hashed:
                ActionHashed {
                    content: action,
                    hash,
                },
            signature,
        } = shh;
        let sh = SignedAction::new(action, signature);
        // By tracking the prev_action of the last action we added to the chain,
        // we can filter out branches. If we performed branch detection in this
        // query, it would not be deterministic.
        //
        // TODO: ensure that this still works with the scratch, and that we
        // never have to run this query including the Cache. That is, if we join
        // results from multiple Stores, the ordering of action_seq will be
        // discontinuous, and we will have to collect into a sorted list before
        // doing this fold.
        if Some(hash) == state.prev_action {
            state.prev_action = sh.action().prev_action().cloned();
            state.chain.push((sh, status).into());
        }
        Ok(state)
    }

    fn render<S>(&self, state: Self::State, _stores: S) -> StateQueryResult<Self::Output>
    where
        S: Store,
    {
        Ok(DeterministicGetAgentActivityResponse::new(state.chain))
    }

    fn as_map(&self) -> Arc<dyn Fn(&Row) -> StateQueryResult<Self::Item>> {
        let f = row_blob_and_hash_to_action("blob", "hash");
        Arc::new(move |row| {
            let validation_status: ValidationStatus = row.get("validation_status")?;
            Ok(Judged::new(f(row)?, validation_status))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::fill_db;
    use ::fixt::prelude::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn agent_activity_query() {
        holochain_trace::test_run();
        let test_db = test_dht_db();
        let db = test_db.to_db();
        let entry_type_1 = fixt!(EntryType);
        let agents = [fixt!(AgentPubKey), fixt!(AgentPubKey), fixt!(AgentPubKey)];
        let mut chains = vec![];

        for a in 0..3 {
            let mut chain: Vec<ActionHash> = Vec::new();
            for seq in 0..10 {
                let action: Action = if let Some(top) = chain.last() {
                    let mut action = fixt!(Create);
                    action.entry_type = entry_type_1.clone();
                    action.author = agents[a].clone();
                    action.prev_action = top.clone();
                    action.action_seq = seq;
                    let entry = Entry::App(fixt!(AppEntryBytes));
                    action.entry_hash = EntryHash::with_data_sync(&entry);
                    action.into()
                } else {
                    let mut action = fixt!(Dna);
                    action.author = agents[a].clone();
                    action.into()
                };
                chain.push(ActionHash::with_data_sync(&action));
                let op = ChainOp::RegisterAgentActivity(fixt!(Signature), action);
                let op = ChainOpHashed::from_content_sync(op);
                fill_db(&db, op).await;
            }
            chains.push(chain);
        }

        let filter_full = DeterministicGetAgentActivityFilter {
            range: (None, chains[2].last().unwrap().clone()),
            entry_type: Some(entry_type_1.clone()),
            action_type: None,
            include_entries: false,
        };

        let filter_partial = DeterministicGetAgentActivityFilter {
            range: (Some(chains[2][4].clone()), chains[2][8].clone()),
            entry_type: Some(entry_type_1),
            action_type: None,
            include_entries: false,
        };
        let options = GetActivityOptions::default();

        let results_full = crate::authority::handle_get_agent_activity_deterministic(
            db.clone().into(),
            agents[2].clone(),
            filter_full,
            options.clone(),
        )
        .await
        .unwrap();

        let results_partial = crate::authority::handle_get_agent_activity_deterministic(
            db.clone().into(),
            agents[2].clone(),
            filter_partial,
            options,
        )
        .await
        .unwrap();

        assert_eq!(results_full.chain.len(), 10);
        assert_eq!(results_partial.chain.len(), 5);
    }
}
