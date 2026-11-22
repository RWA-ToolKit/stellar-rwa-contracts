#![no_std]
//! # rwa-common
//!
//! Shared helpers extracted from the four RWA ToolKit contracts to avoid
//! duplication and guarantee consistent behaviour:
//!
//! * `validation` — string-length guard (`check_str`, issue #765)
//! * `admin`      — two-step admin-handover state machine (issue #764)

pub mod admin;
pub mod validation;
