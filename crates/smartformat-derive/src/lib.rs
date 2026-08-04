//! Derive macro for the `smartformat` crate.

use proc_macro::TokenStream;
use quote::quote;
use syn::ext::IdentExt;
use syn::{parse_macro_input, parse_quote, Data, DeriveInput, Fields, FieldsNamed};

/// Derives `ToSmartValue`, converting a struct into a `Value::Map` keyed by
/// field name so templates can address it as `{FieldName}`.
///
/// Field names are used verbatim, case preserved, mirroring how
/// `ReflectionSource` in SmartFormat.NET exposes property names.
#[proc_macro_derive(ToSmartValue)]
pub fn derive_to_smart_value(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn expand(mut input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let entries: Vec<_> = named_fields(&input)?
        .named
        .iter()
        .map(|field| {
            let ident = field
                .ident
                .as_ref()
                .expect("named_fields only yields fields with an identifier");
            let key = ident.unraw().to_string();
            quote! {
                map.insert(
                    ::std::string::String::from(#key),
                    ::smartformat::value::ToSmartValue::to_smart_value(&self.#ident),
                );
            }
        })
        .collect();

    for param in input.generics.type_params_mut() {
        param
            .bounds
            .push(parse_quote!(::smartformat::value::ToSmartValue));
    }
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics ::smartformat::value::ToSmartValue for #name #ty_generics #where_clause {
            fn to_smart_value(&self) -> ::smartformat::value::Value {
                let mut map = ::std::collections::BTreeMap::new();
                #(#entries)*
                ::smartformat::value::Value::Map(map)
            }
        }
    })
}

fn named_fields(input: &DeriveInput) -> syn::Result<&FieldsNamed> {
    let unsupported = |what: &str| {
        syn::Error::new_spanned(
            input,
            format!("#[derive(ToSmartValue)] supports only structs with named fields; {what} is not supported"),
        )
    };

    match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => Ok(fields),
            Fields::Unnamed(_) => Err(unsupported("a tuple struct")),
            Fields::Unit => Err(unsupported("a unit struct")),
        },
        Data::Enum(_) => Err(unsupported("an enum")),
        Data::Union(_) => Err(unsupported("a union")),
    }
}
