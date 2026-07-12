const vscode = require("vscode");
const cp = require("child_process");
const fs = require("fs");
const path = require("path");

const CFT_SEMANTIC_TOKENS_LEGEND = new vscode.SemanticTokensLegend(
  [
    "namespace",
    "type",
    "enum",
    "enumMember",
    "property",
    "variable",
    "function",
    "keyword",
    "number",
    "string",
    "comment",
    "operator",
    "decorator",
    "parameter"
  ],
  ["declaration", "reference", "path", "record", "schema"]
);

function activate(context) {
  const selector = [{ language: "cft" }, { language: "cfd" }];
  const diagnostics = new CftDiagnostics(context);
  context.subscriptions.push(
    vscode.languages.registerCompletionItemProvider(
      selector,
      new CftCompletionProvider(diagnostics),
      ".",
      "@",
      ":",
      " ",
      "("
    ),
    vscode.languages.registerHoverProvider(selector, new CftHoverProvider(diagnostics)),
    vscode.languages.registerDocumentSymbolProvider(selector, new CftDocumentSymbolProvider(diagnostics)),
    vscode.languages.registerDefinitionProvider(selector, new CftDefinitionProvider(diagnostics)),
    vscode.languages.registerDocumentFormattingEditProvider(selector, new CftFormattingProvider(diagnostics)),
    vscode.languages.registerDocumentSemanticTokensProvider(
      selector,
      new CftSemanticTokensProvider(diagnostics),
      CFT_SEMANTIC_TOKENS_LEGEND
    ),
    diagnostics
  );
}

function deactivate() {}

class CftCompletionProvider {
  constructor(diagnostics) {
    this.diagnostics = diagnostics;
  }

  async provideCompletionItems(document, position) {
    const lspItems = await this.diagnostics.request(
      document,
      "textDocument/completion",
      textPositionParams(document, position)
    );
    if (Array.isArray(lspItems)) {
      return lspItems.map(lspCompletionItemToVsCode);
    }
    return [];
  }
}

class CftHoverProvider {
  constructor(diagnostics) {
    this.diagnostics = diagnostics;
  }

  async provideHover(document, position) {
    const hover = await this.diagnostics.request(
      document,
      "textDocument/hover",
      textPositionParams(document, position)
    );
    if (hover) {
      return lspHoverToVsCode(hover);
    }
    return undefined;
  }
}

class CftDocumentSymbolProvider {
  constructor(diagnostics) {
    this.diagnostics = diagnostics;
  }

  async provideDocumentSymbols(document) {
    const symbols = await this.diagnostics.request(document, "textDocument/documentSymbol", {
      textDocument: {
        uri: document.uri.toString()
      }
    });
    if (Array.isArray(symbols)) {
      return symbols.map(lspDocumentSymbolToVsCode);
    }
    return [];
  }
}

class CftDefinitionProvider {
  constructor(diagnostics) {
    this.diagnostics = diagnostics;
  }

  async provideDefinition(document, position) {
    const definitions = await this.diagnostics.request(
      document,
      "textDocument/definition",
      textPositionParams(document, position)
    );
    const lspLocations = lspDefinitionLocations(definitions);
    if (lspLocations) {
      return lspLocations;
    }

    return undefined;
  }
}

class CftDiagnostics {
  constructor(context) {
    this.context = context;
    this.collection = vscode.languages.createDiagnosticCollection("cft");
    this.timers = new Map();
    this.sessions = new Map();
    this.documentSessions = new Map();

    const configWatcher = vscode.workspace.createFileSystemWatcher("**/coflow.{yaml,yml}");
    configWatcher.onDidChange((uri) => this.notifySessionsForFile(uri, 2));
    configWatcher.onDidCreate((uri) => this.restartSessionsForProject(uri));
    configWatcher.onDidDelete((uri) => this.restartSessionsForProject(uri));
    const sourceWatcher = vscode.workspace.createFileSystemWatcher("**/*.{cft,cfd}");
    sourceWatcher.onDidChange((uri) => this.notifySessionsForFile(uri, 2));
    sourceWatcher.onDidCreate((uri) => this.notifySessionsForFile(uri, 1));
    sourceWatcher.onDidDelete((uri) => this.notifySessionsForFile(uri, 3));

    context.subscriptions.push(
      this.collection,
      configWatcher,
      sourceWatcher,
      vscode.workspace.onDidOpenTextDocument((document) => this.openDocument(document)),
      vscode.workspace.onDidChangeTextDocument((event) => this.schedule(event.document)),
      vscode.workspace.onDidSaveTextDocument((document) => this.saveDocument(document)),
      vscode.workspace.onDidCloseTextDocument((document) => this.closeDocument(document)),
      vscode.workspace.onDidChangeConfiguration((event) => {
        if (event.affectsConfiguration("coflow.diagnostics")) {
          this.restartAllSessions();
        }
      })
    );

    this.validateAllOpenDocuments();
  }

