"""Generate C++ with the ANTLR jar using only a Java runtime toolchain."""

def _antlr_cpp_impl(ctx):
    runtime = ctx.toolchains["@bazel_tools//tools/jdk:runtime_toolchain_type"].java_runtime
    args = ctx.actions.args()
    args.add_all([
        "-jar",
        ctx.file._jar.path,
        "-Dlanguage=Cpp",
        "-no-listener",
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
        mnemonic = "Antlr4Cpp",
        progress_message = "Generating ANTLR4 C++ parser from " + ctx.file.src.short_path,
    )
    return [DefaultInfo(files = depset(ctx.outputs.outs))]

antlr_cpp = rule(
    implementation = _antlr_cpp_impl,
    attrs = {
        "src": attr.label(allow_single_file = [".g4"], mandatory = True),
        "outs": attr.output_list(mandatory = True),
        "_jar": attr.label(default = "@antlr4_tool//file", allow_single_file = True),
    },
    toolchains = ["@bazel_tools//tools/jdk:runtime_toolchain_type"],
)
