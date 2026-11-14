"""Build against the pinned LLVM server with XRack document hooks."""

def _lsp_sources_impl(ctx):
    for label in ctx.attr.srcs:
        source = ctx.path(label)
        ctx.file(source.basename, ctx.read(source))
    ctx.patch(ctx.attr.patch, strip = 1)
    ctx.file("BUILD.bazel", 'exports_files(glob(["*.cpp", "*.h"]))\n')

mlir_lsp_sources = repository_rule(
    implementation = _lsp_sources_impl,
    attrs = {
        "patch": attr.label(allow_single_file = [".patch"], mandatory = True),
        "srcs": attr.label_list(allow_files = True, mandatory = True),
    },
)
