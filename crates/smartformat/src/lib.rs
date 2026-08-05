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
//! - [`sources`]: how a selector finds its value, and
//!   [`variables`](sources::variables) for names a template can use without
//!   them being passed as arguments.
//! - [`fmt::culture`]: locale data, generated from .NET itself.
//! - [`settings`]: [`SmartSettings`], error actions, case sensitivity.
//! - [`parsing`]: the template parser and [`Format`](parsing::Format) AST,
//!   for parse-once/render-many workflows.
//!
//! Compatibility scope and every known divergence from .NET live in
//! `DESIGN.md` in the repository.

// The guides under `docs/` are compiled and run as doctests. They use the
// derive, plural and time features, so a build without them skips the module
// rather than failing on examples it cannot compile.
#[cfg(all(doctest, feature = "derive", feature = "plural", feature = "time"))]
mod docs;

pub(crate) mod dotnet_messages;
pub mod error;
pub mod extensions;
pub mod fmt;
pub mod formatter;
pub mod parsing;
pub(crate) mod registry;
pub mod settings;
pub mod sources;
pub mod value;

pub use error::Error;
#[cfg(feature = "plural")]
pub use extensions::PluralLocalizationFormatter;
pub use extensions::{
    ChooseFormatter, ConditionalFormatter, HashMapLocalizationProvider, ListFormatter,
    LocalizationFormatter, LocalizationProvider, NullFormatter, SubStringFormatter,
    SubStringOutOfRangeBehavior, TemplateFormatter,
};
#[cfg(feature = "regex-formatters")]
pub use extensions::{IsMatchFormatter, RegexOptions};
#[cfg(feature = "time")]
pub use extensions::{TimeFormatter, TimeSpanFormatOptions, TimeTextInfo};
pub use formatter::SmartFormatter;
pub use settings::{CaseSensitivity, ErrorAction, SmartSettings};
// The only sources a caller has to name: every other one is in the default
// registry and is never mentioned by name.
pub use sources::variables::{GlobalVariablesSource, PersistentVariablesSource, VariablesGroup};
// `ToSmartValue` is not re-exported at the crate root: the derive macro of the
// same name lives there.
pub use value::Value;

#[cfg(feature = "derive")]
pub use smartformat_derive::ToSmartValue;
