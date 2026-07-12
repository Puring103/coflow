const assert = require("assert");
const fs = require("fs");
const Module = require("module");
const os = require("os");
const path = require("path");

const originalLoad = Module._load;
Module._load = function load(request, parent, isMain) {
  if (request !== "vscode") {
    return originalLoad(request, parent, isMain);
  }
  return {
    CompletionItem: class {},
    CompletionItemKind: {},
    Diagnostic: class {},
    DiagnosticRelatedInformation: class {},
    DiagnosticSeverity: {},
    DocumentSymbol: class {},
    Hover: class {},
    Location: class {
      constructor(uri, range) {
        this.uri = uri;
        this.range = range;
      }
    },
    MarkdownString: class {},
    Position: class {
      constructor(line, character) {
        this.line = line;
        this.character = character;
      }
      compareTo(other) {
        return this.line === other.line
          ? this.character - other.character
          : this.line - other.line;
      }
    },
    Range: class {
      constructor(startLine, startCharacter, endLine, endCharacter) {
        this.start = typeof startLine === "object"
          ? startLine
          : new this.constructor.VscodePosition(startLine, startCharacter);
        this.end = typeof startLine === "object"
          ? startCharacter
          : new this.constructor.VscodePosition(endLine, endCharacter);
      }
      static VscodePosition = class {
        constructor(line, character) {
          this.line = line;
          this.character = character;
        }
        compareTo(other) {
          return this.line === other.line
            ? this.character - other.character
            : this.line - other.line;
        }
      };
    },
    RelativePattern: class {},
    SemanticTokensBuilder: class {},
    SemanticTokensLegend: class {
      constructor(tokenTypes, tokenModifiers) {
        this.tokenTypes = tokenTypes;
        this.tokenModifiers = tokenModifiers;
      }
    },
    SnippetString: class {},
    SymbolKind: {},
    TextEdit: class {},
    Uri: {
      file(filePath) {
        return {
          fsPath: filePath,
          scheme: "file",
          toString: () => `file://${filePath.replace(/\\/g, "/")}`
        };
      },
      parse(value) {
        return {
          fsPath: value.replace(/^file:\/\//, ""),
          scheme: value.split(":")[0],
          toString: () => value
        };
      }
    },
    languages: {},
    workspace: {
      textDocuments: [],
      getConfiguration() {
        return {
          get(key, fallback) {
            return fallback;
          }
        };
      },
      getWorkspaceFolder() {
        return undefined;
      },
      findFiles: async () => []
    }
  };
};

const extension = require("../src/extension.js");
const extensionPackage = require("../package.json");

async function main() {
  const extensionRoot = path.resolve(__dirname, "..");
  for (const file of jsonFilesUnder(extensionRoot)) {
    assert.doesNotThrow(
      () => JSON.parse(fs.readFileSync(file, "utf8")),
      `${path.relative(extensionRoot, file)} should be valid JSON`
    );
  }
  for (const contributionPath of packageContributionPaths(extensionPackage)) {
    assert(
      fs.existsSync(path.resolve(extensionRoot, contributionPath)),
      `package contribution path should exist: ${contributionPath}`
    );
  }

  const singleLocation = extension.__test.lspDefinitionLocations({
    uri: "file:///tmp/source.cft",
    range: { start: { line: 2, character: 2 }, end: { line: 2, character: 8 } }
  });
  assert.strictEqual(singleLocation.length, 1);
  assert.strictEqual(singleLocation[0].range.start.line, 2);
  assert.strictEqual(singleLocation[0].range.start.character, 2);

  assert.deepStrictEqual(extension.__test.semanticTokensLegend.tokenModifiers, [
    "declaration",
    "reference",
    "path",
    "record",
    "schema"
  ]);
  assert.deepStrictEqual(
    extensionPackage.contributes.semanticTokenModifiers.map((modifier) => modifier.id),
    ["reference", "path", "record", "schema"]
  );
  assert(extension.__test.isPathWithin(extensionRoot, path.join(extensionRoot, "syntaxes", "cft.tmLanguage.json")));
  assert(!extension.__test.isPathWithin(extensionRoot, path.join(path.dirname(extensionRoot), "outside.cft")));
  assert(
    extensionPackage.contributes.configurationDefaults["editor.semanticTokenColorCustomizations"].rules[
      "namespace.declaration.record:cfd"
    ],
    "CFD record declarations should have a default semantic token color"
  );
  assert.strictEqual(
    extensionPackage.contributes.configurationDefaults["editor.semanticTokenColorCustomizations"].rules[
      "property.reference.path.schema:cfd"
    ],
    undefined,
    "CFD field access path segments should not have positive metadata after ref simplification"
  );
  assert(
    extensionPackage.contributes.configurationDefaults["editor.semanticTokenColorCustomizations"].rules[
      "property.reference.path.schema:cft"
    ],
    "CFT check expression path segments should keep a default semantic token color"
  );
  const cftGrammar = JSON.parse(
    fs.readFileSync(path.join(__dirname, "..", "syntaxes", "cft.tmLanguage.json"), "utf8")
  );
  const annotationPattern = cftGrammar.repository.annotations.patterns[0].match;
  assert(!annotationPattern.includes("ref"), "CFT TextMate grammar must not treat @ref as valid");
  assert(!annotationPattern.includes("inline"), "CFT TextMate grammar must not treat @inline as valid");
  assert(
    cftGrammar.repository.references.patterns.every(
      (pattern) => !String(pattern.name).includes("typed")
    ),
    "CFT TextMate grammar must not include old typed reference rules"
  );

  const session = Object.create(extension.__test.CftLspSession.prototype);
  Object.assign(session, {
    failed: false,
    disposed: false,
    nextId: 1,
    pending: new Map(),
    send(message) {
      this.handleMessage({
        jsonrpc: "2.0",
        id: message.id,
        result: [{ uri: "file:///tmp/target.cft", range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } } }]
      });
    }
  });
  const immediate = await Promise.race([
    session.request("textDocument/definition", {}),
    new Promise((resolve) => setTimeout(() => resolve("timed-out"), 25))
  ]);
  assert.notStrictEqual(immediate, "timed-out");

  const watchedNotifications = [];
  const watchedSession = Object.create(extension.__test.CftLspSession.prototype);
  Object.assign(watchedSession, {
    failed: false,
    disposed: false,
    openedUris: new Set(),
    sendNotification(method, params) {
      watchedNotifications.push({ method, params });
    }
  });
  watchedSession.notifyWatchedFile(extension.__test.vscodeUriFile("/tmp/closed.cfd"), 2);
  assert.deepStrictEqual(watchedNotifications, [
    {
      method: "workspace/didChangeWatchedFiles",
      params: {
        changes: [{ uri: "file:///tmp/closed.cfd", type: 2 }]
      }
    }
  ]);

  const diagnosticsWrites = [];
  const disabledDiagnosticsSession = Object.create(extension.__test.CftLspSession.prototype);
  Object.assign(disabledDiagnosticsSession, {
    collection: {
      set(uri, diagnostics) {
        diagnosticsWrites.push({ uri, diagnostics });
      }
    },
    uriFromLsp: (rawUri) => extension.__test.vscodeUriFile(rawUri.replace(/^file:\/\//, "")),
    diagnosticsEnabledForUri: () => false
  });
  disabledDiagnosticsSession.handleMessage({
    method: "textDocument/publishDiagnostics",
    params: {
      uri: "file:///tmp/disabled.cft",
      diagnostics: [
        {
          range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } },
          message: "ignored when diagnostics are disabled"
        }
      ]
    }
  });
  assert.deepStrictEqual(
    diagnosticsWrites,
    [],
    "diagnostics setting must suppress publishDiagnostics without disabling LSP-backed features"
  );

  const disposedDiagnosticsWrites = [];
  const disposedDiagnosticsSession = Object.create(extension.__test.CftLspSession.prototype);
  Object.assign(disposedDiagnosticsSession, {
    disposed: true,
    collection: {
      set(uri, diagnostics) {
        disposedDiagnosticsWrites.push({ uri, diagnostics });
      }
    },
    uriFromLsp: (rawUri) => extension.__test.vscodeUriFile(rawUri.replace(/^file:\/\//, "")),
    diagnosticsEnabledForUri: () => true
  });
  disposedDiagnosticsSession.handleMessage({
    method: "textDocument/publishDiagnostics",
    params: {
      uri: "file:///tmp/disposed.cft",
      diagnostics: [
        {
          range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } },
          message: "late diagnostics from a disposed session"
        }
      ]
    }
  });
  assert.deepStrictEqual(
    disposedDiagnosticsWrites,
    [],
    "a disposed session must not publish late diagnostics over its replacement"
  );

  const supersededDiagnosticsWrites = [];
  const supersededDiagnosticsSession = Object.create(extension.__test.CftLspSession.prototype);
  Object.assign(supersededDiagnosticsSession, {
    disposed: false,
    collection: {
      set(uri, diagnostics) {
        supersededDiagnosticsWrites.push({ uri, diagnostics });
      }
    },
    uriFromLsp: (rawUri) => extension.__test.vscodeUriFile(rawUri.replace(/^file:\/\//, "")),
    diagnosticsEnabledForUri: () => true,
    ownsDiagnosticUri: () => false
  });
  supersededDiagnosticsSession.handleMessage({
    method: "textDocument/publishDiagnostics",
    params: {
      uri: "file:///tmp/superseded.cft",
      diagnostics: [
        {
          range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } },
          message: "late diagnostics from a superseded session"
        }
      ]
    }
  });
  assert.deepStrictEqual(
    supersededDiagnosticsWrites,
    [],
    "only the session currently assigned to a document may publish its diagnostics"
  );

  const scopedDiagnosticsWrites = [];
  const scopedDiagnosticsSession = Object.create(extension.__test.CftLspSession.prototype);
  Object.assign(scopedDiagnosticsSession, {
    collection: {
      set(uri, diagnostics) {
        scopedDiagnosticsWrites.push({ uri, diagnostics });
      }
    },
    uriFromLsp: (rawUri) => extension.__test.vscodeUriFile(rawUri.replace(/^file:\/\//, "")),
    diagnosticsEnabledForUri: () => false
  });
  scopedDiagnosticsSession.handleMessage({
    method: "textDocument/publishDiagnostics",
    params: {
      uri: "file:///tmp/scoped-disabled.cft",
      diagnostics: [
        {
          range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } },
          message: "ignored by resource scoped diagnostics setting"
        }
      ]
    }
  });
  assert.deepStrictEqual(
    scopedDiagnosticsWrites,
    [],
    "diagnostics filtering must use the diagnostic URI, not only a session-level flag"
  );

  const failureDiagnosticsWrites = [];
  const disabledFailureSession = Object.create(extension.__test.CftLspSession.prototype);
  Object.assign(disabledFailureSession, {
    collection: {
      set(uri, diagnostics) {
        failureDiagnosticsWrites.push({ uri, diagnostics });
      }
    },
    diagnosticsEnabledForUri: () => false,
    failureMessage: "server failed"
  });
  disabledFailureSession.publishFailure(extension.__test.vscodeUriFile("/tmp/failed.cft"));
  assert.deepStrictEqual(
    failureDiagnosticsWrites,
    [],
    "diagnostics setting must suppress local language-server failure diagnostics"
  );

  const completionSource = "type Item {}\ntype Holder {\n  item: &Item;\n}\n";
  const completionDocument = textDocument(path.join(os.tmpdir(), "completion.cft"), completionSource);
  const completionProvider = new extension.__test.CftCompletionProvider({
    request: async () => undefined
  });
  const noCompletionFallback = await completionProvider.provideCompletionItems(
    completionDocument,
    completionDocument.positionAt(completionSource.indexOf("item: ") + "item: ".length)
  );
  assert.deepStrictEqual(
    noCompletionFallback,
    [],
    "VS Code extension must not synthesize semantic completions when LSP has no result"
  );

  const hoverProvider = new extension.__test.CftHoverProvider({ request: async () => undefined });
  const noHoverFallback = await hoverProvider.provideHover(
    completionDocument,
    completionDocument.positionAt(completionSource.indexOf("Item"))
  );
  assert.strictEqual(noHoverFallback, undefined);

  const symbolProvider = new extension.__test.CftDocumentSymbolProvider({ request: async () => undefined });
  const noSymbolFallback = await symbolProvider.provideDocumentSymbols(completionDocument);
  assert.deepStrictEqual(noSymbolFallback, []);

  const definitionProvider = new extension.__test.CftDefinitionProvider({ request: async () => undefined });
  const noDefinitionFallback = await definitionProvider.provideDefinition(
    completionDocument,
    completionDocument.positionAt(completionSource.indexOf("Item"))
  );
  assert.strictEqual(noDefinitionFallback, undefined);
}