  dispose() {
    for (const timer of this.timers.values()) {
      clearTimeout(timer);
    }
    this.timers.clear();
    for (const session of this.sessions.values()) {
      session.dispose();
    }
    this.sessions.clear();
    this.documentSessions.clear();
    this.collection.dispose();
  }

  validateAllOpenDocuments() {
    for (const document of vscode.workspace.textDocuments) {
      this.openDocument(document);
    }
  }

  restartSessionsForProject(configUri) {
    const projectDir = path.dirname(configUri.fsPath);
    // Find all session keys whose args include the affected project directory.
    const affectedKeys = [...this.sessions.keys()].filter((key) => {
      try {
        const parsed = JSON.parse(key);
        return parsed.args && parsed.args.some(
          (arg) => normalizePath(arg) === normalizePath(projectDir)
        );
      } catch {
        return false;
      }
    });
    if (affectedKeys.length === 0) {
      return;
    }
    for (const key of affectedKeys) {
      const session = this.sessions.get(key);
      if (session) {
        session.dispose();
      }
      this.sessions.delete(key);
    }
    // Remove document→session mappings for the affected sessions.
    for (const [docUri, sessionKey] of this.documentSessions.entries()) {
      if (affectedKeys.includes(sessionKey)) {
        this.documentSessions.delete(docUri);
      }
    }
    this.validateAllOpenDocuments();
  }

  restartAllSessions() {
    for (const timer of this.timers.values()) {
      clearTimeout(timer);
    }
    this.timers.clear();
    for (const session of this.sessions.values()) {
      session.dispose();
    }
    this.sessions.clear();
    this.documentSessions.clear();
    this.collection.clear();
    this.validateAllOpenDocuments();
  }

  notifySessionsForFile(uri, changeType) {
    for (const session of this.sessions.values()) {
      if (isPathWithin(session.projectDir, uri.fsPath)) {
        session.notifyWatchedFile(uri, changeType);
      }
    }
  }

  openDocument(document) {
    if (!this.canValidate(document)) {
      return;
    }
    const session = this.ensureSession(document);
    if (!session) {
      return;
    }
    session.openOrChangeDocument(document);
  }

  schedule(document) {
    if (!this.canValidate(document)) {
      return;
    }

    const config = vscode.workspace.getConfiguration("coflow.diagnostics", document.uri);
    const key = document.uri.toString();
    const oldTimer = this.timers.get(key);
    if (oldTimer) {
      clearTimeout(oldTimer);
    }

    const debounceMs = config.get("debounceMs", 350);
    this.timers.set(
      key,
      setTimeout(() => {
        this.timers.delete(key);
        const session = this.ensureSession(document);
        if (session) {
          session.openOrChangeDocument(document);
        }
      }, debounceMs)
    );
  }

  saveDocument(document) {
    if (!this.canValidate(document)) {
      return;
    }

    const key = document.uri.toString();
    const oldTimer = this.timers.get(key);
    if (oldTimer) {
      clearTimeout(oldTimer);
      this.timers.delete(key);
    }

    const session = this.ensureSession(document);
    if (session) {
      session.openOrChangeDocument(document);
      session.saveDocument(document);
    }
  }

  closeDocument(document) {
    if ((document.languageId !== "cft" && document.languageId !== "cfd") || document.uri.scheme !== "file") {
      return;
    }

    const uriString = document.uri.toString();
    const oldTimer = this.timers.get(uriString);
    if (oldTimer) {
      clearTimeout(oldTimer);
      this.timers.delete(uriString);
    }

    const sessionKey = this.documentSessions.get(uriString);
    const session = sessionKey ? this.sessions.get(sessionKey) : undefined;
    if (session) {
      session.closeDocument(document);
    }
    this.documentSessions.delete(uriString);
    this.collection.delete(document.uri);
  }

