# Elfie for VS Code

A small extension that runs `elfie lsp` as the language server for `.lfy` files and adds
syntax highlighting, comment toggling, and bracket pairing.

## Setup

1. Build the compiler once from the repository root:

   ```bash
   cargo build --release -p elfie
   ```

   The executable is `target/release/elfie`. Put it on your `PATH`, or note its absolute
   path for step 3.

2. Install the extension. Either package it and install the `.vsix`:

   ```bash
   cd editors/vscode && npm install && npm run package
   code --install-extension elfie-0.1.0.vsix
   ```

   or, for development, link the folder into your extensions directory and reload VS Code:

   ```bash
   cd editors/vscode && npm install --omit=dev
   ln -s "$PWD" ~/.vscode/extensions/elfie.elfie-0.1.0        # WSL remote: ~/.vscode-server/extensions
   ```

3. Point VS Code at the executable when it is not on `PATH`. In your user or workspace
   settings (`.vscode/settings.json`):

   ```json
   {
     "elfie.path": "/absolute/path/to/target/release/elfie",
     "elfie.root": ""
   }
   ```

   `elfie.root` is passed as `--root`; leave it empty to use the first workspace folder,
   which should be the directory holding `elfie.json`.

Formatting: **Format Document** and format on save use the server, which applies the same
rules as `elfie format`; the extension sets itself as the default formatter for `.lfy` files.
**Elfie: Format File with elfie format** (also in the editor context menu) runs the command
line on the file instead, which works even when the server is stopped.

Open any `.lfy` file. Diagnostics, hover, go to definition, references, rename, completion,
document and workspace symbols, and formatting come from the server. The command
**Elfie: Restart Language Server** restarts it after rebuilding; `elfie.trace.server` set to
`verbose` shows every message in the *Elfie Language Server* output channel.

## Working in WSL

When VS Code runs on Windows and the repository lives in WSL, the extension has to be
installed in the WSL extension host: run the `code --install-extension` command from a WSL
shell (the `code` command forwards to the Windows VS Code and installs into the remote), and
use the WSL path of the executable in `elfie.path`.
