//! Takumi (匠) — OpenAPI to typed IR resolution pipeline.
//!
//! Lowers OpenAPI specs (via `sekkei` types) into resolved, typed
//! intermediate representations suitable for code generation.

pub mod crud;
pub mod field_type;
pub mod resolve;

pub use crud::*;
pub use field_type::*;
pub use resolve::*;
