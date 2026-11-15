"""Register only Ruff for the Python formatter."""

load("@rules_multitool//multitool/private:multitool.bzl", "hub")

def _python_tools_impl(_module_ctx):
    hub(
        name = "xrack_python_tools",
        lockfiles = [Label("//bindings/python:ruff.lock.json")],
    )

python_tools = module_extension(implementation = _python_tools_impl)
