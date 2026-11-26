//! Generate the host-facing Rust API for development-mode components.

use crate::{Type, children, host_ty, ident, rust_name};
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};
use xrack_mlir::{Module, OperationRef};

pub fn generate_live(module: &Module, filename: &str, cargo: bool) -> Result<TokenStream, String> {
    module.verify()?;
    let mut output = TokenStream::new();
    for component in module
        .operations()
        .filter(|op| op.name() == "xr.component" && op.attribute("exported") == Some("true"))
    {
        let name = component.attr("sym_name")?;
        if component.attribute("global") == Some("true") {
            return Err(format!(
                "live development components do not expose global {name} yet"
            ));
        }
        let rust_name = ident(name);
        let open = if cargo {
            quote!(::xrack_preview::LiveComponent::open_bundled(
                #filename, #name, ::core::env!("XRACK_LIVE_COMPILER"), ::core::env!("CARGO_MANIFEST_DIR")
            ))
        } else {
            quote!(::xrack_preview::open_live(#filename, #name))
        };
        let mut methods = TokenStream::new();
        for property in children(component, "xr.property") {
            let visibility = property.attr("visibility")?;
            if visibility != "private" {
                methods.extend(property_methods(
                    property,
                    matches!(visibility, "in" | "in-out"),
                )?);
            }
        }
        for callback in children(component, "xr.callback") {
            methods.extend(callback_methods(callback)?);
        }
        output.extend(quote! {
            pub struct #rust_name(::xrack_preview::LiveComponent<Self>);

            impl ::xrack_preview::LiveUi for #rust_name {
                fn from_live(component: ::xrack_preview::LiveComponent<Self>) -> Self {
                    Self(component)
                }
            }

            impl ::core::default::Default for #rust_name {
                fn default() -> Self {
                    Self::new()
                }
            }

            #[allow(dead_code, clippy::clone_on_copy)]
            impl #rust_name {
                pub fn try_new() -> ::core::result::Result<Self, ::std::string::String> {
                    (#open).map(Self)
                }

                pub fn new() -> Self {
                    Self::try_new().unwrap_or_else(|error| ::core::panic!("XRack live reload: {}", error))
                }

                pub fn as_weak(&self) -> ::xrack_runtime::WeakComponent<Self> {
                    self.0.as_weak()
                }

                pub fn render(&self) -> ::xrack_runtime::Node {
                    ::xrack_runtime::Component::render(self)
                }

                fn __xr_component(&self) -> ::xrack_preview::ReloadableInstance {
                    self.0.component()
                }

                #methods
            }

            impl ::xrack_runtime::Component for #rust_name {
                fn render(&self) -> ::xrack_runtime::Node {
                    self.__xr_component().render()
                }

                fn render_shared(&self) -> ::std::rc::Rc<::xrack_runtime::Node> {
                    self.__xr_component().render_shared()
                }

                fn scene_dirty(&self) -> ::core::primitive::bool {
                    self.__xr_component().scene_dirty()
                }

                fn set_locale_context(&self, context: ::xrack_runtime::LocaleContext) {
                    self.__xr_component().set_locale_context(context);
                }

                fn publish_window_status(&self, status: ::xrack_runtime::WindowStatus) {
                    self.__xr_component().publish_window_status(status);
                }

                ::xrack_preview::__xr_forward_component_methods!(self, __xr_component);
            }
        });
    }
    Ok(output)
}

fn operand_type(op: OperationRef<'_>) -> Result<Type, String> {
    op.operand(0)
        .ok_or_else(|| format!("{} has no value operand", op.name()))?
        .xr_type()
}

