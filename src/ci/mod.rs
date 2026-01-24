pub mod ci_context;
pub mod github;
pub mod gitlab;

pub use ci_context::{CiContext, CiEvent, CiRunResult};