pub mod bench;
pub mod error;
pub mod generator;

pub use bench::{run_scale_benchmark, ScaleBenchmarkResult};
pub use error::{EvalError, Result};
pub use generator::{generate_synthetic_dataset, SimpleRng};
