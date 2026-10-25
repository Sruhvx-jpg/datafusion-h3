use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn, LitStr};

macro_rules! method_macro {
    ($macro_name:ident, $method_ident:ident) => {
        #[proc_macro_attribute]
        pub fn $macro_name(attr: TokenStream, item: TokenStream) -> TokenStream {
            let path = parse_macro_input!(attr as LitStr);
            let input_fn = parse_macro_input!(item as ItemFn);
            let fn_name = &input_fn.sig.ident;
            let vis = &input_fn.vis;

            let expanded = quote! {
                #[allow(non_camel_case_types)]
                #vis struct #fn_name;

                impl quinn_router::HttpServiceFactory for #fn_name {
                    fn register(self, config: &mut quinn_router::ServiceConfig) {
                        #input_fn

                        config.register_route(
                            #path,
                            http::Method::$method_ident,
                            std::sync::Arc::new(|req, stream| {
                                Box::pin(#fn_name(req, stream))
                            }),
                        );
                    }
                }
            };

            expanded.into()
        }
    };
}

method_macro!(get, GET);
method_macro!(post, POST);
method_macro!(put, PUT);
method_macro!(delete, DELETE);
method_macro!(patch, PATCH);
method_macro!(head, HEAD);
method_macro!(options, OPTIONS);
