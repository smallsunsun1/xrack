use proc_macro2::{Ident, TokenStream};
use quote::quote;
use std::collections::BTreeMap;
use xrack_mlir::{Module, OperationRef};
use xrack_runtime::StyledText;

pub(crate) fn collect(module: &Module) -> Result<BTreeMap<&str, StyledText>, String> {
    let mut texts = BTreeMap::new();
    for op in module.operations() {
        collect_in(op, &mut texts)?;
    }
    Ok(texts)
}

fn collect_in<'m>(
    op: OperationRef<'m>,
    texts: &mut BTreeMap<&'m str, StyledText>,
) -> Result<(), String> {
    if op.name() == "xr.markdown"
        && let std::collections::btree_map::Entry::Vacant(entry) = texts.entry(op.attr("value")?)
    {
        let text = StyledText::from_markdown(entry.key()).map_err(|error| {
            let file = op.source_file().unwrap_or("<unknown>");
            let span = op.span();
            format!("{file}:{}:{}: {error}", span.line, span.column)
        })?;
        entry.insert(text);
    }
    for child in op.children() {
        collect_in(child, texts)?;
    }
    Ok(())
}

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
    quote! {
        static #name: ::xrack_runtime::StyledText =
            ::xrack_runtime::StyledText::from_compiled(#plain, &[#(#runs),*]);
    }
}
