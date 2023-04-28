use base64::{engine::general_purpose, Engine};
use proc_macro::{TokenStream};
use syn::{parse_macro_input, LitStr};
use quote::quote;

#[proc_macro]
pub fn include_base64(input: TokenStream) -> TokenStream {
    let string = parse_macro_input!(input as LitStr).value();
    let res = match general_purpose::URL_SAFE_NO_PAD.decode(string) {
        Ok(s) => s,
        Err(e) => {
            let s = e.to_string();
            return quote!(compile_error!(#s)).into()
        }
    };
    match String::from_utf8(res) {
        Ok(s) => {
            quote!(#s).into()
        },
        Err(e) => {
            let s = e.to_string();
            quote!(#s).into()
        }
    }
}
