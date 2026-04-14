use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

/// Derive macro that marks a component as saveable.
/// Use `#[save]` on individual fields to include them in save data.
#[proc_macro_derive(Saveable, attributes(save))]
pub fn derive_saveable(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    let expanded = quote! {
        impl Saveable for #name {
            fn type_name(&self) -> &'static str {
                stringify!(#name)
            }
        }
    };

    TokenStream::from(expanded)
}
