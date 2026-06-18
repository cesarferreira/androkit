//! Error handling for androkit.
//!
//! androkit follows the same pragmatic approach as the CLIs that consume it:
//! `anyhow::Result` everywhere, with descriptive context messages. Callers that
//! need to match on specific failures can downcast, but no bespoke error enum is
//! exposed until a consumer actually needs one.

pub use anyhow::{anyhow, bail, Context, Error, Result};
