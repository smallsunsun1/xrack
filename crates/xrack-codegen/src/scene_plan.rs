use crate::{children, scene_children};
use proc_macro2::{Ident, Literal, TokenStream};
use quote::quote;
use xrack_mlir::OperationRef;
use xrack_runtime::{GridPlanner, GridTrackInput, VectorSegment};

pub struct EmbeddedPath {
    pub bounds: [f32; 4],
    pub segments: Vec<VectorSegment>,
}

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

fn literal_binding<'a>(element: OperationRef<'a>, name: &str) -> Result<Option<&'a str>, String> {
    children(element, "xr.bind")
        .filter(|op| op.attribute("name") == Some(name))
        .last()
        .map(|binding| {
            binding
                .operand(0)
                .and_then(|value| value.defining_operation())
                .filter(|definition| definition.name() == "xr.literal")
                .ok_or_else(|| format!("static grid requires a literal {name} binding"))?
                .attr("value")
        })
        .transpose()
}

pub(crate) fn static_grid(element: OperationRef<'_>, node: &Ident) -> Result<TokenStream, String> {
    let mut planner = GridPlanner::default();
    let mut slots = Vec::new();
    let add = |planner: &mut GridPlanner,
               slots: &mut Vec<TokenStream>,
               cell,
               index,
               child_index|
     -> Result<(), String> {
        if literal_binding(cell, "visible")? == Some("false") {
            return Ok(());
        }
        let track = |name| {
            literal_binding(cell, name)?
                .map(|text| {
                    text.parse::<i64>()
                        .map_err(|error| format!("invalid static grid {name}: {error}"))
                })
                .transpose()
        };
        let input = GridTrackInput {
            row: track("row")?,
            col: track("col")?,
            rowspan: track("rowspan")?,
            colspan: track("colspan")?,
        };
        if let Some(position) = planner.place(input) {
            let child_index = match child_index {
                Some(index) => quote!(::core::option::Option::Some(#index)),
                None => quote!(::core::option::Option::None),
            };
            let (row, col, rows, cols) = (position.row, position.col, position.rows, position.cols);
            slots.push(quote!(::xrack_runtime::GridCellSlot {
                index: #index, child_index: #child_index,
                position: ::xrack_runtime::GridCellPosition { row: #row, col: #col, rows: #rows, cols: #cols },
            }));
        }
        Ok(())
    };
    for (index, child) in scene_children(element).enumerate() {
        if child.attr("kind")? == "Row" {
            if literal_binding(child, "visible")? == Some("false") {
                continue;
            }
            planner.begin_row();
            for (child_index, cell) in scene_children(child).enumerate() {
                add(&mut planner, &mut slots, cell, index, Some(child_index))?;
            }
            planner.end_row();
        } else {
            add(&mut planner, &mut slots, child, index, None)?;
        }
    }
    Ok(quote!(#node.set_static_grid(&[#(#slots),*]);))
}