  canValidate(document) {
    if ((document.languageId !== "cft" && document.languageId !== "cfd") || document.uri.scheme !== "file") {
      return false;
    }

    const config = vscode.workspace.getConfiguration("coflow.diagnostics", document.uri);
    if (!config.get("enabled", true)) {
      const sessionKey = this.documentSessions.get(document.uri.toString());
      const session = sessionKey ? this.sessions.get(sessionKey) : undefined;
      if (session) {
        session.closeDocument(document);
      }
      this.documentSessions.delete(document.uri.toString());
      this.collection.delete(document.uri);
      return false;
    }

    return true;
  }

  ensureSession(document) {
    const config = vscode.workspace.getConfiguration("coflow.diagnostics", document.uri);
    let command = config.get("command", "coflow");
    let baseArgs = config.get("args", ["lsp"]);
    if (shouldUseDevelopmentCargoServer(config, command, baseArgs, this.context.extensionPath)) {
      command = "cargo";
      baseArgs = ["run", "--quiet", "-p", "coflow", "--", "lsp"];
    }
    const projectDir = findNearestCoflowConfigDir(path.dirname(document.uri.fsPath));
    const cwd = findDiagnosticsCwd(
      document.uri.fsPath,
      this.context.extensionPath,
      command,
      baseArgs,
      projectDir
    );
    const args = appendProjectArg(baseArgs, projectDir);
    const key = JSON.stringify({ command, args, cwd });
    const previousKey = this.documentSessions.get(document.uri.toString());
    if (previousKey && previousKey !== key) {
      const previous = this.sessions.get(previousKey);
      if (previous) {
        previous.closeDocument(document);
      }
      this.documentSessions.delete(document.uri.toString());
    }

    let session = this.sessions.get(key);
    if (!session) {
      session = new CftLspSession(
        command,
        args,
        cwd,
        this.collection,
        (uri) => this.diagnosticsEnabledForUri(uri),
        projectDir || cwd
      );
      this.sessions.set(key, session);
    }

    this.documentSessions.set(document.uri.toString(), key);
    return session;
  }

  async request(document, method, params) {
    if ((document.languageId !== "cft" && document.languageId !== "cfd") || document.uri.scheme !== "file") {
      return undefined;
    }
    const session = this.ensureSession(document);
    if (!session) {
      return undefined;
    }
    session.openOrChangeDocument(document);
    return session.request(method, params);
  }

  diagnosticsEnabledForUri(uri) {
    return vscode.workspace
      .getConfiguration("coflow.diagnostics", uri)
      .get("enabled", true);
  }
}

class CftFormattingProvider {
  constructor(diagnostics) {
    this.diagnostics = diagnostics;
  }

  async provideDocumentFormattingEdits(document) {
    const edits = await this.diagnostics.request(document, "textDocument/formatting", {
      textDocument: {
        uri: document.uri.toString()
      },
      options: {
        tabSize: 2,
        insertSpaces: true
      }
    });
    return Array.isArray(edits) ? edits.map(lspTextEditToVsCode) : [];
  }
}

class CftSemanticTokensProvider {
  constructor(diagnostics) {
    this.diagnostics = diagnostics;
  }

  async provideDocumentSemanticTokens(document) {
    const result = await this.diagnostics.request(document, "textDocument/semanticTokens/full", {
      textDocument: {
        uri: document.uri.toString()
      }
    });
    const builder = new vscode.SemanticTokensBuilder(CFT_SEMANTIC_TOKENS_LEGEND);
    if (!result || !Array.isArray(result.data)) {
      return builder.build();
    }

    let line = 0;
    let character = 0;
    const data = result.data;
    for (let index = 0; index + 4 < data.length; index += 5) {
      const deltaLine = data[index];
      const deltaStart = data[index + 1];
      line += deltaLine;
      character = deltaLine === 0 ? character + deltaStart : deltaStart;
      builder.push(line, character, data[index + 2], data[index + 3], data[index + 4]);
    }
    return builder.build();
  }
}

