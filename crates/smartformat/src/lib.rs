// The README is the crate documentation, so its examples run as doctests.
// They use the derive, plural, and time features; without those the README is
// swapped for a short pointer instead of failing the doctest build.
#![cfg_attr(
    all(feature = "derive", feature = "plural", feature = "time"),
    doc = include_str!("../../../README.md")
)]
#![cfg_attr(
    not(all(feature = "derive", feature = "plural", feature = "time")),
    doc = "A Rust port of [SmartFormat.NET](https://github.com/axuno/SmartFormat): \
string templating with named placeholders, pluralization, conditional \
formatting, and list formatting, byte-compatible with the .NET library. \
Built without default features; see the repository README for the full \
documentation."
)]
//!
//! # Crate map
//!
//! - [`SmartFormatter`]: parse and render templates; the entry point.
//! - [`value`]: the [`Value`] tree templates render against, and
//!   [`ToSmartValue`](value::ToSmartValue) for converting your types.
//! - [`extensions`]: the built-in formatters (`list`, `plural`, `choose`, …).
//! - [`fmt::culture`]: locale data, generated from .NET itself.
//! - [`settings`]: [`SmartSettings`], error actions, case sensitivity.
//! - [`parsing`]: the template parser and [`Format`](parsing::Format) AST,
//!   for parse-once/render-many workflows.
//!
//! Compatibility scope and every known divergence from .NET live in
//! `DESIGN.md` in the repository.

pub mod error;
pub mod extensions;
pub mod fmt;
pub mod formatter;
pub mod parsing;
pub mod settings;
pub mod sources;
pub mod value;

pub use error::Error;
#[cfg(feature = "plural")]
pub use extensions::PluralLocalizationFormatter;
pub use extensions::{
    ChooseFormatter, ConditionalFormatter, ListFormatter, NullFormatter, SubStringFormatter,
    SubStringOutOfRangeBehavior, TemplateFormatter,
};
#[cfg(feature = "regex-formatters")]
pub use extensions::{IsMatchFormatter, RegexOptions};
pub use formatter::SmartFormatter;
pub use settings::{CaseSensitivity, ErrorAction, SmartSettings};
// `ToSmartValue` is not re-exported at the crate root: the derive macro of the
// same name lives there.
pub use value::Value;

#[cfg(feature = "derive")]
pub use smartformat_derive::ToSmartValue;
