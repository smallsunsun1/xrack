# XRack for VS Code

The extension adds definition navigation, semantic highlighting, context-sensitive completions, diagnostics, a component outline, and live previews for `.xr` files. The Rust server reuses the XRack parser and semantic analyzer.

Use **Go to Definition** (F12 or Ctrl/Cmd-click) on properties, component IDs, callbacks, parameters, repeat bindings, record fields, and imported types. Hover for symbol information, or place the cursor on a symbol to highlight its occurrences in the current file. Semantic highlighting distinguishes declared components, records, built-in types, enum variants, properties, callbacks, and local bindings. The TextMate grammar provides highlighting while the server starts.

Use **Find All References** or **Rename Symbol** (F2) to work across the open workspace's `.xr` sources and their imports. Renaming an explicit import alias updates the alias and its local uses. Renaming an exported component updates the original name in import clauses and leaves explicit aliases intact. Rename checks the affected scopes and rejects edits that introduce a declaration conflict or change which declaration a reference resolves to.

The server reports progress while indexing workspace files in the background. Unsaved edits take precedence over disk contents. Rapid changes are grouped before diagnostics and indexing run. File watcher events update the affected entries, and closing a document restores its saved contents. Reference and rename requests wait for pending index updates and can be cancelled while waiting.

The workspace index is cached in VS Code's extension storage. On restart XRack checks saved file contents and import paths before reusing references. Changed files and affected importers are rebuilt. Unsaved buffers stay in memory.

Definition and rename use the compiler's resolved declarations where available, including inherited members. The editor index continues to provide navigation while a file is incomplete. Use **Go to Symbol in Workspace** to find components and records. In an element or type position, completion can offer an exported declaration from another file and insert its import when you accept it.

Callback calls show parameter names and types as you type, including the active argument in nested calls. Ctrl/Cmd-click an import path to open its file. When a diagnostic reports a missing component, use **Quick Fix** to import an exported component with that name.

Use **Format Document** to format a `.xr` file. The formatter uses the editor's tab size and space settings and preserves comments and string literals. The XRack repository enables formatting on save. To enable it in another workspace, add these settings to its `.vscode/settings.json`.

```json
{
  "[xrack]": {
    "editor.defaultFormatter": "SunJiahe.xrack",
    "editor.formatOnSave": true
  }
}
```

Type `root.` or `self.` for properties and callbacks in scope, or press **Ctrl+Space** to complete element types, bindings, event handlers and enum values. If `import { Card as Tile } from "widgets.xr";` introduces a component, `card := Tile { }` makes its public properties and callbacks available after `card.`. The instance body also suggests its input properties. Open files are read from the editor buffer, so unsaved changes to exported members show up in completions.

With the `from "widgets.xr"` path in place, completion inside the import braces suggests exported components and records. A preceding font import can stay in the same file. In `for item[index] in root.people : Text { ... }`, a declared `[Person]` model lets `item.` suggest the record's fields and shows `index` as an `int`.

Install the VSIX for your platform with **Extensions → Install from VSIX…** and open an `.xr` file. The platform package includes `xrack-lsp`, `xrackc`, the `xrack-preview` host, and runtime sources for the generated-code fallback.

## Live preview

Open a saved `.xr` file and choose **XRack: Open Live Preview** from the Command Palette or use the preview button in the editor title bar. The side panel follows edits to that file and its open imports, including unsaved edits. Use the component dropdown to choose an entry component. Click buttons in the preview to run their XRack handlers and see the updated component state. While a new frame is building, the previous frame stays visible. Compiler errors appear above it and clear automatically after the next successful render.

For supported components, the bundled preview host loads optimized XR MLIR into the XRack interpreter and keeps the FemtoVG/wgpu renderer alive across edits. Click a text input to type or paste into it, and click a slider track to change its value. Use the wheel to scroll a viewport or click its scrollbar track. Supported transitions and animated GIFs play in the panel. PNG, SVG, sprite atlases, and nine-slice images use the same renderer as compiled components. Source edits create a fresh component instance, and a failed edit leaves the previous frame available. This path requires a working graphics adapter. Components that use operations the interpreter does not yet support use the generated-code preview, which requires Cargo and a compatible Rust toolchain. In a source checkout, build the preview host with `bazel build //crates/xrack-preview`. Use `xrack.preview.compilerPath` to configure the compiler for the generated-code preview.

