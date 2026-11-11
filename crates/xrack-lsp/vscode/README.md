# XRack for VS Code

The extension adds definition navigation, semantic highlighting, context-sensitive completions, diagnostics, a component outline, and live previews for `.xr` files. The Rust server reuses the XRack parser and semantic analyzer.

Use **Go to Definition** (F12 or Ctrl/Cmd-click) on properties, component IDs, callbacks, parameters, repeat bindings, record fields, and imported types. Hover for symbol information, or place the cursor on a symbol to highlight its occurrences in the current file. Semantic highlighting distinguishes declared components, records, built-in types, enum variants, properties, callbacks, and local bindings. The TextMate grammar provides highlighting while the server starts.

Type `root.` or `self.` for properties and callbacks in scope, or press **Ctrl+Space** to complete element types, bindings, event handlers and enum values. If `import { Card as Tile } from "widgets.xr";` introduces a component, `card := Tile { }` makes its public properties and callbacks available after `card.`. The instance body also suggests its input properties. Open files are read from the editor buffer, so unsaved changes to exported members show up in completions.

With the `from "widgets.xr"` path in place, completion inside the import braces suggests exported components and records. A preceding font import can stay in the same file. In `for item[index] in root.people : Text { ... }`, a declared `[Person]` model lets `item.` suggest the record's fields and shows `index` as an `int`.

Install the VSIX for your platform with **Extensions → Install from VSIX…** and open an `.xr` file. The platform package includes `xrack-lsp`, `xrackc`, and a small Cargo workspace for the preview runtime.

## Live preview

Open a saved `.xr` file and choose **XRack: Open Live Preview** from the Command Palette or use the preview button in the editor title bar. The side panel follows edits to that file and its open imports, including unsaved edits. Use the component dropdown to choose an entry component. Click buttons in the preview to run their XRack handlers and see the updated component state. While a new frame is building, the previous frame stays visible. Compiler errors appear above it and clear automatically after the next successful render.

The panel uses the XRack compiler and the same FemtoVG/wgpu renderer as the desktop runtime. The renderer stays active between clicks and component selections. Edits that produce identical Rust reuse the current state, while edits that change the generated code compile and start a new component. Preview requires Cargo, a compatible Rust toolchain, and a working graphics adapter. The first preview may take longer while Cargo builds its dependencies. In an XRack checkout the preview uses the local runtime crates. The platform package supplies the matching runtime source for other projects, and Cargo fetches its third-party dependencies. Set `xrack.preview.compilerPath` to override the checkout's Bazel build, the bundled compiler, or `xrackc` on PATH.

## Development

From the XRack repository root, build the server and install the extension dependencies.

```sh
bazel build //crates/xrack-lsp:xrack-lsp //:xrackc
npm --prefix crates/xrack-lsp/vscode ci
code crates/xrack-lsp/vscode
```

Press **F5** in that VS Code window to open an Extension Development Host at the XRack repository root. Open `examples/counter/counter.xr`. Change `count * 2` in `doubled` to `missing * 2` to see a diagnostic without saving. Undo the change to clear it. The Outline view shows `Counter`.

The extension first uses its bundled server if available. In development, it looks for the Bazel build under the workspace's `bazel-bin/crates/xrack-lsp/` directory and then for `xrack-lsp` on `PATH`. Set `xrack.server.path` to override the executable, then reload the window. The server uses stdio and can also be launched by other LSP clients.

Semantic diagnostics in this version cover self-contained files. Files with imports get syntax diagnostics and an outline when they parse successfully. Run `xrackc --check` for cross-file, MLIR and resource checks.

```sh
bazel test //crates/xrack-lsp:xrack_lsp_test
```

## Packaging and Marketplace releases

The GitHub Actions **CI** workflow accepts a `package-vsix` input when run manually. Enable it to build and smoke-test all five VSIX files and download them as Actions artifacts. The packages target `darwin-arm64`, `linux-x64`, `linux-arm64`, `win32-x64`, and `win32-arm64`. Each includes the host's compiler and language server plus the preview runtime source.

To build a native VSIX locally, run the following from the repository root with Python 3.11+ and Node.js 22+. Set `host` to the native Rust host triple shown by `rustc -vV`.

```sh
host=aarch64-apple-darwin
python3 tools/package_xrackc.py
python3 tools/package_vscode.py package --host "$host" --archives .tools/dist --output .tools/vsix
```

### Configure Trusted Publishing

1. Sign in to [Visual Studio Marketplace publisher management](https://marketplace.visualstudio.com/manage/publishers/) and create or select the publisher. Its ID must match `publisher` in this extension's `package.json`, currently `xrack`.
2. In [the GitHub repository's Environments settings](https://github.com/smallsunsun1/xrack/settings/environments), create `vscode-marketplace`. If you restrict deployments to selected branches and tags, allow the tag pattern `vscode-v*`.
3. In the Marketplace publisher, open **Trusted Publishing** and create the following policy.

   | Field | Value |
   | --- | --- |
   | Policy name | XRack VS Code release |
   | Repository owner | `smallsunsun1` |
   | Repository | `xrack` |
   | Workflow | `release-vscode.yml` |
   | Environment | `vscode-marketplace` |

The publish job uses GitHub OIDC via `vsce publish --oidc`. The policy supplies its publishing authorization. [Trusted Publishing documentation](https://code.visualstudio.com/api/working-with-extensions/publishing-extension#publish-with-github-actions-and-trusted-publishing)

### Publish a version

Set the extension version in `package.json` and `package-lock.json`, merge the release commit into `main`, then push the matching tag. For version `0.4.2`, run the following once `origin/main` points to the intended release commit.

```sh
git fetch origin
git tag -a vscode-v0.4.2 origin/main -m "XRack for VS Code 0.4.2"
git push origin vscode-v0.4.2
```

**Release XRack for VS Code** checks the tag and runs the native matrix. Once all five artifacts pass verification, it publishes them under one extension version. If an upload fails after some targets have been published, rerun the same workflow to skip those targets and finish the release. Crate releases use their existing `v…` tags.
