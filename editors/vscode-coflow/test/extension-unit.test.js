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
  assert.deepStrictEqual(
    extension.__test.schemaEntriesFromCoflowConfigText("schema:\n  - 02_enums_and_flags.cft\n  - 03_types_fields_defaults.cft\n"),
    ["02_enums_and_flags.cft", "03_types_fields_defaults.cft"]
  );
  assert.deepStrictEqual(
    extension.__test.schemaEntriesFromCoflowConfigText("schema: schema/\n"),
    ["schema/"]
  );

  const root = fs.mkdtempSync(path.join(os.tmpdir(), "coflow-vscode-cft-"));
  fs.mkdirSync(path.join(root, "schema"));
  fs.writeFileSync(path.join(root, "coflow.yaml"), "schema: schema/\n", "utf8");
  fs.writeFileSync(path.join(root, "schema", "a.cft"), "enum A { X, }\n", "utf8");
  fs.writeFileSync(path.join(root, "schema", "b.cft"), "type B {}\n", "utf8");

  const paths = await extension.__test.collectConfiguredSchemaPaths(root);
  assert.deepStrictEqual(
    paths.map((item) => path.basename(item)).sort(),
    ["a.cft", "b.cft"]
  );

  fs.rmSync(root, { recursive: true, force: true });

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
  assert(
    extensionPackage.contributes.configurationDefaults["editor.semanticTokenColorCustomizations"].rules[
      "namespace.declaration.record:cfd"
    ],
    "CFD record declarations should have a default semantic token color"
  );
  assert(
    extensionPackage.contributes.configurationDefaults["editor.semanticTokenColorCustomizations"].rules[
      "property.reference.path.schema:cfd"
    ],
    "CFD reference path segments should have a default semantic token color"
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

  const completionSource = "type Item {}\ntype Holder {\n  item: Item;\n  @ref(\n}\n";
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

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
