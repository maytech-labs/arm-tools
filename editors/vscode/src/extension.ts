// Starts armls for AArch64 assembly files. The extension is only glue: every
// feature is in armls (docs/decisions/0025-distribution.md).
import { ExtensionContext, window, workspace } from "vscode";
import { LanguageClient, LanguageClientOptions, ServerOptions } from "vscode-languageclient/node";

let client: LanguageClient | undefined;

export async function activate(context: ExtensionContext): Promise<void> {
  const command = workspace.getConfiguration("armTools").get<string>("serverPath", "armls");
  const serverOptions: ServerOptions = { command, args: [] };
  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "arm64-asm" }],
    synchronize: { fileEvents: workspace.createFileSystemWatcher("**/armtool.toml") },
  };
  client = new LanguageClient("armls", "armls", serverOptions, clientOptions);
  try {
    await client.start();
  } catch (error) {
    window.showErrorMessage(
      `armls didn't start (${String(error)}). Install it with \`make install\` from the arm-tools repo, or set armTools.serverPath.`,
    );
  }
  context.subscriptions.push({ dispose: () => void client?.stop() });
}

export async function deactivate(): Promise<void> {
  await client?.stop();
}
