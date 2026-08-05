//! The repository documentation, compiled: every ```` ```rust ```` block in the
//! files below is a doctest, so an example that stops working fails the build.
//!
//! The module is `#[cfg(doctest)]`, so it costs an ordinary build nothing.

#[doc = include_str!("../../../docs/reference/template-syntax.md")]
mod reference_template_syntax {}

#[doc = include_str!("../../../docs/reference/formatters.md")]
mod reference_formatters {}

#[doc = include_str!("../../../docs/reference/format-specifiers.md")]
mod reference_format_specifiers {}

#[doc = include_str!("../../../docs/reference/settings-and-features.md")]
mod reference_settings_and_features {}

#[doc = include_str!("../../../docs/reference/cultures.md")]
mod reference_cultures {}
