use proc_macro2::{Ident, TokenStream};
use quote::quote;
use xrack_runtime::StyledText;

pub(crate) fn declaration(name: &Ident, text: &StyledText) -> TokenStream {
    let plain = text.text();
    let runs = text.runs().iter().map(|run| {
        let start = run.start;
        let end = run.end;
        let style = run.style;
        let bold = style.bold;
        let italic = style.italic;
        let monospace = style.monospace;
        let strikethrough = style.strikethrough;
        let underline = style.underline;
        let color = match style.color {
            Some(color) => {
                let rgba = color.0;
                quote!(::core::option::Option::Some(::xrack_runtime::Color(#rgba)))
            }
            None => quote!(::core::option::Option::None),
        };
        quote! {
            ::xrack_runtime::StyledRun {
                start: #start,
                end: #end,
                style: ::xrack_runtime::TextStyle {
                    bold: #bold,
                    italic: #italic,
                    monospace: #monospace,
                    strikethrough: #strikethrough,
                    underline: #underline,
                    color: #color,
                },
            }
        }
    });
    let links = text.links().iter().map(|link| {
        let start = link.start;
        let end = link.end;
        let href = link.href.as_ref();
        quote! {
            ::xrack_runtime::StyledLink { start: #start, end: #end, href: ::std::borrow::Cow::Borrowed(#href) }
        }
    });
    quote! {
        static #name: ::xrack_runtime::StyledText = {
            static LINKS: &[::xrack_runtime::StyledLink] = &[#(#links),*];
            ::xrack_runtime::StyledText::from_compiled_parts(#plain, &[#(#runs),*], LINKS)
        };
    }
}
