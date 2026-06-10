const vscode = require("vscode");
const cp = require("child_process");
const fs = require("fs");
const path = require("path");

const IDENT_START = "[_\\p{ID_Start}]";
const IDENT_CONTINUE = "[_\\p{ID_Continue}]";
const IDENT = `${IDENT_START}${IDENT_CONTINUE}*`;
const IDENT_BOUNDARY_BEFORE = `(?<!${IDENT_CONTINUE})`;
const IDENT_BOUNDARY_AFTER = `(?!${IDENT_CONTINUE})`;
const IDENT_WORD_RE = new RegExp(IDENT, "u");
const ANNOTATION_WORD_RE = new RegExp(`@?${IDENT}`, "u");
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
  []
);

const KEYWORDS = [
  ["const", "Define a compile-time constant."],
  ["enum", "Define an enum."],
  ["type", "Define a schema type."],
  ["abstract", "Mark a type as non-instantiable."],
  ["sealed", "Prevent a type from being inherited."],
  ["check", "Start a validation block inside a type."],
  ["when", "Run nested checks only when the condition is true."],
  ["all", "Require every collection item to pass."],
  ["any", "Require at least one collection item to pass."],
  ["none", "Require no collection item to pass."],
  ["in", "Bind a quantifier variable to a collection."],
  ["is", "Check the runtime type or null value."]
];

const PRIMITIVE_TYPES = [
  ["int", "64-bit integer."],
  ["float", "64-bit floating point number."],
  ["bool", "Boolean value."],
  ["string", "String value."]
];

const LITERALS = [
  ["true", "Boolean true."],
  ["false", "Boolean false."],
  ["null", "Nullable value."]
];

const BUILTIN_FUNCTIONS = [
  ["len", "len(col): return the number of items in an array or dict."],
  ["contains", "contains(col, val): test array element or dict key presence."],
  ["unique", "unique(array): true when supported scalar elements are unique."],
  ["min", "min(array): minimum value in a non-empty int, float, or enum array."],
  ["max", "max(array): maximum value in a non-empty int, float, or enum array."],
  ["sum", "sum(array): sum an int or float array."],
  ["keys", "keys(dict): return dict keys as an array."],
  ["values", "values(dict): return dict values as an array."],
  ["matches", "matches(str, pat): regex match with a string literal pattern."]
];

const ANNOTATIONS = [
  {
    label: "@struct",
    insertText: "@struct",
    detail: "type annotation",
    documentation: "Generate a value type. The target must be a sealed type."
  },
  {
    label: "@flag",
    insertText: "@flag",
    detail: "enum annotation",
    documentation: "Mark an enum as bit flags. Non-zero values must be powers of two."
  },
  {
    label: "@id",
    insertText: "@id",
    detail: "field annotation",
    documentation: "Mark a string or int field as the primary key."
  },
  {
    label: "@ref",
    insertText: "@ref(${1:TypeName})",
    detail: "field annotation",
    documentation: "Mark a string or int field as a reference to a type."
  },
  {
    label: "@index",
    insertText: "@index",
    detail: "field annotation",
    documentation: "Generate an index for a string, int, or enum field."
  },
  {
    label: "@display",
    insertText: "@display(\"${1:text}\")",
    detail: "type, enum, or field annotation",
    documentation: "Attach a human-readable display name."
  },
  {
    label: "@deprecated",
    insertText: "@deprecated",
    detail: "type, enum, or field annotation",
    documentation: "Mark the target as deprecated for generated code."
  }
];

