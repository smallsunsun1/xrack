"""Clang and MLIR header directories in the Bazel LLVM source repository."""

LLVM_BUILTIN_HEADERS = Label("@llvm-project//clang:builtin_headers_files").workspace_root + "/clang/lib/Headers"
MLIR_C_HEADERS = Label("@llvm-project//mlir:CAPIIR").workspace_root + "/mlir/include"
