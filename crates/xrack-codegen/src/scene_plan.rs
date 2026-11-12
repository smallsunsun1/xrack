use crate::EmbeddedPath;
use proc_macro2::{Ident, Literal, TokenStream};
use quote::quote;
use xrack_runtime::{GridCellSlot, VectorSegment};

pub(crate) fn path_declaration(name: &Ident, commands: &str, path: &EmbeddedPath) -> TokenStream {
    let bounds = path.bounds.into_iter().map(Literal::f32_suffixed);
    let point = |[x, y]: [f32; 2]| {
        let x = Literal::f32_suffixed(x);
        let y = Literal::f32_suffixed(y);
        quote!([#x, #y])
    };
    let segments = path.segments.iter().map(|segment| match *segment {
        VectorSegment::MoveTo(p) => {
            let p = point(p);
            quote!(::xrack_runtime::VectorSegment::MoveTo(#p))
        }
        VectorSegment::LineTo(p) => {
            let p = point(p);
            quote!(::xrack_runtime::VectorSegment::LineTo(#p))
        }
        VectorSegment::QuadTo(c, p) => {
            let (c, p) = (point(c), point(p));
            quote!(::xrack_runtime::VectorSegment::QuadTo(#c, #p))
        }
        VectorSegment::CubicTo(a, b, p) => {
            let (a, b, p) = (point(a), point(b), point(p));
            quote!(::xrack_runtime::VectorSegment::CubicTo(#a, #b, #p))
        }
        VectorSegment::Close => quote!(::xrack_runtime::VectorSegment::Close),
    });
    quote! {
        static #name: ::xrack_runtime::StaticVectorPath = ::xrack_runtime::StaticVectorPath {
            commands: #commands,
            bounds: [#(#bounds),*],
            segments: &[#(#segments),*],
        };
    }
}

pub(crate) fn grid_declaration(node: &Ident, grid: Vec<GridCellSlot>) -> TokenStream {
    let slots = grid.into_iter().map(|slot| {
        let index = slot.index;
        let child_index = match slot.child_index {
            Some(index) => quote!(::core::option::Option::Some(#index)),
            None => quote!(::core::option::Option::None),
        };
        let (row, col, rows, cols) = (
            slot.position.row, slot.position.col, slot.position.rows, slot.position.cols,
        );
        quote!(::xrack_runtime::GridCellSlot {
            index: #index, child_index: #child_index,
            position: ::xrack_runtime::GridCellPosition { row: #row, col: #col, rows: #rows, cols: #cols },
        })
    });
    quote!(#node.set_static_grid(&[#(#slots),*]);)
}
