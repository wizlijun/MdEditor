//! Memory Protocol v2 authority, immutable repository, reducer and projector.

mod canonical;
mod model;
mod projector;
mod reducer;
mod repository;
mod service;
mod writer;

pub use canonical::{
    canonical_bytes, canonical_yaml, payload_sha256, raw_sha256, CanonicalPayload,
};
pub use model::*;
pub use projector::{project, rebuild_projections, select_context, ProjectionBundle};
pub use reducer::{reduce, ReducerError};
pub use repository::{Loaded, RepositoryError, RepositorySnapshot, V2Repository};
pub use service::{dispatch as dispatch_rpc, propose_pending, PendingProposalInput};
pub use writer::{Published, RepositoryWriter, WriterError};

pub const PROTOCOL_MAJOR: u32 = 2;
