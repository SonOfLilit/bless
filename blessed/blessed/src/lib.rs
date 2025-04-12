use serde_json::Value;

pub use blessed_macros::harness;
pub use blessed_macros::tests;
pub use serde::{Serialize, Deserialize};

// Potentially add pub use schemars::JsonSchema; later

pub struct HarnessFn {
    pub name: &'static str,
    pub func: fn(Value) -> Result<Value, String>,
}

inventory::collect!(HarnessFn); 