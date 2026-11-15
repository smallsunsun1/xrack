"""Compile a .xr file to Rust and expose it as a normal rules_rust library."""

load("@rules_rust//rust:defs.bzl", "rust_library")

def _rust_generated_source_impl(ctx):
    files = depset([ctx.file.src])
    return [
        DefaultInfo(files = files),
        OutputGroupInfo(rust_generated_srcs = files),
    ]

# gen_rust_project requests this output group without compiling the Rust crate.
rust_generated_source = rule(
    implementation = _rust_generated_source_impl,
    attrs = {"src": attr.label(allow_single_file = [".rs"], mandatory = True)},
)

def xr_ui_library(name, src, visibility = None, assets = [], sources = [], live_reload = False):
    generated = name + ".rs"
    asset_args = "".join([" --asset \"$(location %s)\"" % asset for asset in assets])
    source_args = "".join([" --source \"$(location %s)\"" % file for file in sources])
    native.genrule(
        name = name + "_codegen",
        srcs = [src] + assets + sources,
        outs = [generated],
        tools = ["//crates/xrack-compiler:xrackc"],
        cmd = '"$(location //crates/xrack-compiler:xrackc)" --emit %s --restrict-assets --restrict-sources%s%s -o "$@" "$(location %s)"' % ("rust-live" if live_reload else "rust", asset_args, source_args, src),
    )
    rust_library(
        name = name,
        crate_name = name.replace("-", "_"),
        srcs = [generated],
        crate_root = generated,
        edition = "2024",
        # The compiler tests verify the generated source. Lint the generator itself.
        tags = ["no-clippy"],
        deps = ["//crates/xrack-runtime:xrack_runtime"] + (["//crates/xrack-preview:xrack_preview_live"] if live_reload else []),
        visibility = visibility,
    )
