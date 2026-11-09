use crate::EmbeddedImage;
use proc_macro2::{Ident, Literal, TokenStream};
use quote::quote;
use std::collections::BTreeMap;
use xrack_mlir::{Module, OperationRef};
use xrack_runtime::{ImageFormat, NineSlice};

pub(crate) fn edges(op: OperationRef<'_>) -> Result<NineSlice, String> {
    let edge = |index| {
        op.operand(index)
            .and_then(|value| value.defining_operation())
            .and_then(|literal| literal.attribute("value"))
            .and_then(|value| value.parse::<u32>().ok())
            .ok_or_else(|| "expected a nonnegative nine-slice edge".to_owned())
    };
    Ok(NineSlice {
        top: edge(1)?,
        right: edge(2)?,
        bottom: edge(3)?,
        left: edge(4)?,
    })
}

pub fn check_nine_slices(
    module: &Module,
    images: &BTreeMap<String, EmbeddedImage>,
) -> Result<(), String> {
    fn check(op: OperationRef<'_>, images: &BTreeMap<String, EmbeddedImage>) -> Result<(), String> {
        if op.name() == "xr.nine_slice" {
            let url = op
                .operand(0)
                .and_then(|value| value.defining_operation())
                .ok_or("missing nine-slice source")?
                .attr("value")?;
            let image = images
                .get(url)
                .ok_or_else(|| format!("unresolved image {url:?}"))?;
            if matches!(image, EmbeddedImage::Animated { .. }) {
                return Err("nine-slice requires a static PNG or SVG source".into());
            }
            if !edges(op)?.fits(image.size()) {
                let span = op.span();
                let source = op.source_file().unwrap_or("<unknown>");
                let [width, height] = image.size();
                return Err(format!(
                    "{source}:{}:{}: nine-slice edges must leave a nonempty center in image {url:?} ({width} × {height})",
                    span.line, span.column
                ));
            }
        }
        for child in op.children() {
            check(child, images)?;
        }
        Ok(())
    }
    for op in module.operations() {
        check(op, images)?;
    }
    Ok(())
}

pub(crate) fn declaration(name: &Ident, image: &EmbeddedImage) -> TokenStream {
    let [width, height] = image.size();
    let value = match image {
        EmbeddedImage::Static { format, bytes, .. } => {
            let variant = match format {
                ImageFormat::Png => quote!(Png),
                ImageFormat::Svg => quote!(Svg),
            };
            let bytes = Literal::byte_string(bytes);
            quote!(::xrack_runtime::Image::Embedded {
                format: ::xrack_runtime::ImageFormat::#variant,
                bytes: #bytes,
                size: [#width, #height],
            })
        }
        EmbeddedImage::Animated { frames, plays, .. } => {
            let frames = frames.iter().map(|(end_ms, png)| {
                let png = Literal::byte_string(png);
                quote!(::xrack_runtime::ImageAnimationFrame { end_ms: #end_ms, png: #png })
            });
            quote!({
                static ANIMATION: ::xrack_runtime::ImageAnimation =
                    ::xrack_runtime::ImageAnimation::new([#width, #height], &[#(#frames),*], #plays);
                ::xrack_runtime::Image::Animated(&ANIMATION)
            })
        }
    };
    quote!(static #name: ::xrack_runtime::Image = #value;)
}
