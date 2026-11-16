//! Core domain model, persistence and audit log for AGASTHY.
//!
//! This crate is platform-independent. It knows nothing about how data is
//! discovered; discovery crates hand it [`model::IngestBatch`] values and the
//! [`store::Store`] persists them.

pub mod availability;
pub mod error;
pub mod model;
pub mod store;
pub mod time;

pub use availability::Availability;
pub use error::{CoreError, CoreResult};
pub use model::*;
pub use store::Store;
