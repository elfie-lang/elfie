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

async function activate(context) {
  await start(context);
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