fn value_variant(ty: &Type) -> Result<Ident, String> {
    let variant = match ty {
        Type::Bool => "Bool",
        Type::Int => "Int",
        Type::Float => "Float",
        Type::String => "String",
        Type::StringList => "StringList",
        Type::IntList => "IntList",
        Type::DateList => "DateList",
        Type::Date => "Date",
        Type::Length => "Length",
        Type::Point2D => "Point2D",
        Type::Point3D => "Point3D",
        Type::Vector3D => "Vector3D",
        Type::Quaternion3D => "Quaternion3D",
        Type::RotationTrack3D => "RotationTrack3D",
        Type::Hit3D => "Hit3D",
        Type::Hit3DList => "Hit3DList",
        Type::PhysicsContact3D => "PhysicsContact3D",
        Type::PhysicsHit3D => "PhysicsHit3D",
        Type::PhysicsContact3DList => "PhysicsContact3DList",
        Type::PhysicsHit3DList => "PhysicsHit3DList",
        Type::ProjectedPoint3D => "ProjectedPoint3D",
        Type::Instance3D => "Instance3D",
        Type::InstanceTable3D => "InstanceTable3D",
        Type::MeshAsset3D => "MeshAsset3D",
        Type::MeshNode3D => "MeshNode3D",
        Type::Bounds3D => "Bounds3D",
        Type::WalkResult3D => "WalkResult3D",
        Type::PhysicsWorld3D => "PhysicsWorld3D",
        Type::SceneModel3D => "SceneObjects",
        Type::Vector2D => "Vector2D",
        Type::Transform2D => "Transform2D",
        Type::Color => "Color",
        _ => {
            return Err(format!(
                "live development bindings do not support {}",
                ty.name()
            ));
        }
    };
    Ok(format_ident!("{variant}"))
}