Drag the canvas background to move the preview. Turn on **Pan** or hold **Alt** to drag directly on the component. Use **−**, **+**, or **Alt + wheel** to zoom, the percentage button to return to actual size, and **Fit** to keep the component centered and within the panel. Trackpad pinch also zooms the view in Pan mode. These controls change the view of the rendered component while preserving its layout and state.

Window presentation commands update the preview component's window state inside the panel. If the component calls `minimize()`, use **Restore preview window** to show it again.

## Development

From the XRack repository root, build the server and install the extension dependencies.

```sh
bazel build //crates/xrack-lsp:xrack-lsp //:xrackc
npm --prefix crates/xrack-lsp/vscode ci
code crates/xrack-lsp/vscode
```

Press **F5** in that VS Code window to open an Extension Development Host at the XRack repository root. Open `examples/counter/counter.xr`. Change `count * 2` in `doubled` to `missing * 2` to see a diagnostic without saving. Undo the change to clear it. The Outline view shows `Counter`.

The extension first uses its bundled server if available. In development, it looks for the Bazel build under the workspace's `bazel-bin/crates/xrack-lsp/` directory and then for `xrack-lsp` on `PATH`. Set `xrack.server.path` to override the executable, then reload the window. The server uses stdio and can also be launched by other LSP clients.

Semantic diagnostics follow direct and transitive imports. Open buffers take precedence over disk, and changing or closing a dependency refreshes diagnostics for the files that import it. VS Code also reports changes to `.xr` files on disk. When an error is in an imported file, the dependent file shows a diagnostic with a link to the original location. Run `xrackc --check` for MLIR and resource checks.

```sh
bazel test //crates/xrack-lsp:xrack_lsp_test
```

## Packaging and Marketplace releases

GitHub Actions builds and smoke-tests all five VSIX files on pushes to `main`. You can also enable `package-vsix` on a manual CI run. The packages target `darwin-arm64`, `linux-x64`, `linux-arm64`, `win32-x64`, and `win32-arm64`. Each includes the host's compiler, language server, preview executable, and fallback runtime sources.

To build a native VSIX locally, run the following from the repository root with Python 3.11+ and Node.js 22+. Set `host` to the native Rust host triple shown by `rustc -vV`.

```sh
host=aarch64-apple-darwin
python3 tools/package_xrackc.py
python3 tools/package_vscode.py package --host "$host" --archives .tools/dist --output .tools/vsix
```

### Configure Trusted Publishing

1. Sign in to [Visual Studio Marketplace publisher management](https://marketplace.visualstudio.com/manage/publishers/) and create or select the publisher. Its ID must match `publisher` in this extension's `package.json`, currently `SunJiahe`.
2. In [the GitHub repository's Environments settings](https://github.com/smallsunsun1/xrack/settings/environments), create `vscode-marketplace`. If you restrict deployments to selected branches and tags, allow release tags such as `v0.5.0`.
3. In the Marketplace publisher, open **Trusted Publishing** and create the following policy.

   | Field | Value |
   | --- | --- |
   | Policy name | XRack VS Code release |
   | Repository owner | `smallsunsun1` |
   | Repository | `xrack` |
   | Workflow | `release-crates.yml` |
   | Environment | `vscode-marketplace` |

The publish job uses GitHub OIDC via `vsce publish --oidc`. The policy supplies its publishing authorization. [Trusted Publishing documentation](https://code.visualstudio.com/api/working-with-extensions/publishing-extension#publish-with-github-actions-and-trusted-publishing)

### Publish a version

Set the extension version in `package.json` and `package-lock.json` to match the Cargo workspace. Once CI passes on the release commit on `main`, push the matching tag.

```sh
git fetch origin
git tag -a v0.5.0 origin/main -m "XRack 0.5.0"
git push origin v0.5.0
```

**Release XRack** checks the tag and downloads the verified artifacts from the successful native matrix on that exact commit. If an upload fails after some targets have been published, rerun the failed jobs to finish the release.
