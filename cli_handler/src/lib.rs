use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{FnArg, Ident, ItemFn, Pat, parse_macro_input};

#[proc_macro_attribute]
pub fn cli_handler(args: TokenStream, input: TokenStream) -> TokenStream {
    // Expect a single identifier as the ApiResponseKind variant: #[cli_handler(Variant)]
    let variant_ident = parse_macro_input!(args as Ident);

    let mut input_fn = parse_macro_input!(input as ItemFn);

    // Capture pieces
    let vis = input_fn.vis.clone();
    let sig = input_fn.sig.clone();
    let attrs = input_fn.attrs.clone();
    let fn_name = sig.ident.clone();
    let impl_name = format_ident!("{}_impl", fn_name);
    let inputs = sig.inputs.clone();
    let generics = sig.generics.clone();
    let where_clause = sig.generics.where_clause.clone();

    // Collect argument patterns for call
    let mut call_args = Vec::new();
    for arg in &input_fn.sig.inputs {
        match arg {
            FnArg::Receiver(r) => {
                if r.reference.is_some() {
                    call_args.push(quote! { &self });
                } else {
                    call_args.push(quote! { self });
                }
            }
            FnArg::Typed(pat_ty) => {
                let pat: &Pat = &pat_ty.pat;
                call_args.push(quote! { #pat });
            }
        }
    }

    // Rename the original function to an inner implementation
    input_fn.sig.ident = impl_name.clone();

    // Wrapper output type
    let api_kind = quote! { api_model::protocol::message::api_response_message::ApiResponseKind };

    let output = quote! {
        // Emit the inner implementation (with original attrs)
        #(#attrs)*
        #input_fn

        // Emit the public wrapper with the same signature but ApiResponseKind return
        #vis async fn #fn_name #generics ( #inputs ) -> #api_kind #where_clause {
            match #impl_name( #(#call_args),* ).await {
                Ok(__resp) => #api_kind::#variant_ident(__resp),
                Err(e) => #api_kind::Error(e.to_string()),
            }
        }
    };

    TokenStream::from(output)
}

#[proc_macro_attribute]
pub fn cli_impl(_args: TokenStream, input: TokenStream) -> TokenStream {
    let mut input_fn = parse_macro_input!(input as ItemFn);

    // Capture pieces
    let vis = input_fn.vis.clone();
    let sig = input_fn.sig.clone();
    let attrs = input_fn.attrs.clone();
    let fn_name = sig.ident.clone();
    let impl_name = format_ident!("{}_impl", fn_name);
    let inputs = sig.inputs.clone();
    let generics = sig.generics.clone();

    // Collect argument patterns for call
    let mut call_args = Vec::new();
    for arg in &input_fn.sig.inputs {
        match arg {
            FnArg::Receiver(r) => {
                if r.reference.is_some() {
                    call_args.push(quote! { &self });
                } else {
                    call_args.push(quote! { self });
                }
            }
            FnArg::Typed(pat_ty) => {
                let pat: &Pat = &pat_ty.pat;
                call_args.push(quote! { #pat });
            }
        }
    }

    // Rename the original function to an inner implementation
    input_fn.sig.ident = impl_name.clone();

    let output = quote! {
        // Emit the inner implementation (with original attrs)
        #(#attrs)*
        #input_fn

        // Emit the public wrapper with the same signature but ApiResponseKind return
        #vis fn #fn_name #generics ( #inputs ) {
            match #impl_name( #(#call_args),* ) {
                Ok(__resp) => {},
                Err(e) => {
                    println!("{:?}", e);
                }
            }
        }
    };

    TokenStream::from(output)
}
