pub mod engine;
pub mod error;

pub use engine::{QueryEngine, QueryExplanation, QueryOptions, QueryResultItem, ScoreBreakdown};
pub use error::{QueryError, Result};