function textDocument(filePath, text) {
  const uri = extension.__test.vscodeUriFile(filePath);
  const lines = text.split(/\r?\n/);
  return {
    uri,
    languageId: "cft",
    getText(range) {
      if (!range) {
        return text;
      }
      return text.slice(this.offsetAt(range.start), this.offsetAt(range.end));
    },
    lineAt(line) {
      return { text: lines[line] || "" };
    },
    offsetAt(position) {
      let line = 0;
      let character = 0;
      for (let index = 0; index < text.length; index += 1) {
        if (line === position.line && character === position.character) {
          return index;
        }
        if (text[index] === "\n") {
          line += 1;
          character = 0;
        } else {
          character += 1;
        }
      }
      return text.length;
    },
    positionAt(offset) {
      let line = 0;
      let character = 0;
      for (let index = 0; index < Math.min(offset, text.length); index += 1) {
        if (text[index] === "\n") {
          line += 1;
          character = 0;
        } else {
          character += 1;
        }
      }
      return new extension.__test.vscodePosition(line, character);
    },
    getWordRangeAtPosition(position) {
      const line = this.lineAt(position.line).text;
      let start = position.character;
      if (start >= line.length || !isIdentContinue(line[start])) {
        if (start > 0 && isIdentContinue(line[start - 1])) {
          start -= 1;
        }
      }
      while (start > 0 && isIdentContinue(line[start - 1])) {
        start -= 1;
      }
      let end = start;
      while (end < line.length && isIdentContinue(line[end])) {
        end += 1;
      }
      return end > start
        ? new extension.__test.vscodeRange(position.line, start, position.line, end)
        : undefined;
    }
  };
}

function isIdentContinue(char) {
  return Boolean(char) && /[_\p{ID_Continue}]/u.test(char);
}

function jsonFilesUnder(root) {
  const files = [];
  for (const entry of fs.readdirSync(root, { withFileTypes: true })) {
    const fullPath = path.join(root, entry.name);
    if (entry.isDirectory()) {
      files.push(...jsonFilesUnder(fullPath));
    } else if (entry.name.endsWith(".json")) {
      files.push(fullPath);
    }
  }
  return files;
}

function packageContributionPaths(packageJson) {
  const paths = [];
  for (const language of packageJson.contributes.languages || []) {
    if (language.configuration) {
      paths.push(language.configuration);
    }
  }
  for (const grammar of packageJson.contributes.grammars || []) {
    if (grammar.path) {
      paths.push(grammar.path);
    }
  }
  for (const snippet of packageJson.contributes.snippets || []) {
    if (snippet.path) {
      paths.push(snippet.path);
    }
  }
  return paths;
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
