//! Formatter extensions beyond `DefaultFormatter`, each a port of the
//! same-named SmartFormat.NET extension. They implement
//! [`Formatter`](crate::formatter::Formatter) and are registered by
//! `SmartFormatter` in .NET's `CreateDefaultSmartFormat` order.

pub mod choose;
pub mod conditional;
#[cfg(feature = "plural")]
pub mod plural;
#[cfg(feature = "plural")]
pub mod plural_rules;
