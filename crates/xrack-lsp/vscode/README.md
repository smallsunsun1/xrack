# XRack for VS Code

The extension adds `.xr` highlighting, context-sensitive completions, compiler diagnostics for unsaved edits, and a component outline. The Rust server reuses the XRack parser and semantic analyzer.

Type `root.` or `self.` for properties and callbacks in scope, or press **Ctrl+Space** to complete element types, bindings, event handlers and enum values. If `import { Card as Tile } from "widgets.xr";` introduces a component, `card := Tile { }` makes its public properties and callbacks available after `card.`. The instance body also suggests its input properties. Open files are read from the editor buffer, so unsaved changes to exported members show up in completions.

Install the VSIX for your platform with **Extensions → Install from VSIX…** and open an `.xr` file. The package includes `xrack-lsp`, so this setup works without a local XRack build.

## Development

From the XRack repository root, build the server and install the extension dependencies.

```sh
bazel build //crates/xrack-lsp:xrack-lsp
npm --prefix crates/xrack-lsp/vscode ci
code crates/xrack-lsp/vscode
```

Press **F5** in that VS Code window to open an Extension Development Host at the XRack repository root. Open `examples/counter/counter.xr`. Change `count * 2` in `doubled` to `missing * 2` to see a diagnostic without saving. Undo the change to clear it. The Outline view shows `Counter`.

The extension first uses its bundled server if available. In development, it looks for the Bazel build under the workspace's `bazel-bin/crates/xrack-lsp/` directory and then for `xrack-lsp` on `PATH`. Set `xrack.server.path` to override the executable, then reload the window. The server uses stdio and can also be launched by other LSP clients.

Semantic diagnostics in this version cover self-contained files. Files with imports get syntax diagnostics and an outline when they parse successfully. Run `xrackc --check` for cross-file, MLIR and resource checks.

```sh
bazel test //crates/xrack-lsp:xrack_lsp_test
```
