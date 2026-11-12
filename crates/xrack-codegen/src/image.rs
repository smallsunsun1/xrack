use crate::EmbeddedImage;
use proc_macro2::{Ident, Literal, TokenStream};
use quote::quote;
use xrack_runtime::ImageFormat;

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
