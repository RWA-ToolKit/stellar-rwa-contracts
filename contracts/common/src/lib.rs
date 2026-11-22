#![no_std]
//! # rwa-common
//!
//! Shared helpers extracted from the four RWA ToolKit contracts to avoid
//! duplication and guarantee consistent behaviour:
//!
//! * `validation` — string-length guard (`check_str`, issue #765)
//! * `admin`      — two-step admin-handover state machine (issue #764)
//! * `types`      — `AssetMetadata`, the asset token's metadata as other
//!   contracts read it over a cross-contract call

pub mod admin;
pub mod types;
pub mod validation;

pub use types::AssetMetadata;