function activate(context) {
  const selector = { language: "cft" };
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

    const linePrefix = document.lineAt(position).text.slice(0, position.character);
    if (isTriviaCompletionPosition(document, position)) {
      return [];
    }

    const localSymbols = collectSymbols(document);
    const workspace = await collectWorkspaceSymbols(document);
    const offset = document.offsetAt(position);

    const annotationPrefix = new RegExp(`@${IDENT_CONTINUE}*$`, "u");
    if (annotationPrefix.test(linePrefix)) {
      const range = rangeFromLineMatch(document, position, annotationPrefix);
      return annotationItemsForContext(document, position, range);
    }

    const dot = linePrefix.match(new RegExp(`(${IDENT}(?:\\s*\\.\\s*${IDENT})*)\\.\\s*(${IDENT})?$`, "u"));
    if (dot) {
      const receiverChain = [...dot[1].matchAll(new RegExp(IDENT, "gu"))].map((match) => match[0]);
      const target = receiverChain[0];
      const typed = dot[2] || "";
      const range = new vscode.Range(
        position.line,
        position.character - typed.length,
        position.line,
        position.character
      );
      const variants = receiverChain.length === 1 ? workspaceEnumVariants(workspace, target) : undefined;
      if (variants) {
        return variants.map((variant) =>
          simpleItem(variant.name, vscode.CompletionItemKind.EnumMember, `${target} variant`, range)
        );
      }
      return dotFieldCompletions(workspace, document, position, receiverChain, range);
    }

    if (isTypePredicateContext(linePrefix)) {
      return [
        ...workspaceTypes(workspace).map((type) =>
          simpleItem(type.name, vscode.CompletionItemKind.Class, "CFT type")
        ),
        simpleItem("null", vscode.CompletionItemKind.Keyword, "Null predicate")
      ];
    }

    if (isRefAnnotationContext(linePrefix)) {
      return workspaceTypes(workspace).map((type) =>
        simpleItem(type.name, vscode.CompletionItemKind.Class, "CFT type")
      );
    }

    if (topLevelNeedsTypeKeyword(linePrefix)) {
      return topLevelCompletionItems(linePrefix);
    }

    if (isTypeHeaderParentContext(linePrefix)) {
      return workspaceTypes(workspace).map((type) =>
        simpleItem(type.name, vscode.CompletionItemKind.Class, "CFT type")
      );
    }

    if (isTypeReferenceContext(linePrefix)) {
      return typeReferenceItems(workspace);
    }

    const entry = workspace.documents.get(document.uri.toString());
    const currentType = entry ? currentTypeAt(entry.symbols, offset) : currentTypeAt(localSymbols, offset);
    const scope = completionScopeAt(document, localSymbols, offset);
    if (scope === "topLevel") {
      return topLevelCompletionItems(linePrefix);
    }
    if (scope === "typeBody") {
      if (isFieldDefaultContext(linePrefix)) {
        return fieldDefaultCompletionItems(
          workspace,
          currentFieldFromLinePrefix(linePrefix, currentType)
        );
      }
      return [keywordItem("check")];
    }
    if (scope === "checkBlock") {
      return checkExpressionCompletionItems(workspace, document, position, currentType);
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

    const range =
      document.getWordRangeAtPosition(position, ANNOTATION_WORD_RE) ||
      document.getWordRangeAtPosition(position, IDENT_WORD_RE);
    if (!range) {
      return undefined;
    }

    const text = document.getText(range);
    const staticDoc = staticDocumentation(text);
    if (staticDoc) {
      return new vscode.Hover(markdown(staticDoc), range);
    }

    const symbols = collectSymbols(document);
    const type = symbols.types.find((item) => item.name === text);
    if (type) {
      return new vscode.Hover(markdown(`CFT type with ${type.fields.length} field(s).`), range);
    }

    const enumDef = symbols.enums.find((item) => item.name === text);
    if (enumDef) {
      const count = symbols.enumVariants.get(enumDef.name)?.length || 0;
      return new vscode.Hover(markdown(`CFT enum with ${count} variant(s).`), range);
    }

    if (symbols.consts.some((item) => item.name === text)) {
      return new vscode.Hover(markdown("CFT compile-time constant."), range);
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

    const localSymbols = collectSymbols(document);
    const output = [];

    for (const item of localSymbols.consts) {
      output.push(documentSymbol(document, item, vscode.SymbolKind.Constant));
    }

    for (const item of localSymbols.enums) {
      const symbol = documentSymbol(document, item, vscode.SymbolKind.Enum);
      for (const variant of localSymbols.enumVariants.get(item.name) || []) {
        symbol.children.push(documentSymbol(document, variant, vscode.SymbolKind.EnumMember));
      }
      output.push(symbol);
    }

    for (const item of localSymbols.types) {
      const symbol = documentSymbol(document, item, vscode.SymbolKind.Class);
      for (const field of item.fields) {
        symbol.children.push(documentSymbol(document, field, vscode.SymbolKind.Field));
      }
      output.push(symbol);
    }

    return output.sort((a, b) => a.range.start.compareTo(b.range.start));
  }
}

class CftDefinitionProvider {
  constructor(diagnostics) {
    this.diagnostics = diagnostics;
  }

  async provideDefinition(document, position) {
    const localLocations = await localDefinitionLocations(document, position);
    if (localLocations) {
      return localLocations;
    }

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

async function localDefinitionLocations(document, position) {
    const range = document.getWordRangeAtPosition(position, IDENT_WORD_RE);
    if (!range) {
      return undefined;
    }

    const word = document.getText(range);
    if (isBuiltinName(word)) {
      return undefined;
    }

    const workspace = await collectWorkspaceSymbols(document);
    const chain = dottedChainAt(document, range);

    if (chain.length > 1 && chain[chain.length - 1].name === word) {
      const enumVariant = enumVariantLocations(workspace, chain);
      if (enumVariant.length > 0) {
        return enumVariant;
      }

      const field = fieldChainLocations(workspace, document, position, chain);
      if (field.length > 0) {
        return field;
      }
    }

    const global = globalSymbolLocations(workspace, word);
    if (global.length > 0) {
      return global;
    }

    const localField = directFieldLocations(workspace, document, position, word);
    return localField.length > 0 ? localField : undefined;
}

class CftDiagnostics {
  constructor(context) {
    this.context = context;
    this.collection = vscode.languages.createDiagnosticCollection("cft");
    this.timers = new Map();
    this.sessions = new Map();
    this.documentSessions = new Map();

    context.subscriptions.push(
      this.collection,
      vscode.workspace.onDidOpenTextDocument((document) => this.openDocument(document)),
      vscode.workspace.onDidChangeTextDocument((event) => this.schedule(event.document)),
      vscode.workspace.onDidSaveTextDocument((document) => this.saveDocument(document)),
      vscode.workspace.onDidCloseTextDocument((document) => this.closeDocument(document)),
      vscode.workspace.onDidChangeConfiguration((event) => {
        if (event.affectsConfiguration("coflowCft.diagnostics")) {
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

    const config = vscode.workspace.getConfiguration("coflowCft.diagnostics", document.uri);
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
    if (document.languageId !== "cft" || document.uri.scheme !== "file") {
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
    if (document.languageId !== "cft" || document.uri.scheme !== "file") {
      return false;
    }

    const config = vscode.workspace.getConfiguration("coflowCft.diagnostics", document.uri);
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
    const config = vscode.workspace.getConfiguration("coflowCft.diagnostics", document.uri);
    let command = config.get("command", "coflow");
    let baseArgs = config.get("args", [
      "cft",
      "lsp"
    ]);
    if (shouldUseDevelopmentCargoServer(config, command, baseArgs, this.context.extensionPath)) {
      command = "cargo";
      baseArgs = ["run", "--quiet", "-p", "coflow", "--", "cft", "lsp"];
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
      session = new CftLspSession(command, args, cwd, this.collection);
      this.sessions.set(key, session);
    }

    this.documentSessions.set(document.uri.toString(), key);
    return session;
  }

  async request(document, method, params) {
    if (document.languageId !== "cft" || document.uri.scheme !== "file") {
      return undefined;
    }
    const session = this.ensureSession(document);
    if (!session) {
      return undefined;
    }
    session.openOrChangeDocument(document);
    return session.request(method, params);
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
  constructor(command, args, cwd, collection) {
    this.command = command;
    this.args = args;
    this.cwd = cwd;
    this.collection = collection;
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

function annotationItem(annotation, range) {
  const item = new vscode.CompletionItem(annotation.label, vscode.CompletionItemKind.Property);
  item.detail = annotation.detail;
  item.documentation = markdown(annotation.documentation);
  item.insertText = annotation.insertText.includes("$")
    ? new vscode.SnippetString(annotation.insertText)
    : annotation.insertText;
  item.range = range;
  item.sortText = `0_${annotation.label}`;
  return item;
}

function simpleItem(label, kind, detail, range, documentation) {
  const item = new vscode.CompletionItem(label, kind);
  item.detail = detail;
  if (documentation) {
    item.documentation = markdown(documentation);
  }
  if (range) {
    item.range = range;
  }
  return item;
}

function functionItems() {
  return BUILTIN_FUNCTIONS.map(([label, documentation]) => {
    const item = simpleItem(label, vscode.CompletionItemKind.Function, "CFT built-in function");
    item.documentation = markdown(documentation);
    item.insertText = new vscode.SnippetString(`${label}($1)`);
    return item;
  });
}

function keywordItem(label) {
  const entry = KEYWORDS.find(([itemLabel]) => itemLabel === label);
  return simpleItem(
    label,
    vscode.CompletionItemKind.Keyword,
    "CFT keyword",
    undefined,
    entry?.[1]
  );
}

function topLevelCompletionItems(linePrefix) {
  const labels = topLevelNeedsTypeKeyword(linePrefix)
    ? ["type"]
    : ["const", "enum", "type", "abstract", "sealed"];
  return labels.map(keywordItem);
}

function literalItems(includeNull) {
  return LITERALS
    .filter(([label]) => includeNull || label !== "null")
    .map(([label, documentation]) =>
      simpleItem(label, vscode.CompletionItemKind.Keyword, "CFT literal", undefined, documentation)
    );
}

function checkExpressionCompletionItems(workspace, document, position, currentType) {
  const items = [
    ...["when", "all", "any", "none"].map(keywordItem),
    ...literalItems(true),
    ...functionItems(),
    ...workspaceConsts(workspace).map((constant) =>
      simpleItem(constant.name, vscode.CompletionItemKind.Constant, "CFT constant")
    )
  ];

  if (currentType) {
    for (const field of fieldsForType(workspace, currentType.name)) {
      items.push(simpleItem(field.name, vscode.CompletionItemKind.Field, `${currentType.name} field`));
    }
  }

  for (const binding of quantifierBindingsAtWithWorkspace(workspace, document, position)) {
    items.push(simpleItem(binding.name, vscode.CompletionItemKind.Variable, "CFT quantifier binding"));
  }

  return items;
}

function fieldDefaultCompletionItems(workspace, field) {
  if (!field) {
    return [
      ...literalItems(true),
      ...workspaceConsts(workspace).map((constant) =>
        simpleItem(constant.name, vscode.CompletionItemKind.Constant, "CFT constant")
      )
    ];
  }

  return [
    ...defaultItemsForType(workspace, field.typeRef),
    ...workspaceConsts(workspace)
      .filter((constant) => constAssignableToType(constant, field.typeRef))
      .map((constant) => simpleItem(constant.name, vscode.CompletionItemKind.Constant, "CFT constant"))
  ];
}

function defaultItemsForType(workspace, typeRef) {
  if (!typeRef) {
    return [];
  }
  switch (typeRef.kind) {
    case "bool":
      return literalItems(false);
    case "named": {
      const variants = workspaceEnumVariants(workspace, typeRef.name);
      return (variants || []).map((variant) =>
        simpleItem(`${typeRef.name}.${variant.name}`, vscode.CompletionItemKind.EnumMember, "CFT enum variant")
      );
    }
    case "array":
      return [simpleItem("[]", vscode.CompletionItemKind.Constant, "Empty array default")];
    case "dict":
      return [simpleItem("{}", vscode.CompletionItemKind.Constant, "Empty object default")];
    case "nullable":
      return [
        simpleItem("null", vscode.CompletionItemKind.Keyword, "CFT literal", undefined, "Nullable value."),
        ...defaultItemsForType(workspace, typeRef.inner)
      ];
    default:
      return [];
  }
}

function constAssignableToType(constant, typeRef) {
  if (!constant || !typeRef) {
    return false;
  }
  if (typeRef.kind === "nullable") {
    return constAssignableToType(constant, typeRef.inner);
  }
  return constant.valueKind === typeRef.kind;
}

function symbolItems(workspace) {
  const items = [];
  for (const type of workspaceTypes(workspace)) {
    items.push(simpleItem(type.name, vscode.CompletionItemKind.Class, "CFT type"));
  }
  for (const enumDef of workspaceEnums(workspace)) {
    items.push(simpleItem(enumDef.name, vscode.CompletionItemKind.Enum, "CFT enum"));
    for (const variant of workspaceEnumVariants(workspace, enumDef.name) || []) {
      const item = simpleItem(
        `${enumDef.name}.${variant.name}`,
        vscode.CompletionItemKind.EnumMember,
        "CFT enum variant"
      );
      item.insertText = `${enumDef.name}.${variant.name}`;
      items.push(item);
    }
  }
  for (const constant of workspaceConsts(workspace)) {
    items.push(simpleItem(constant.name, vscode.CompletionItemKind.Constant, "CFT constant"));
  }
  return items;
}

function typeReferenceItems(workspace) {
  return [
    ...PRIMITIVE_TYPES.map(([label, documentation]) =>
      simpleItem(label, vscode.CompletionItemKind.Keyword, "Primitive type", undefined, documentation)
    ),
    ...workspaceTypes(workspace).map((type) => simpleItem(type.name, vscode.CompletionItemKind.Class, "CFT type")),
    ...workspaceEnums(workspace).map((enumDef) => simpleItem(enumDef.name, vscode.CompletionItemKind.Enum, "CFT enum"))
  ];
}

function dotFieldCompletions(workspace, document, position, receiverChain, range) {
  const receiver = typeOfChainAt(workspace, document, position, receiverChain);
  if (!receiver) {
    return [];
  }

  if (receiver.kind === "dictEntry") {
    return [
      simpleItem("key", vscode.CompletionItemKind.Field, "Dict entry key", range),
      simpleItem("value", vscode.CompletionItemKind.Field, "Dict entry value", range)
    ];
  }

  const typeName = typeNameOf(receiver);
  if (!typeName) {
    return [];
  }

  return fieldsForType(workspace, typeName).map((field) =>
    simpleItem(field.name, vscode.CompletionItemKind.Field, `${typeName} field`, range)
  );
}

function annotationItemsForContext(document, position, range) {
  const target = annotationTargetAt(document, position);
  return ANNOTATIONS.filter((annotation) => annotationAppliesTo(annotation.label, target)).map((annotation) =>
    annotationItem(annotation, range)
  );
}

function annotationTargetAt(document, position) {
  const maxLine = Math.min(document.lineCount, position.line + 8);
  for (let lineNumber = position.line; lineNumber < maxLine; lineNumber += 1) {
    const rawLine = document.lineAt(lineNumber).text;
    const line = maskTrivia(
      lineNumber === position.line ? rawLine.slice(position.character) : rawLine
    ).trim();
    if (!line || line.startsWith("@")) {
      continue;
    }
    if (new RegExp(`^(?:(?:abstract|sealed)\\s+)*type${IDENT_BOUNDARY_AFTER}`, "u").test(line)) {
      return "type";
    }
    if (new RegExp(`^enum${IDENT_BOUNDARY_AFTER}`, "u").test(line)) {
      return "enum";
    }
    if (new RegExp(`^const${IDENT_BOUNDARY_AFTER}`, "u").test(line)) {
      return "const";
    }
    if (new RegExp(`^${IDENT}\\s*:`, "u").test(line)) {
      return "field";
    }
    return "unknown";
  }
  return "unknown";
}

function annotationAppliesTo(label, target) {
  switch (label) {
    case "@struct":
      return target === "type" || target === "unknown";
    case "@flag":
      return target === "enum" || target === "unknown";
    case "@id":
    case "@ref":
    case "@index":
      return target === "field" || target === "unknown";
    case "@display":
    case "@deprecated":
      return target === "type" || target === "enum" || target === "field" || target === "unknown";
    default:
      return true;
  }
}

function isTriviaCompletionPosition(document, position) {
  const linePrefix = document.lineAt(position).text.slice(0, position.character);
  return isAfterLineComment(linePrefix) || isInsideString(linePrefix);
}

function isAfterLineComment(linePrefix) {
  let inString = false;
  let escaped = false;
  for (const char of linePrefix) {
    if (escaped) {
      escaped = false;
      continue;
    }
    if (inString && char === "\\") {
      escaped = true;
      continue;
    }
    if (char === "\"") {
      inString = !inString;
      continue;
    }
    if (!inString && char === "#") {
      return true;
    }
  }
  return false;
}

function isInsideString(linePrefix) {
  let inString = false;
  let escaped = false;
  for (const char of linePrefix) {
    if (escaped) {
      escaped = false;
      continue;
    }
    if (inString && char === "\\") {
      escaped = true;
      continue;
    }
    if (char === "\"") {
      inString = !inString;
    }
  }
  return inString;
}

function isTypePredicateContext(linePrefix) {
  const trimmed = linePrefix.trimEnd();
  return new RegExp(`(?:^|[^${IDENT_CONTINUE.slice(1, -1)}])is(?:\\s+${IDENT_CONTINUE}*)?$`, "u").test(trimmed);
}

function isRefAnnotationContext(linePrefix) {
  return new RegExp(`@\\s*ref\\s*\\(\\s*${IDENT_CONTINUE}*$`, "u").test(linePrefix);
}

function topLevelNeedsTypeKeyword(linePrefix) {
  const match = linePrefix.trimEnd().match(new RegExp(`(${IDENT})$`, "u"));
  return match ? match[1] === "abstract" || match[1] === "sealed" : false;
}

function isTypeHeaderParentContext(linePrefix) {
  const colon = linePrefix.lastIndexOf(":");
  if (colon < 0) {
    return false;
  }
  return new RegExp(`${IDENT_BOUNDARY_BEFORE}type${IDENT_BOUNDARY_AFTER}`, "u").test(linePrefix.slice(0, colon));
}

function isTypeReferenceContext(linePrefix) {
  const trimmed = linePrefix.trimEnd();
  const colon = trimmed.lastIndexOf(":");
  if (colon < 0) {
    return false;
  }
  const afterColon = trimmed.slice(colon + 1);
  return !afterColon.includes(";") && !afterColon.includes("=");
}

function isFieldDefaultContext(linePrefix) {
  const trimmed = linePrefix.trimEnd();
  const equal = trimmed.lastIndexOf("=");
  const colon = trimmed.lastIndexOf(":");
  return colon >= 0 && colon < equal && !trimmed.slice(equal + 1).includes(";");
}

function completionScopeAt(document, symbols, offset) {
  for (const type of symbols.types) {
    if (type.start <= offset && offset <= type.end) {
      return isInsideCheckBlock(document.getText(), type, offset) ? "checkBlock" : "typeBody";
    }
  }
  for (const enumDef of symbols.enums) {
    if (enumDef.start <= offset && offset <= enumDef.end) {
      return "enumBody";
    }
  }
  return "topLevel";
}

function isInsideCheckBlock(text, type, offset) {
  const body = text.slice(type.start, type.end);
  const match = maskTrivia(body).match(/\bcheck\s*\{/);
  if (!match) {
    return false;
  }
  const open = type.start + match.index + match[0].lastIndexOf("{");
  const close = findMatchingBrace(maskTrivia(text), open);
  return open <= offset && offset <= close;
}

function currentFieldFromLinePrefix(linePrefix, currentType) {
  if (!currentType) {
    return undefined;
  }
  const match = linePrefix.match(new RegExp(`^\\s*(${IDENT})\\s*:`, "u"));
  return match ? currentType.fields.find((field) => field.name === match[1]) : undefined;
}

function rangeFromLineMatch(document, position, regex) {
  const prefix = document.lineAt(position).text.slice(0, position.character);
  const match = prefix.match(regex);
  if (!match) {
    return undefined;
  }
  return new vscode.Range(
    position.line,
    position.character - match[0].length,
    position.line,
    position.character
  );
}

function collectSymbols(document) {
  const text = document.getText();
  const masked = maskTrivia(text);
  const types = [];
  const enums = [];
  const consts = [];
  const enumVariants = new Map();

  const constRegex = new RegExp(`${IDENT_BOUNDARY_BEFORE}const\\s+(${IDENT})${IDENT_BOUNDARY_AFTER}(?:\\s*:\\s*(${IDENT}))?\\s*=\\s*([^;]*)`, "gu");
  for (const match of masked.matchAll(constRegex)) {
    const name = match[1];
    const start = match.index + match[0].lastIndexOf(name);
    const end = start + name.length;
    consts.push({
      name,
      start,
      end,
      uri: document.uri,
      valueKind: constValueKind(match[2], match[3])
    });
  }

  const enumRegex = new RegExp(`${IDENT_BOUNDARY_BEFORE}enum\\s+(${IDENT})${IDENT_BOUNDARY_AFTER}`, "gu");
  for (const match of masked.matchAll(enumRegex)) {
    const name = match[1];
    const nameStart = match.index + match[0].lastIndexOf(name);
    const open = masked.indexOf("{", match.index + match[0].length);
    const close = open >= 0 ? findMatchingBrace(masked, open) : -1;
    const end = close >= 0 ? close + 1 : nameStart + name.length;
    const enumDef = { name, start: nameStart, end, uri: document.uri };
    enums.push(enumDef);
    if (open >= 0 && close >= 0) {
      enumVariants.set(name, parseEnumVariants(masked, open + 1, close, document.uri));
    } else {
      enumVariants.set(name, []);
    }
  }

  const typeRegex = new RegExp(`${IDENT_BOUNDARY_BEFORE}(?:(?:abstract|sealed)\\s+)*type\\s+(${IDENT})${IDENT_BOUNDARY_AFTER}`, "gu");
  for (const match of masked.matchAll(typeRegex)) {
    const name = match[1];
    const nameStart = match.index + match[0].lastIndexOf(name);
    const afterName = match.index + match[0].length;
    const headerEnd = masked.indexOf("{", afterName);
    const header = headerEnd >= 0 ? masked.slice(afterName, headerEnd) : "";
    const parentMatch = header.match(new RegExp(`:\\s*(${IDENT})`, "u"));
    const parent = parentMatch ? parentMatch[1] : undefined;
    const open = headerEnd;
    const close = open >= 0 ? findMatchingBrace(masked, open) : -1;
    const end = close >= 0 ? close + 1 : nameStart + name.length;
    const fields = open >= 0 && close >= 0 ? parseFields(masked, open + 1, close) : [];
    types.push({ name, start: nameStart, end, parent, fields, uri: document.uri });
  }

  return { types, enums, consts, enumVariants };
}

function parseEnumVariants(masked, bodyStart, bodyEnd, uri) {
  const body = masked
    .slice(bodyStart, bodyEnd)
    .replace(new RegExp(`@${IDENT}(?:\\([^)]*\\))?`, "gu"), " ");
  const variants = [];
  const variantRegex = new RegExp(`(?:^|,)\\s*(${IDENT})${IDENT_BOUNDARY_AFTER}`, "gu");
  for (const match of body.matchAll(variantRegex)) {
    const name = match[1];
    const start = bodyStart + match.index + match[0].lastIndexOf(name);
    variants.push({ name, start, end: start + name.length, uri });
  }
  return variants;
}

function parseFields(masked, bodyStart, bodyEnd) {
  const body = masked.slice(bodyStart, bodyEnd);
  const checkMatch = body.match(/\bcheck\s*\{/);
  const fieldBody = checkMatch ? body.slice(0, checkMatch.index) : body;
  const fields = [];
  const fieldRegex = new RegExp(
    `(?:^|[;\\n])\\s*((?:@${IDENT}(?:\\([^)]*\\))?\\s*)*)(${IDENT})\\s*:\\s*([^;=]+)`,
    "gu"
  );
  for (const match of fieldBody.matchAll(fieldRegex)) {
    const annotations = match[1] || "";
    const name = match[2];
    const rawType = (match[3] || "").trim();
    if (PRIMITIVE_TYPES.some(([primitive]) => primitive === name)) {
      continue;
    }
    const start = bodyStart + match.index + match[0].lastIndexOf(name);
    fields.push({
      name,
      start,
      end: start + name.length,
      typeRef: parseTypeRefText(rawType),
      typeName: namedTypeFromTypeRef(rawType),
      rawType,
      refTarget: refTargetFromAnnotations(annotations)
    });
  }
  return fields;
}

function namedTypeFromTypeRef(rawType) {
  return typeNameOf(parseTypeRefText(rawType));
}

function parseTypeRefText(rawType) {
  const text = rawType.trim();
  if (!text) {
    return undefined;
  }
  if (text.endsWith("?")) {
    return {
      kind: "nullable",
      inner: parseTypeRefText(text.slice(0, -1))
    };
  }
  if (text.startsWith("[") && text.endsWith("]")) {
    return {
      kind: "array",
      element: parseTypeRefText(text.slice(1, -1))
    };
  }
  if (text.startsWith("{") && text.endsWith("}")) {
    const inner = text.slice(1, -1);
    const colon = findTopLevelColon(inner);
    if (colon >= 0) {
      return {
        kind: "dict",
        key: parseTypeRefText(inner.slice(0, colon)),
        value: parseTypeRefText(inner.slice(colon + 1))
      };
    }
  }
  if (PRIMITIVE_TYPES.some(([primitive]) => primitive === text)) {
    return {
      kind: text
    };
  }
  if (new RegExp(`^${IDENT}$`, "u").test(text)) {
    return {
      kind: "named",
      name: text
    };
  }
  return undefined;
}

function constValueKind(typeName, rawValue) {
  if (typeName && PRIMITIVE_TYPES.some(([primitive]) => primitive === typeName)) {
    return typeName;
  }
  const value = (rawValue || "").trim();
  if (/^-?\d+$/.test(value)) {
    return "int";
  }
  if (/^-?(?:\d+\.\d*|\d*\.\d+)(?:[eE][+-]?\d+)?$/.test(value)) {
    return "float";
  }
  if (value === "true" || value === "false") {
    return "bool";
  }
  if (value.startsWith("\"")) {
    return "string";
  }
  return undefined;
}

function findTopLevelColon(text) {
  let depth = 0;
  for (let index = 0; index < text.length; index += 1) {
    const char = text[index];
    if (char === "{" || char === "[") {
      depth += 1;
    } else if (char === "}" || char === "]") {
      depth -= 1;
    } else if (char === ":" && depth === 0) {
      return index;
    }
  }
  return -1;
}

function typeNameOf(typeRef) {
  if (!typeRef) {
    return undefined;
  }
  if (typeRef.kind === "named") {
    return typeRef.name;
  }
  if (typeRef.kind === "nullable") {
    return typeNameOf(typeRef.inner);
  }
  return undefined;
}

function refTargetFromAnnotations(annotations) {
  const match = annotations.match(new RegExp(`@ref\\s*\\(\\s*(${IDENT})\\s*\\)`, "u"));
  return match ? match[1] : undefined;
}

function currentTypeAt(symbols, offset) {
  return symbols.types.find((type) => type.start <= offset && offset <= type.end);
}

function findMatchingBrace(masked, openIndex) {
  let depth = 0;
  for (let index = openIndex; index < masked.length; index += 1) {
    const char = masked[index];
    if (char === "{") {
      depth += 1;
    } else if (char === "}") {
      depth -= 1;
      if (depth === 0) {
        return index;
      }
    }
  }
  return -1;
}

function maskTrivia(text) {
  const chars = text.split("");
  let index = 0;
  while (index < chars.length) {
    const char = chars[index];
    if (char === "\"") {
      index = maskString(chars, index);
    } else if (char === "#") {
      index = maskLineComment(chars, index);
    } else {
      index += 1;
    }
  }
  return chars.join("");
}

function maskString(chars, start) {
  let index = start + 1;
  while (index < chars.length) {
    if (chars[index] === "\\") {
      chars[index] = " ";
      if (index + 1 < chars.length) {
        chars[index + 1] = " ";
      }
      index += 2;
      continue;
    }
    if (chars[index] === "\"") {
      return index + 1;
    }
    if (chars[index] !== "\n" && chars[index] !== "\r") {
      chars[index] = " ";
    }
    index += 1;
  }
  return index;
}

function maskLineComment(chars, start) {
  let index = start;
  while (index < chars.length && chars[index] !== "\n" && chars[index] !== "\r") {
    chars[index] = " ";
    index += 1;
  }
  return index;
}

function staticDocumentation(text) {
  const fromLists = [...KEYWORDS, ...PRIMITIVE_TYPES, ...LITERALS, ...BUILTIN_FUNCTIONS].find(
    ([label]) => label === text
  );
  if (fromLists) {
    return fromLists[1];
  }
  const annotation = ANNOTATIONS.find((item) => item.label === text);
  return annotation?.documentation;
}

function documentSymbol(document, item, kind) {
  const selection = new vscode.Range(document.positionAt(item.start), document.positionAt(item.end));
  const range = new vscode.Range(document.positionAt(item.start), document.positionAt(item.end));
  return new vscode.DocumentSymbol(item.name, "", kind, range, selection);
}

function markdown(text) {
  const value = new vscode.MarkdownString(text);
  value.isTrusted = false;
  return value;
}

async function collectCftPaths(uri) {
  const projectDir = findNearestCoflowConfigDir(path.dirname(uri.fsPath));
  if (projectDir) {
    const configured = await collectConfiguredSchemaPaths(projectDir);
    if (configured.length > 0) {
      return configured;
    }
  }

  const folder = vscode.workspace.getWorkspaceFolder(uri);
  if (!folder) {
    return [normalizePath(uri.fsPath)];
  }

  const files = await vscode.workspace.findFiles(
    new vscode.RelativePattern(folder, "**/*.cft"),
    new vscode.RelativePattern(folder, "**/{target,node_modules,.git}/**")
  );
  return files.map((file) => normalizePath(file.fsPath));
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
      } else if (stat.isFile() && resolved.toLowerCase().endsWith(".cft")) {
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
    } else if (entry.isFile() && entry.name.toLowerCase().endsWith(".cft")) {
      output.push(normalizePath(fullPath));
    }
  }
  return output;
}

async function collectWorkspaceSymbols(document) {
  const symbolsByUri = new Map();
  const openDocuments = new Map();
  for (const openDocument of vscode.workspace.textDocuments) {
    if (openDocument.languageId === "cft" && openDocument.uri.scheme === "file") {
      openDocuments.set(openDocument.uri.toString(), openDocument);
    }
  }

  for (const openDocument of openDocuments.values()) {
    symbolsByUri.set(openDocument.uri.toString(), {
      document: openDocument,
      symbols: collectSymbols(openDocument)
    });
  }

  for (const filePath of await collectCftPaths(document.uri)) {
    const uri = vscode.Uri.file(filePath);
    if (symbolsByUri.has(uri.toString())) {
      continue;
    }
    try {
      const text = await fs.promises.readFile(filePath, "utf8");
      const diskDocument = {
        uri,
        getText: () => text,
        positionAt: (offset) => positionAtText(text, offset)
      };
      symbolsByUri.set(uri.toString(), {
        document: diskDocument,
        symbols: collectSymbols(diskDocument)
      });
    } catch {
      // Ignore unreadable files. Diagnostics will report file errors separately.
    }
  }

  const workspace = {
    documents: symbolsByUri,
    types: new Map(),
    enums: new Map(),
    consts: new Map(),
    enumVariants: new Map()
  };

  for (const entry of symbolsByUri.values()) {
    for (const type of entry.symbols.types) {
      pushMap(workspace.types, type.name, { item: type, document: entry.document });
    }
    for (const enumDef of entry.symbols.enums) {
      pushMap(workspace.enums, enumDef.name, { item: enumDef, document: entry.document });
      for (const variant of entry.symbols.enumVariants.get(enumDef.name) || []) {
        pushMap(workspace.enumVariants, `${enumDef.name}.${variant.name}`, {
          item: variant,
          document: entry.document
        });
      }
    }
    for (const constant of entry.symbols.consts) {
      pushMap(workspace.consts, constant.name, { item: constant, document: entry.document });
    }
  }

  return workspace;
}

function pushMap(map, key, value) {
  const values = map.get(key) || [];
  values.push(value);
  map.set(key, values);
}

function globalSymbolLocations(workspace, name) {
  return [
    ...locationsForEntries(workspace.types.get(name)),
    ...locationsForEntries(workspace.enums.get(name)),
    ...locationsForEntries(workspace.consts.get(name))
  ];
}

function workspaceTypes(workspace) {
  return uniqueEntries(workspace.types);
}

function workspaceEnums(workspace) {
  return uniqueEntries(workspace.enums);
}

function workspaceConsts(workspace) {
  return uniqueEntries(workspace.consts);
}

function uniqueEntries(map) {
  const items = [];
  const seen = new Set();
  for (const entries of map.values()) {
    for (const entry of entries) {
      const key = `${entry.document.uri.toString()}#${entry.item.start}`;
      if (!seen.has(key)) {
        seen.add(key);
        items.push(entry.item);
      }
    }
  }
  return items.sort((left, right) => left.name.localeCompare(right.name));
}

function workspaceEnumVariants(workspace, enumName) {
  const variants = locationsEntriesToItems(workspace.enumVariants, `${enumName}.`);
  return variants.length > 0 ? variants : undefined;
}

function locationsEntriesToItems(map, prefix) {
  const items = [];
  const seen = new Set();
  for (const [key, entries] of map.entries()) {
    if (!key.startsWith(prefix)) {
      continue;
    }
    for (const entry of entries) {
      const itemKey = `${entry.document.uri.toString()}#${entry.item.start}`;
      if (!seen.has(itemKey)) {
        seen.add(itemKey);
        items.push(entry.item);
      }
    }
  }
  return items.sort((left, right) => left.name.localeCompare(right.name));
}

function enumVariantLocations(workspace, chain) {
  if (chain.length < 2) {
    return [];
  }
  const enumName = chain[chain.length - 2].name;
  const variantName = chain[chain.length - 1].name;
  return locationsForEntries(workspace.enumVariants.get(`${enumName}.${variantName}`));
}

function directFieldLocations(workspace, document, position, fieldName) {
  const entry = workspace.documents.get(document.uri.toString());
  if (!entry) {
    return [];
  }
  const offset = document.offsetAt(position);
  const currentType = currentTypeAt(entry.symbols, offset);
  const field = currentType?.fields.find((item) => item.name === fieldName);
  return field ? [locationForItem(document, field)] : [];
}

function fieldChainLocations(workspace, document, position, chain) {
  if (chain.length < 2) {
    return [];
  }

  const root = chain[0].name;
  let typeName = typeNameOf(typeOfNameAt(workspace, document, position, root));
  if (!typeName) {
    return [];
  }

  let field;
  for (const part of chain.slice(1)) {
    field = fieldByType(workspace, typeName, part.name);
    if (!field) {
      return [];
    }
    typeName = typeNameOf(fieldReceiverType(field.item));
  }

  return field ? [locationForItem(field.document, field.item)] : [];
}

function typeOfNameAt(workspace, document, position, name) {
  const entry = workspace.documents.get(document.uri.toString());
  if (!entry) {
    return undefined;
  }
  const currentType = currentTypeAt(entry.symbols, document.offsetAt(position));
  const field = currentType
    ? fieldsForType(workspace, currentType.name).find((item) => item.name === name)
    : undefined;
  if (field) {
    return fieldReceiverType(field);
  }

  const binding = quantifierBindingsAtWithWorkspace(workspace, document, position)
    .reverse()
    .find((item) => item.name === name);
  if (binding) {
    return binding.typeRef;
  }
  return undefined;
}

function typeOfChainAt(workspace, document, position, chain) {
  if (chain.length === 0) {
    return undefined;
  }

  let typeRef = typeOfNameAt(workspace, document, position, chain[0]);
  for (const part of chain.slice(1)) {
    const typeName = typeNameOf(typeRef);
    const field = typeName ? fieldByType(workspace, typeName, part)?.item : undefined;
    if (!field) {
      return undefined;
    }
    typeRef = fieldReceiverType(field);
  }
  return typeRef;
}

function fieldByType(workspace, typeName, fieldName) {
  for (const entry of typeEntriesForType(workspace, typeName)) {
    const field = fieldsForType(workspace, entry.item.name).find((item) => item.name === fieldName);
    if (field) {
      return { item: field, document: entry.document };
    }
  }
  return undefined;
}

function quantifierBindingsAt(document, position) {
  const symbols = collectSymbols(document);
  const workspaceEntry = {
    documents: new Map([[document.uri.toString(), { document, symbols }]]),
    types: new Map(),
    enums: new Map(),
    consts: new Map(),
    enumVariants: new Map()
  };
  for (const type of symbols.types) {
    pushMap(workspaceEntry.types, type.name, { item: type, document });
  }

  return quantifierBindingsAtWithWorkspace(workspaceEntry, document, position);
}

function quantifierBindingsAtWithWorkspace(workspace, document, position) {
  const text = document.getText();
  const masked = maskTrivia(text);
  const offset = document.offsetAt(position);
  const bindings = [];
  const quantifierRegex = new RegExp(`${IDENT_BOUNDARY_BEFORE}(all|any|none)\\s+(${IDENT})\\s+in\\s+([^{};]+)\\{`, "gu");

  for (const match of masked.matchAll(quantifierRegex)) {
    const open = match.index + match[0].lastIndexOf("{");
    const close = findMatchingBrace(masked, open);
    if (close < 0 || offset <= open || offset > close) {
      continue;
    }

    const collection = match[3].trim();
    const collectionType = typeOfSimpleExpression(workspace, document, document.positionAt(match.index), collection);
    const bindingType = quantifierBindingType(collectionType);
    if (bindingType) {
      bindings.push({
        name: match[2],
        typeRef: bindingType
      });
    }
  }

  return bindings;
}

function typeOfSimpleExpression(workspace, document, position, text) {
  const parts = [...text.matchAll(new RegExp(IDENT, "gu"))].map((match) => match[0]);
  if (parts.length === 0) {
    return undefined;
  }

  let typeRef = typeOfFieldOrCurrentName(workspace, document, position, parts[0]);
  for (const part of parts.slice(1)) {
    const typeName = typeNameOf(typeRef);
    const field = typeName ? fieldByType(workspace, typeName, part)?.item : undefined;
    if (!field) {
      return undefined;
    }
    typeRef = fieldReceiverType(field);
  }
  return typeRef;
}

function typeOfFieldOrCurrentName(workspace, document, position, name) {
  const entry = workspace.documents.get(document.uri.toString());
  if (!entry) {
    return undefined;
  }
  const currentType = currentTypeAt(entry.symbols, document.offsetAt(position));
  const field = currentType
    ? fieldsForType(workspace, currentType.name).find((item) => item.name === name)
    : undefined;
  return field ? fieldReceiverType(field) : undefined;
}

function quantifierBindingType(collectionType) {
  if (!collectionType) {
    return undefined;
  }
  if (collectionType.kind === "nullable") {
    return quantifierBindingType(collectionType.inner);
  }
  if (collectionType.kind === "array") {
    return collectionType.element;
  }
  if (collectionType.kind === "dict") {
    return {
      kind: "dictEntry",
      key: collectionType.key,
      value: collectionType.value
    };
  }
  return undefined;
}

function fieldReceiverType(field) {
  if (field.refTarget) {
    return {
      kind: "named",
      name: field.refTarget
    };
  }
  return field.typeRef;
}

function fieldsForType(workspace, typeName, seen = new Set()) {
  if (!typeName || seen.has(typeName)) {
    return [];
  }
  seen.add(typeName);

  const entries = typeEntriesForType(workspace, typeName);
  const fields = [];
  const fieldNames = new Set();
  for (const entry of entries) {
    for (const parentField of fieldsForType(workspace, entry.item.parent, seen)) {
      if (!fieldNames.has(parentField.name)) {
        fieldNames.add(parentField.name);
        fields.push(parentField);
      }
    }
    for (const field of entry.item.fields) {
      if (!fieldNames.has(field.name)) {
        fieldNames.add(field.name);
        fields.push(field);
      }
    }
  }
  return fields;
}

function typeEntriesForType(workspace, typeName) {
  return workspace.types.get(typeName) || [];
}

function locationsForEntries(entries) {
  return (entries || []).map((entry) => locationForItem(entry.document, entry.item));
}

function locationForItem(document, item) {
  const start = document.positionAt(item.start);
  const end = document.positionAt(item.end);
  return new vscode.Location(document.uri, new vscode.Range(start, end));
}

function dottedChainAt(document, range) {
  const line = document.lineAt(range.start.line).text;
  const left = line.slice(0, range.end.character);
  const right = line.slice(range.end.character);
  const leftMatch = left.match(new RegExp(`(${IDENT}(?:\\s*\\.\\s*${IDENT})*)$`, "u"));
  const rightMatch = right.match(new RegExp(`^(?:\\s*\\.\\s*${IDENT})*`, "u"));
  const text = `${leftMatch ? leftMatch[1] : ""}${rightMatch ? rightMatch[0] : ""}`;
  return [...text.matchAll(new RegExp(IDENT, "gu"))].map((match) => ({ name: match[0] }));
}

function positionAtText(text, offset) {
  const target = Math.max(0, Math.min(offset, text.length));
  let line = 0;
  let character = 0;
  for (let index = 0; index < target; index += 1) {
    const ch = text[index];
    if (ch === "\n") {
      line += 1;
      character = 0;
    } else {
      character += 1;
    }
  }
  return new vscode.Position(line, character);
}

function isBuiltinName(name) {
  return (
    KEYWORDS.some(([label]) => label === name) ||
    PRIMITIVE_TYPES.some(([label]) => label === name) ||
    LITERALS.some(([label]) => label === name) ||
    BUILTIN_FUNCTIONS.some(([label]) => label === name)
  );
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
  if (command !== "coflow" || JSON.stringify(args) !== JSON.stringify(["cft", "lsp"])) {
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
  if (!Array.isArray(definitions)) {
    return undefined;
  }

  const locations = definitions.map(lspLocationToVsCode).filter(Boolean);
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
    CftLspSession,
    collectConfiguredSchemaPaths,
    localDefinitionLocations,
    schemaEntriesFromCoflowConfigText,
    lspDefinitionLocations,
    vscodePosition: vscode.Position,
    vscodeRange: vscode.Range,
    vscodeUriFile: vscode.Uri.file
  }
};