class CftLspSession {
  constructor(
    command,
    args,
    cwd,
    collection,
    diagnosticsEnabledForUri = () => true,
    projectDir = cwd
  ) {
    this.command = command;
    this.args = args;
    this.cwd = cwd;
    this.collection = collection;
    this.diagnosticsEnabledForUri = diagnosticsEnabledForUri;
    this.projectDir = projectDir;
    this.nextId = 1;
    this.buffer = Buffer.alloc(0);
    this.openedUris = new Set();
    this.openedFileUris = new Map();
    this.pending = new Map();
    this.failed = false;
    this.disposed = false;

    this.child = cp.spawn(command, args, {
      cwd,
      shell: process.platform === "win32"
    });

    this.child.stdout.on("data", (chunk) => this.handleStdout(chunk));
    this.child.stderr.setEncoding("utf8");
    this.child.stderr.on("data", (chunk) => {
      this.lastStderr = `${this.lastStderr || ""}${chunk}`;
    });
    this.child.stdin.on("error", (error) => this.markFailed(error.message));
    this.child.on("error", (error) => this.markFailed(error.message));
    this.child.on("close", (code) => {
      if (!this.disposed && code !== 0) {
        const message = (this.lastStderr || `${command} exited with code ${code}`).trim();
        this.markFailed(message);
      }
    });

    this.sendRequest("initialize", {
      processId: null,
      rootUri: vscode.Uri.file(cwd).toString(),
      capabilities: {},
      workspaceFolders: null
    });
    this.sendNotification("initialized", {});
  }

  openOrChangeDocument(document) {
    if (this.failed || this.disposed) {
      this.publishFailure(document.uri);
      return;
    }

    this.rememberDocumentUri(document);
    const uri = document.uri.toString();
    if (this.openedUris.has(uri)) {
      this.sendNotification("textDocument/didChange", {
        textDocument: {
          uri,
          version: document.version
        },
        contentChanges: [
          {
            text: document.getText()
          }
        ]
      });
    } else {
      this.openedUris.add(uri);
      this.sendNotification("textDocument/didOpen", {
        textDocument: {
          uri,
          languageId: document.languageId,
          version: document.version,
          text: document.getText()
        }
      });
    }
  }

  saveDocument(document) {
    if (this.failed || this.disposed) {
      this.publishFailure(document.uri);
      return;
    }

    this.sendNotification("textDocument/didSave", {
      textDocument: {
        uri: document.uri.toString()
      }
    });
  }

  closeDocument(document) {
    const uri = document.uri.toString();
    if (!this.openedUris.delete(uri) || this.failed || this.disposed) {
      return;
    }

    this.forgetDocumentUri(document);
    this.sendNotification("textDocument/didClose", {
      textDocument: {
        uri
      }
    });
  }

  notifyWatchedFile(uri, changeType) {
    if (this.failed || this.disposed || this.openedUris.has(uri.toString())) {
      return;
    }
    this.sendNotification("workspace/didChangeWatchedFiles", {
      changes: [{ uri: uri.toString(), type: changeType }]
    });
  }

  dispose() {
    this.disposed = true;
    try {
      this.sendRequest("shutdown", null);
      this.sendNotification("exit", {});
    } catch {
      // The process may already be gone.
    }
    if (this.child && !this.child.killed) {
      this.child.kill();
    }
  }

  handleStdout(chunk) {
    this.buffer = Buffer.concat([this.buffer, Buffer.from(chunk)]);

    while (true) {
      const headerEnd = this.buffer.indexOf("\r\n\r\n");
      if (headerEnd < 0) {
        return;
      }

      const header = this.buffer.slice(0, headerEnd).toString("utf8");
      const match = header.match(/(?:^|\r\n)Content-Length:\s*(\d+)/i);
      if (!match) {
        this.markFailed("language server sent an invalid LSP header");
        return;
      }

      const length = Number(match[1]);
      const bodyStart = headerEnd + 4;
      const bodyEnd = bodyStart + length;
      if (this.buffer.length < bodyEnd) {
        return;
      }

      const body = this.buffer.slice(bodyStart, bodyEnd).toString("utf8");
      this.buffer = this.buffer.slice(bodyEnd);

      try {
        this.handleMessage(JSON.parse(body));
      } catch (error) {
        this.markFailed(`failed to parse language server message: ${error.message || error}`);
        return;
      }
    }
  }

