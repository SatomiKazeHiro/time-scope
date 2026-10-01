pub mod config;
pub mod types;

pub use config::EngineConfig;
pub use types::{ActivityContext, ActivitySegment, Category, OpenSegment, CLASSIFIER_RULE};

#[cfg(test)]
mod tests;
