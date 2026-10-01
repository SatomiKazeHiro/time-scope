pub mod classifier;
pub mod config;
pub mod context;
pub mod segmenter;
pub mod types;

pub use classifier::{Classification, Rule, RuleError, RuleSet, DEFAULT_RULES_TOML};
pub use config::EngineConfig;
pub use context::{context_of, ContextBuilder};
pub use segmenter::{reduce, EngineOutput, EngineState};
pub use types::{ActivityContext, ActivitySegment, Category, OpenSegment, CLASSIFIER_RULE};

#[cfg(test)]
mod tests;
