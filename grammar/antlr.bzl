"""Generate the Rust ANTLR parser using the JDK from the Bazel toolchain."""

def _antlr_rust_impl(ctx):
    runtime = ctx.toolchains["@bazel_tools//tools/jdk:runtime_toolchain_type"].java_runtime
    args = ctx.actions.args()
    args.add_all([
        "-jar",
        ctx.file._jar.path,
        "-Dlanguage=Rust",
        # The 0.6.0 Rust target references the listener trait in its parser.
        "-listener",
        "-no-visitor",
        "-Xexact-output-dir",
        "-o",
        ctx.outputs.outs[0].dirname,
        ctx.file.src.path,
    ])
    ctx.actions.run(
        executable = runtime.java_executable_exec_path,
        arguments = [args],
        inputs = [ctx.file._jar, ctx.file.src],
        tools = [runtime.files],
        outputs = ctx.outputs.outs,
        mnemonic = "Antlr4Rust",
        progress_message = "Generating ANTLR4 Rust parser from " + ctx.file.src.short_path,
    )
    ctx.actions.write(
        ctx.outputs.crate_root,
        "pub mod xracklexer;\npub mod xracklistener;\npub mod xrackparser;\n",
    )
    files = depset(ctx.outputs.outs + [ctx.outputs.crate_root])
    return [DefaultInfo(files = files), OutputGroupInfo(rust_generated_srcs = files)]

antlr_rust = rule(
    implementation = _antlr_rust_impl,
    attrs = {
        "src": attr.label(allow_single_file = [".g4"], mandatory = True),
        "outs": attr.output_list(mandatory = True),
        "crate_root": attr.output(mandatory = True),
        "_jar": attr.label(default = "@antlr4_rust_tool//file", allow_single_file = True),
    },
    toolchains = ["@bazel_tools//tools/jdk:runtime_toolchain_type"],
)