fn pack_value(ty: &Type, variant: &Ident, source: TokenStream) -> TokenStream {
    let source = if matches!(
        ty,
        Type::Hit3DList | Type::PhysicsContact3DList | Type::PhysicsHit3DList
    ) {
        quote!(::xrack_runtime::VecModel::from(#source))
    } else {
        source
    };
    quote!(::xrack_runtime::Value::#variant(#source))
}

fn unpack_value(ty: &Type, source: TokenStream) -> TokenStream {
    if matches!(
        ty,
        Type::Hit3DList | Type::PhysicsContact3DList | Type::PhysicsHit3DList
    ) {
        quote!((#source).into_snapshot())
    } else {
        source
    }
}

fn property_methods(property: OperationRef<'_>, writable: bool) -> Result<TokenStream, String> {
    let name = property.attr("name")?;
    let name_rust = rust_name(name);
    let ty = operand_type(property)?;
    let rust_type = host_ty(&ty);
    let variant = value_variant(&ty)?;
    let get = format_ident!("get_{name_rust}");
    let try_get = format_ident!("try_get_{name_rust}");
    let snapshot = unpack_value(&ty, quote!(value));
    let mut methods = quote! {
        pub fn #try_get(&self) -> ::core::result::Result<#rust_type, ::std::string::String> {
            let value = self.__xr_component().current().get_snapshot(#name)
                .map_err(|error| error.to_string())?;
            match value {
                ::xrack_runtime::Value::#variant(value) => ::core::result::Result::Ok(#snapshot),
                _ => ::core::result::Result::Err(::std::format!("XRack live property {} has an incompatible value", #name)),
            }
        }

        pub fn #get(&self) -> #rust_type {
            self.#try_get().unwrap_or_else(|error| ::core::panic!("XRack live property {}: {}", #name, error))
        }
    };
    if writable {
        let set = format_ident!("set_{name_rust}");
        let reset = format_ident!("reset_{name_rust}");
        let packed = pack_value(&ty, &variant, quote!(value));
        methods.extend(quote! {
            pub fn #set(&mut self, value: #rust_type) {
                self.__xr_component().current().set(#name, #packed).unwrap();
            }

            pub fn #reset(&mut self) {
                self.__xr_component().current().reset(#name).unwrap();
            }
        });
    }
    if matches!(
        ty,
        Type::StringList
            | Type::IntList
            | Type::DateList
            | Type::Hit3DList
            | Type::PhysicsContact3DList
            | Type::PhysicsHit3DList
    ) {
        let model = format_ident!("model_{name_rust}");
        let (item, access, value) = if ty == Type::IntList {
            (
                quote!(::core::primitive::i64),
                quote!(int_model),
                quote!(::xrack_runtime::Value::IntModel(model)),
            )
        } else if ty == Type::DateList {
            (
                quote!(::xrack_runtime::Date),
                quote!(date_model),
                quote!(::xrack_runtime::Value::DateModel(model)),
            )
        } else if ty == Type::Hit3DList {
            (
                quote!(::xrack_runtime::Hit3D),
                quote!(hit3d_model),
                quote!(::xrack_runtime::Value::Hit3DList(model)),
            )
        } else if ty == Type::PhysicsContact3DList {
            (
                quote!(::xrack_runtime::PhysicsContact3D),
                quote!(physics_contact3d_model),
                quote!(::xrack_runtime::Value::PhysicsContact3DList(model)),
            )
        } else if ty == Type::PhysicsHit3DList {
            (
                quote!(::xrack_runtime::PhysicsHit3D),
                quote!(physics_hit3d_model),
                quote!(::xrack_runtime::Value::PhysicsHit3DList(model)),
            )
        } else {
            (
                quote!(::std::string::String),
                quote!(string_model),
                quote!(model.into()),
            )
        };
        methods.extend(quote! {
            pub fn #model(&self) -> ::xrack_runtime::VecModel<#item> {
                self.__xr_component().current().#access(#name).unwrap()
            }
        });
        if writable {
            let bind = format_ident!("bind_model_{name_rust}");
            methods.extend(quote! {
                pub fn #bind(&mut self, model: ::xrack_runtime::VecModel<#item>) {
                    self.__xr_component().current().set(#name, #value).unwrap();
                }
            });
        }
    }
    Ok(methods)
}

fn callback_methods(callback: OperationRef<'_>) -> Result<TokenStream, String> {
    let name = callback.attr("name")?;
    let name_rust = rust_name(name);
    let on = format_ident!("on_{name_rust}");
    let invoke = format_ident!("invoke_{name_rust}");
    let try_invoke = format_ident!("try_invoke_{name_rust}");
    let parameters = callback.parameters()?;
    let types: Vec<_> = parameters
        .iter()
        .map(|parameter| host_ty(&parameter.ty))
        .collect();
    let variants = parameters
        .iter()
        .map(|parameter| value_variant(&parameter.ty))
        .collect::<Result<Vec<_>, _>>()?;
    let args: Vec<_> = (0..parameters.len())
        .map(|index| format_ident!("arg{index}"))
        .collect();
    let result = children(callback, "xr.return")
        .next()
        .map(operand_type)
        .transpose()?;
    let returns = result.as_ref().map(host_ty).unwrap_or_else(|| quote!(()));
    let (pack, unpack) = if let Some(ty) = &result {
        let variant = value_variant(ty)?;
        let packed = pack_value(ty, &variant, quote!(value));
        let snapshot = unpack_value(ty, quote!(value));
        (
            quote!(::core::result::Result::Ok(::core::option::Option::Some(
                #packed
            ))),
            quote! {
                match result {
                    ::core::option::Option::Some(::xrack_runtime::Value::#variant(value)) => ::core::result::Result::Ok(#snapshot),
                    _ => ::core::result::Result::Err(::std::format!("XRack live callback {} returned an incompatible value", #name)),
                }
            },
        )
    } else {
        (
            quote!(::core::result::Result::Ok(::core::option::Option::None)),
            quote! { let _ = result; ::core::result::Result::Ok(()) },
        )
    };
    let unpacked_args = parameters
        .iter()
        .zip(&args)
        .map(|(parameter, arg)| unpack_value(&parameter.ty, quote!(#arg.clone())));
    let args_value = quote!(#(#unpacked_args),*);
    let packed_args = parameters
        .iter()
        .zip(&args)
        .zip(&variants)
        .map(|((parameter, arg), variant)| pack_value(&parameter.ty, variant, quote!(#arg)));
    let (call, bind, bound) = if callback.attribute("pure") == Some("true") {
        (
            quote!(callback(#args_value)),
            TokenStream::new(),
            quote!(::core::ops::Fn),
        )
    } else {
        (
            quote! {
                callback.try_call(|handler| handler(#args_value))
                    .map_err(|error| error.to_string())?
            },
            quote!(let callback = ::xrack_runtime::EffectfulCallback::new(callback);),
            quote!(::core::ops::FnMut),
        )
    };
    Ok(quote! {
        pub fn #on(&mut self, callback: impl #bound(#(#types),*) -> #returns + 'static) {
            #bind
            self.__xr_component().on(#name, ::std::rc::Rc::new(move |arguments| {
                let [#(::xrack_runtime::Value::#variants(#args)),*] = arguments else {
                    return ::core::result::Result::Err(::std::format!("XRack live callback {} received incompatible arguments", #name));
                };
                #[allow(unused_variables)]
                let value = #call;
                #pack
            })).unwrap();
        }

        pub fn #try_invoke(&mut self #(, #args: #types)*)
            -> ::core::result::Result<#returns, ::std::string::String>
        {
            let result = self.__xr_component().current()
                .invoke(#name, &[#(#packed_args),*])
                .map_err(|error| error.to_string())?;
            #unpack
        }

        pub fn #invoke(&mut self #(, #args: #types)*) -> #returns {
            self.#try_invoke(#(#args),*)
                .unwrap_or_else(|error| ::core::panic!("XRack live callback {}: {}", #name, error))
        }
    })
}
