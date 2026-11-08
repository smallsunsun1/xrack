"""Clang's builtin header directory in the Bazel LLVM source repository."""

LLVM_BUILTIN_HEADERS = Label("@llvm-project//clang:builtin_headers_files").workspace_root + "/clang/lib/Headers"
