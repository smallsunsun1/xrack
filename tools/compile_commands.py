#!/usr/bin/env python3
"""Export first-party C/C++ compile actions (including generated ANTLR sources)."""

import json
import os
from pathlib import Path
import subprocess


ROOT = Path(__file__).resolve().parents[1]
NATIVE = ["//native/...", "//grammar/...", "//parser/...", "//mlir/..."]
SUFFIXES = {".c", ".cc", ".cpp", ".cxx", ".C", ".m", ".mm"}


def bazel(*args):
    return subprocess.check_output(["bazel", *args], cwd=ROOT, text=True)


def expand_arguments(action):
    response_files = {
        file["execPath"]: file["arguments"] for file in action.get("paramFiles", [])
    }
    return [
        value
        for arg in action["arguments"]
        for value in (response_files.get(arg[1:], [arg]) if arg.startswith("@") else [arg])
    ]


def macos_driver(args, action, execution_root):
    if Path(args[0]).name not in {"wrapped_clang", "wrapped_clang_pp"}:
        return args
    env = {item["key"]: item["value"] for item in action["environmentVariables"]}
    replacements = {
        "__BAZEL_XCODE_SDKROOT__": env["SDKROOT"],
        "__BAZEL_XCODE_DEVELOPER_DIR__": env["DEVELOPER_DIR"],
        "__BAZEL_EXECUTION_ROOT__": str(execution_root),
    }
    for i, arg in enumerate(args):
        for old, new in replacements.items():
            arg = arg.replace(old, new)
        args[i] = arg
    driver = "clang++" if Path(args[0]).name == "wrapped_clang_pp" else "clang"
    args[0] = subprocess.check_output(
        ["/usr/bin/xcrun", "--find", driver],
        env={**os.environ, "DEVELOPER_DIR": env["DEVELOPER_DIR"]},
        text=True,
    ).strip()
    return args


def main():
    # Materialize ANTLR/TableGen outputs and third-party headers before indexing.
    subprocess.run(["bazel", "build", *NATIVE], cwd=ROOT, check=True)
    execution_root = Path(bazel("info", "execution_root").strip())
    query = 'mnemonic("CppCompile", ' + " + ".join(NATIVE) + ")"
    graph = json.loads(bazel("aquery", query, "--output=jsonproto", "--include_param_files"))
    commands = {}
    for action in graph.get("actions", []):
        args = expand_arguments(action)
        sources = [
            arg for arg in args[1:]
            if not arg.startswith("-") and Path(arg).suffix in SUFFIXES
            and (execution_root / arg).is_file()
        ]
        if not sources:
            continue
        source = (execution_root / sources[-1]).resolve()
        if not source.is_relative_to(ROOT) and not sources[-1].startswith("bazel-out/"):
            continue
        args = macos_driver(args, action, execution_root)
        commands[str(source)] = {
            "directory": str(execution_root),
            "file": str(source),
            "arguments": args,
        }
    if not commands:
        raise SystemExit("Bazel did not report any XRack C/C++ compile actions")
    result = ROOT / "compile_commands.json"
    temporary = result.with_suffix(".json.tmp")
    temporary.write_text(json.dumps([commands[key] for key in sorted(commands)], indent=2) + "\n")
    temporary.replace(result)
    print(f"Wrote {len(commands)} compile commands to {result}")


if __name__ == "__main__":
    main()