  handleMessage(message) {
    if (message.method === "textDocument/publishDiagnostics") {
      const params = message.params || {};
      const uri = this.uriFromLsp(params.uri);
      if (!uri) {
        return;
      }
      if (!this.diagnosticsEnabledForUri(uri)) {
        return;
      }
      const diagnostics = Array.isArray(params.diagnostics)
        ? params.diagnostics.map((diagnostic) => lspDiagnosticToVsCode(
          diagnostic,
          (rawUri) => this.uriFromLsp(rawUri)
        ))
        : [];
      this.collection.set(uri, diagnostics);
    } else if (Object.prototype.hasOwnProperty.call(message, "id")) {
      const pending = this.pending.get(message.id);
      if (!pending) {
        return;
      }
      this.pending.delete(message.id);
      if (message.error) {
        pending.reject(new Error(message.error.message || "language server request failed"));
      } else {
        pending.resolve(message.result);
      }
    }
  }

  sendRequest(method, params) {
    const id = this.nextId++;
    this.send({
      jsonrpc: "2.0",
      id,
      method,
      params
    });
    return id;
  }

  request(method, params) {
    if (this.failed || this.disposed) {
      return Promise.resolve(undefined);
    }
    return new Promise((resolve) => {
      const id = this.nextId++;
      const timer = setTimeout(() => {
        this.pending.delete(id);
        resolve(undefined);
      }, 1500);
      this.pending.set(id, {
        resolve: (value) => {
          clearTimeout(timer);
          resolve(value);
        },
        reject: () => {
          clearTimeout(timer);
          resolve(undefined);
        }
      });
      this.send({
        jsonrpc: "2.0",
        id,
        method,
        params
      });
    });
  }

  sendNotification(method, params) {
    this.send({
      jsonrpc: "2.0",
      method,
      params
    });
  }

  send(message) {
    const body = Buffer.from(JSON.stringify(message), "utf8");
    const header = Buffer.from(`Content-Length: ${body.length}\r\n\r\n`, "utf8");
    try {
      this.child.stdin.write(Buffer.concat([header, body]));
    } catch (error) {
      this.markFailed(error.message || error);
    }
  }

  markFailed(message) {
    this.failed = true;
    this.failureMessage = message || "language server failed";
    for (const pending of this.pending.values()) {
      pending.reject(new Error(this.failureMessage));
    }
    this.pending.clear();
    for (const uriString of this.openedUris) {
      this.publishFailure(vscode.Uri.parse(uriString));
    }
  }

  publishFailure(uri) {
    if (!this.diagnosticsEnabledForUri(uri)) {
      return;
    }
    const diagnostic = new vscode.Diagnostic(
      new vscode.Range(0, 0, 0, 0),
      `CFT language server failed: ${formatFailureMessage(this.failureMessage)}`,
      vscode.DiagnosticSeverity.Error
    );
    diagnostic.source = "cft";
    this.collection.set(uri, [diagnostic]);
  }

  rememberDocumentUri(document) {
    if (document.uri.scheme === "file") {
      this.openedFileUris.set(normalizeFsPathKey(document.uri.fsPath), document.uri);
    }
  }

  forgetDocumentUri(document) {
    if (document.uri.scheme === "file") {
      this.openedFileUris.delete(normalizeFsPathKey(document.uri.fsPath));
    }
  }

  uriFromLsp(rawUri) {
    const uri = lspUriToVsCode(rawUri);
    if (!uri || uri.scheme !== "file") {
      return uri;
    }

    return this.openedFileUris.get(normalizeFsPathKey(uri.fsPath)) || uri;
  }
}

