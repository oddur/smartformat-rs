//! The prose documentation under `docs/`, pulled in so that every Rust block
//! in it is compiled and run by `cargo test --doc`.
//!
//! Nothing here is part of the API: the module exists only under `cfg(doctest)`,
//! and each guide hangs off a private item whose sole job is to carry the
//! `#[doc]` attribute. A guide that stops compiling fails the test suite, which
//! is the point — an example in these files is checked the same way an example
//! in a rustdoc comment is.

#[doc = include_str!("../../../docs/how-to/run-dotnet-templates.md")]
struct HowToRunDotnetTemplates;

#[doc = include_str!("../../../docs/how-to/localize-text.md")]
struct HowToLocalizeText;

#[doc = include_str!("../../../docs/how-to/choose-error-behavior.md")]
struct HowToChooseErrorBehavior;

#[doc = include_str!("../../../docs/how-to/test-your-templates.md")]
struct HowToTestYourTemplates;

#[doc = include_str!("../../../docs/how-to/add-a-culture.md")]
struct HowToAddACulture;

#[doc = include_str!("../../../docs/how-to/extend-with-your-own.md")]
struct HowToExtendWithYourOwn;
