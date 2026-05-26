const cp = require("child_process");
const fs = require("fs");
const path = require("path");
const vscode = require("vscode");

let diagnostics;
let extensionPath;

function activate(context) {
  extensionPath = context.extensionPath;
  diagnostics = vscode.languages.createDiagnosticCollection("cfc");
  context.subscriptions.push(diagnostics);

  context.subscriptions.push(
    vscode.commands.registerCommand("cfc.checkFile", () => {
      const editor = vscode.window.activeTextEditor;
      if (editor) {
        checkDocument(editor.document);
      }
    }),
    vscode.workspace.onDidSaveTextDocument((document) => {
      if (config().get("checkOnSave", true)) {
        checkDocument(document);
      }
    }),
    vscode.workspace.onDidOpenTextDocument((document) => {
      if (config().get("checkOnOpen", true)) {
        checkDocument(document);
      }
    }),
    vscode.workspace.onDidChangeConfiguration((event) => {
      if (event.affectsConfiguration("cfc")) {
        for (const document of vscode.workspace.textDocuments) {
          checkDocument(document);
        }
      }
    })
  );

  for (const document of vscode.workspace.textDocuments) {
    if (config().get("checkOnOpen", true)) {
      checkDocument(document);
    }
  }
}

function deactivate() {
  if (diagnostics) {
    diagnostics.dispose();
  }
}

function config() {
  return vscode.workspace.getConfiguration("cfc");
}

function checkDocument(document) {
  if (document.languageId !== "cfc" || document.uri.scheme !== "file") {
    return;
  }

  const cliPath = resolveCliPath(document);
  const workspaceFolder = vscode.workspace.getWorkspaceFolder(document.uri);
  const cwd = workspaceFolder ? workspaceFolder.uri.fsPath : path.dirname(document.uri.fsPath);
  const child = cp.spawn(cliPath, ["check", "--json", document.uri.fsPath], {
    cwd,
    windowsHide: true
  });

  let stdout = "";
  let stderr = "";
  child.stdout.on("data", (chunk) => {
    stdout += chunk.toString();
  });
  child.stderr.on("data", (chunk) => {
    stderr += chunk.toString();
  });
  child.on("error", (error) => {
    diagnostics.set(document.uri, [
      new vscode.Diagnostic(
        new vscode.Range(0, 0, 0, 1),
        `failed to run cfc: ${error.message}`,
        vscode.DiagnosticSeverity.Error
      )
    ]);
  });
  child.on("close", () => {
    const parsed = parseDiagnostics(stdout, stderr);
    setDiagnostics(document, parsed);
  });
}

function setDiagnostics(document, items) {
  const byFile = new Map();
  for (const item of items) {
    const uri = item.file ? vscode.Uri.file(normalizeDiagnosticPath(item.file)) : document.uri;
    const key = uri.toString();
    const entry = byFile.get(key) || { uri, items: [] };
    entry.items.push(item);
    byFile.set(key, entry);
  }

  diagnostics.delete(document.uri);
  for (const entry of byFile.values()) {
    diagnostics.set(
      entry.uri,
      entry.items.map((item) => toDiagnostic(entry.uri, item))
    );
  }
}

function resolveCliPath(document) {
  const configured = config().get("cliPath", "cfc");
  const workspaceFolder = vscode.workspace.getWorkspaceFolder(document.uri);
  if (workspaceFolder && configured.includes("${workspaceFolder}")) {
    return configured.replaceAll("${workspaceFolder}", workspaceFolder.uri.fsPath);
  }

  if (configured !== "cfc") {
    return configured;
  }

  const devCli = path.resolve(extensionPath, "..", "..", "target", "debug", "cfc.exe");
  return fs.existsSync(devCli) ? devCli : configured;
}

function parseDiagnostics(stdout, stderr) {
  const text = stdout.trim();
  if (!text) {
    return stderr.trim()
      ? [{ message: stderr.trim(), span: null, kind: "Cli" }]
      : [];
  }

  try {
    const parsed = JSON.parse(text);
    return Array.isArray(parsed) ? parsed : [];
  } catch (error) {
    return [{ message: text, span: null, kind: "Cli" }];
  }
}

function toDiagnostic(uri, item) {
  const range = spanToRange(uri, item.span);
  const diagnostic = new vscode.Diagnostic(
    range,
    item.message || "CFC diagnostic",
    vscode.DiagnosticSeverity.Error
  );
  diagnostic.source = "cfc";
  diagnostic.code = item.kind;
  return diagnostic;
}

function spanToRange(uri, span) {
  const document = vscode.workspace.textDocuments.find((candidate) => candidate.uri.toString() === uri.toString());
  if (!document) {
    return new vscode.Range(0, 0, 0, 1);
  }

  if (!span || typeof span.start !== "number" || typeof span.end !== "number") {
    return new vscode.Range(0, 0, 0, 1);
  }

  const text = document.getText();
  const start = byteOffsetToUtf16Offset(text, span.start);
  const end = Math.max(start + 1, byteOffsetToUtf16Offset(text, span.end));
  return new vscode.Range(document.positionAt(start), document.positionAt(end));
}

function normalizeDiagnosticPath(file) {
  return file.startsWith("//?/") ? file.slice(4) : file;
}

function byteOffsetToUtf16Offset(text, byteOffset) {
  let bytes = 0;
  let offset = 0;
  while (offset < text.length) {
    if (bytes >= byteOffset) {
      return offset;
    }
    const codePoint = text.codePointAt(offset);
    const char = String.fromCodePoint(codePoint);
    bytes += Buffer.byteLength(char, "utf8");
    offset += char.length;
  }
  return text.length;
}

module.exports = {
  activate,
  deactivate
};
