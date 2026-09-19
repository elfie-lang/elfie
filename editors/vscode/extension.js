// The Elfie extension: starts `elfie lsp --root <root>` and connects VS Code to it.
const vscode = require("vscode");
const { LanguageClient, TransportKind } = require("vscode-languageclient/node");

let client;

function rootOf() {
  const configured = vscode.workspace.getConfiguration("elfie").get("root");
  if (configured) return configured;
  const folders = vscode.workspace.workspaceFolders || [];
  return folders.length > 0 ? folders[0].uri.fsPath : undefined;
}

function start(context) {
  const configuration = vscode.workspace.getConfiguration("elfie");
  const root = rootOf();
  // `${workspaceFolder}` in elfie.path stands for the first workspace folder.
  const command = (configuration.get("path") || "elfie").replace("${workspaceFolder}", root || "");
  const args = root ? ["lsp", "--root", root] : ["lsp"];
  const serverOptions = {
    run: { command, args, transport: TransportKind.stdio },
    debug: { command, args, transport: TransportKind.stdio },
  };
  const clientOptions = {
    documentSelector: [{ scheme: "file", language: "elfie" }],
    synchronize: {
      fileEvents: [
        vscode.workspace.createFileSystemWatcher("**/*.lfy"),
        vscode.workspace.createFileSystemWatcher("**/elfie.json"),
      ],
    },
    outputChannelName: "Elfie Language Server",
  };
  client = new LanguageClient("elfie", "Elfie Language Server", serverOptions, clientOptions);
  context.subscriptions.push(client);
  return client.start().catch((error) => {
    vscode.window.showErrorMessage(
      `Elfie: could not start '${command} lsp' (${error.message}). ` +
        "Build it with 'cargo build --release' and set 'elfie.path' to the executable."
    );
  });
}

/// Runs `elfie format <file>` on the active document: the same layout the server gives
/// Format Document, for when the server is stopped or a file is outside the program.
async function formatFile() {
  const editor = vscode.window.activeTextEditor;
  if (!editor || editor.document.languageId !== "elfie") return;
  await editor.document.save();
  const root = rootOf();
  const command = (vscode.workspace.getConfiguration("elfie").get("path") || "elfie").replace("${workspaceFolder}", root || "");
  const { execFile } = require("child_process");
  await new Promise((resolve) => {
    execFile(command, ["format", editor.document.uri.fsPath], { cwd: root }, (error, stdout, stderr) => {
      if (error && error.code !== 1) vscode.window.showErrorMessage(`elfie format: ${stderr || error.message}`);
      else if (stdout.trim()) vscode.window.setStatusBarMessage(stdout.trim(), 3000);
      resolve();
    });
  });
}

async function activate(context) {
  await start(context);
  context.subscriptions.push(vscode.commands.registerCommand("elfie.formatFile", formatFile));
  context.subscriptions.push(
    vscode.commands.registerCommand("elfie.restartServer", async () => {
      if (client) await client.stop();
      await start(context);
    })
  );
}

function deactivate() {
  return client ? client.stop() : undefined;
}

module.exports = { activate, deactivate };
