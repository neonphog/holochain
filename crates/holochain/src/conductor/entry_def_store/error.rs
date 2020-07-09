#![allow(missing_docs)]

use thiserror::Error;
use crate::core::ribosome::error::RibosomeError;

#[derive(Error, Debug)]
pub enum EntryDefStoreError {
    #[error(transparent)]
    DnaError(#[from] RibosomeError),
    #[error("Too many entry definitions in a single zome. Entry definitions are limited to 255 per zome")]
    TooManyEntryDefs,
}

pub type EntryDefStoreResult<T> = Result<T, EntryDefStoreError>;