async function collectConfiguredSchemaPaths(projectDir) {
  const configPath = coflowConfigPath(projectDir);
  if (!configPath) {
    return [];
  }

  let entries;
  try {
    const text = await fs.promises.readFile(configPath, "utf8");
    entries = schemaEntriesFromCoflowConfigText(text);
  } catch {
    return [];
  }

  const paths = [];
  for (const entry of entries) {
    const resolved = normalizePath(path.resolve(projectDir, entry));
    try {
      const stat = await fs.promises.stat(resolved);
      if (stat.isDirectory()) {
        paths.push(...await collectCftFilesInDir(resolved));
      } else if (stat.isFile() && resolved.endsWith(".cft")) {
        paths.push(resolved);
      }
    } catch {
      // Diagnostics cover missing schema files; local language features skip them.
    }
  }
  return [...new Set(paths)].sort((left, right) => left.localeCompare(right));
}

function coflowConfigPath(projectDir) {
  const yaml = path.join(projectDir, "coflow.yaml");
  if (fs.existsSync(yaml)) {
    return yaml;
  }
  const yml = path.join(projectDir, "coflow.yml");
  return fs.existsSync(yml) ? yml : undefined;
}

function schemaEntriesFromCoflowConfigText(text) {
  const lines = text.replace(/\r\n/g, "\n").split("\n");
  const entries = [];

  for (let index = 0; index < lines.length; index += 1) {
    const line = stripYamlComment(lines[index]);
    const match = line.match(/^(\s*)schema\s*:\s*(.*?)\s*$/);
    if (!match) {
      continue;
    }

    const indent = match[1].length;
    const inline = unquoteYamlScalar(match[2].trim());
    if (inline) {
      return [inline];
    }

    for (index += 1; index < lines.length; index += 1) {
      const child = stripYamlComment(lines[index]);
      if (!child.trim()) {
        continue;
      }
      const childIndent = child.match(/^\s*/)[0].length;
      if (childIndent <= indent) {
        index -= 1;
        break;
      }
      const item = child.trim().match(/^-\s*(.*?)\s*$/);
      if (item) {
        const value = unquoteYamlScalar(item[1].trim());
        if (value) {
          entries.push(value);
        }
      }
    }
    return entries;
  }

  return entries;
}

function stripYamlComment(line) {
  let inSingle = false;
  let inDouble = false;
  for (let index = 0; index < line.length; index += 1) {
    const ch = line[index];
    if (ch === "'" && !inDouble) {
      inSingle = !inSingle;
    } else if (ch === "\"" && !inSingle) {
      inDouble = !inDouble;
    } else if (ch === "#" && !inSingle && !inDouble) {
      return line.slice(0, index);
    }
  }
  return line;
}

function unquoteYamlScalar(value) {
  if (
    (value.startsWith("\"") && value.endsWith("\"")) ||
    (value.startsWith("'") && value.endsWith("'"))
  ) {
    return value.slice(1, -1);
  }
  return value;
}

async function collectCftFilesInDir(dir) {
  const output = [];
  let entries;
  try {
    entries = await fs.promises.readdir(dir, { withFileTypes: true });
  } catch {
    return output;
  }
  for (const entry of entries) {
    const fullPath = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      if (![".git", "node_modules", "target"].includes(entry.name)) {
        output.push(...await collectCftFilesInDir(fullPath));
      }
    } else if (entry.isFile() && entry.name.endsWith(".cft")) {
      output.push(normalizePath(fullPath));
    }
  }
  return output;
}

function findDiagnosticsCwd(documentPath, extensionPath, command, args, projectDir) {
  const workspace = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
  const candidates = [
    workspace,
    path.resolve(extensionPath, "..", ".."),
    path.dirname(documentPath)
  ].filter(Boolean);

  if (isCargoCommand(command, args)) {
    for (const candidate of candidates) {
      if (fs.existsSync(path.join(candidate, "Cargo.toml"))) {
        return candidate;
      }
    }
  }

  if (projectDir) {
    return projectDir;
  }

  return workspace || path.dirname(documentPath);
}

function isCargoCommand(command, args) {
  const executable = path.basename(command).toLowerCase();
  return executable === "cargo" || executable === "cargo.exe" || args.includes("-p");
}

function shouldUseDevelopmentCargoServer(config, command, args, extensionPath) {
  if (command !== "coflow" || JSON.stringify(args) !== JSON.stringify(["lsp"])) {
    return false;
  }
  const packageCommand = config.inspect("command");
  const packageArgs = config.inspect("args");
  if (packageCommand?.workspaceValue !== undefined || packageCommand?.globalValue !== undefined) {
    return false;
  }
  if (packageArgs?.workspaceValue !== undefined || packageArgs?.globalValue !== undefined) {
    return false;
  }
  const repoRoot = path.resolve(extensionPath, "..", "..");
  return fs.existsSync(path.join(repoRoot, "Cargo.toml")) &&
    fs.existsSync(path.join(repoRoot, "src", "main.rs"));
}

