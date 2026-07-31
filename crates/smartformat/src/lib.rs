//! A Rust port of [SmartFormat.NET](https://github.com/axuno/SmartFormat).
//!
//! String templating with named placeholders, pluralization, conditional
//! formatting, and list formatting — compatible with the SmartFormat template
//! syntax and .NET standard format specifiers, so templates written for the
//! .NET library render identically here.
//!
//! Work in progress; see `DESIGN.md` in the repository for scope and milestones.

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
