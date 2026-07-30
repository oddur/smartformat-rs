//! Derive macro for the `smartformat` crate.

use proc_macro::TokenStream;

/// Derives `ToSmartValue`, converting a struct into a `Value::Map` keyed by
/// field name so templates can address it as `{FieldName}`.
///
/// Not implemented yet — lands with milestone M1. Until then this emits a
/// compile error at the derive site rather than silently doing nothing.
#[proc_macro_derive(ToSmartValue)]
pub fn derive_to_smart_value(_input: TokenStream) -> TokenStream {
    "compile_error!(\"#[derive(ToSmartValue)] is not implemented yet (milestone M1)\");"
        .parse()
        .unwrap()
}