function appendProjectArg(args, projectDir) {
  if (!projectDir) {
    return args;
  }
  const lspIndex = args.lastIndexOf("lsp");
  if (lspIndex >= 0 && args.slice(lspIndex + 1).some((arg) => !arg.startsWith("-"))) {
    return args;
  }
  return [...args, projectDir];
}

function findNearestCoflowConfigDir(startDir) {
  let current = path.resolve(startDir);
  while (true) {
    if (
      fs.existsSync(path.join(current, "coflow.yaml")) ||
      fs.existsSync(path.join(current, "coflow.yml"))
    ) {
      return current;
    }
    const parent = path.dirname(current);
    if (parent === current) {
      return undefined;
    }
    current = parent;
  }
}

function lspDiagnosticToVsCode(raw, uriFromLsp = lspUriToVsCode) {
  const diagnostic = new vscode.Diagnostic(
    lspRangeToVsCode(raw?.range, true),
    raw?.message || "",
    lspSeverityToVsCode(raw?.severity)
  );
  diagnostic.code = raw?.code;
  diagnostic.source = raw?.source || "cft";
  if (Array.isArray(raw?.relatedInformation)) {
    diagnostic.relatedInformation = raw.relatedInformation
      .map((related) => {
        const location = lspLocationToVsCode(related?.location, uriFromLsp, true);
        return location
          ? new vscode.DiagnosticRelatedInformation(
            location,
            related?.message || "related location"
          )
          : undefined;
      })
      .filter(Boolean);
  }
  return diagnostic;
}

function lspSeverityToVsCode(severity) {
  switch (severity) {
    case 1:
      return vscode.DiagnosticSeverity.Error;
    case 2:
      return vscode.DiagnosticSeverity.Warning;
    case 3:
      return vscode.DiagnosticSeverity.Information;
    case 4:
      return vscode.DiagnosticSeverity.Hint;
    default:
      return vscode.DiagnosticSeverity.Error;
  }
}

function textPositionParams(document, position) {
  return {
    textDocument: {
      uri: document.uri.toString()
    },
    position: {
      line: position.line,
      character: position.character
    }
  };
}

function lspCompletionItemToVsCode(raw) {
  const item = new vscode.CompletionItem(raw.label || "", lspCompletionKindToVsCode(raw.kind));
  item.detail = raw.detail;
  if (raw.documentation) {
    item.documentation = typeof raw.documentation === "string"
      ? markdown(raw.documentation)
      : raw.documentation;
  }
  if (raw.insertText) {
    item.insertText = raw.insertTextFormat === 2
      ? new vscode.SnippetString(raw.insertText)
      : raw.insertText;
  }
  item.sortText = raw.sortText;
  if (raw.range) {
    item.range = lspRangeToVsCode(raw.range);
  }
  return item;
}

function lspCompletionKindToVsCode(kind) {
  switch (kind) {
    case 3:
      return vscode.CompletionItemKind.Function;
    case 5:
      return vscode.CompletionItemKind.Field;
    case 6:
      return vscode.CompletionItemKind.Variable;
    case 7:
      return vscode.CompletionItemKind.Class;
    case 10:
      return vscode.CompletionItemKind.Property;
    case 13:
      return vscode.CompletionItemKind.Enum;
    case 14:
      return vscode.CompletionItemKind.Keyword;
    case 20:
      return vscode.CompletionItemKind.EnumMember;
    case 21:
      return vscode.CompletionItemKind.Constant;
    default:
      return vscode.CompletionItemKind.Text;
  }
}

function lspHoverToVsCode(raw) {
  const contents = raw.contents;
  const value = typeof contents === "string"
    ? contents
    : contents?.value || "";
  const range = raw.range ? lspRangeToVsCode(raw.range) : undefined;
  return new vscode.Hover(markdown(value), range);
}

