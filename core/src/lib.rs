//! Platform-independent Telex engine for the ezkey Vietnamese input method.

mod engine;
pub mod ffi;
mod syllable;

pub use engine::{Action, Engine, InputMethod};
pub use syllable::ToneStyle;
