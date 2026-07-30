//! A Rust port of [SmartFormat.NET](https://github.com/axuno/SmartFormat).
//!
//! String templating with named placeholders, pluralization, conditional
//! formatting, and list formatting — compatible with the SmartFormat template
//! syntax and .NET standard format specifiers, so templates written for the
//! .NET library render identically here.
//!
//! Work in progress; see `DESIGN.md` in the repository for scope and milestones.

pub mod error;
pub mod parsing;
pub mod settings;
pub mod value;

pub use error::Error;
pub use settings::{ErrorAction, SmartSettings};
pub use value::Value;

#[cfg(feature = "derive")]
pub use smartformat_derive::ToSmartValue;