function lspDocumentSymbolToVsCode(raw) {
  const symbol = new vscode.DocumentSymbol(
    raw.name || "",
    raw.detail || "",
    lspSymbolKindToVsCode(raw.kind),
    lspRangeToVsCode(raw.range),
    lspRangeToVsCode(raw.selectionRange || raw.range)
  );
  if (Array.isArray(raw.children)) {
    symbol.children.push(...raw.children.map(lspDocumentSymbolToVsCode));
  }
  return symbol;
}

function lspDefinitionLocations(definitions) {
  const rawDefinitions = Array.isArray(definitions)
    ? definitions
    : definitions?.uri
      ? [definitions]
      : undefined;
  if (!rawDefinitions) {
    return undefined;
  }

  const locations = rawDefinitions.map((definition) => lspLocationToVsCode(definition)).filter(Boolean);
  return locations.length > 0 ? locations : undefined;
}

function lspSymbolKindToVsCode(kind) {
  switch (kind) {
    case 5:
      return vscode.SymbolKind.Class;
    case 8:
      return vscode.SymbolKind.Field;
    case 10:
      return vscode.SymbolKind.Enum;
    case 14:
      return vscode.SymbolKind.Constant;
    case 22:
      return vscode.SymbolKind.EnumMember;
    default:
      return vscode.SymbolKind.Variable;
  }
}

function lspLocationToVsCode(raw, uriFromLsp = lspUriToVsCode, ensureNonEmpty = false) {
  const uri = uriFromLsp(raw?.uri);
  if (!uri) {
    return undefined;
  }
  return new vscode.Location(uri, lspRangeToVsCode(raw.range, ensureNonEmpty));
}

function lspTextEditToVsCode(raw) {
  return new vscode.TextEdit(lspRangeToVsCode(raw.range), raw.newText || "");
}

function lspRangeToVsCode(raw, ensureNonEmpty = false) {
  const start = raw?.start || {};
  const end = raw?.end || start;
  const startLine = lspPositionNumber(start.line, 0);
  const startCharacter = lspPositionNumber(start.character, 0);
  let endLine = lspPositionNumber(end.line, startLine);
  let endCharacter = lspPositionNumber(end.character, startCharacter);

  if (endLine < startLine || (endLine === startLine && endCharacter < startCharacter)) {
    endLine = startLine;
    endCharacter = startCharacter;
  }

  if (ensureNonEmpty && endLine === startLine && endCharacter === startCharacter) {
    endCharacter += 1;
  }

  return new vscode.Range(
    startLine,
    startCharacter,
    endLine,
    endCharacter
  );
}

function lspPositionNumber(value, fallback) {
  return Number.isInteger(value) && value >= 0 ? value : fallback;
}

function lspUriToVsCode(rawUri) {
  if (typeof rawUri !== "string" || rawUri.length === 0) {
    return undefined;
  }

  try {
    const uri = vscode.Uri.parse(rawUri);
    return uri.scheme === "file" ? vscode.Uri.file(uri.fsPath) : uri;
  } catch {
    return undefined;
  }
}

function normalizeFsPathKey(fsPath) {
  const normalized = path.normalize(fsPath);
  return process.platform === "win32" ? normalized.toLowerCase() : normalized;
}

function isPathWithin(root, filePath) {
  if (!root) {
    return false;
  }
  const relative = path.relative(path.resolve(root), path.resolve(filePath));
  return relative === "" || (!relative.startsWith("..") && !path.isAbsolute(relative));
}

function formatFailureMessage(message) {
  return String(message || "language server failed").trim() || "language server failed";
}

function normalizePath(filePath) {
  return path.resolve(filePath);
}

module.exports = {
  activate,
  deactivate,
  __test: {
    CftCompletionProvider,
    CftDefinitionProvider,
    CftDocumentSymbolProvider,
    CftHoverProvider,
    CftLspSession,
    collectConfiguredSchemaPaths,
    semanticTokensLegend: CFT_SEMANTIC_TOKENS_LEGEND,
    schemaEntriesFromCoflowConfigText,
    lspDefinitionLocations,
    isPathWithin,
    vscodePosition: vscode.Position,
    vscodeRange: vscode.Range,
    vscodeUriFile: vscode.Uri.file,
    vscodeWorkspace: vscode.workspace
  }
};
