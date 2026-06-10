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
    SemanticTokensLegend: class {},
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
      textDocuments: []
    }
  };
};

const extension = require("../src/extension.js");

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

  const project = fs.mkdtempSync(path.join(os.tmpdir(), "coflow-vscode-cft-definition-"));
  const enumPath = path.join(project, "02_enums_and_flags.cft");
  const sourcePath = path.join(project, "03_types_fields_defaults.cft");
  fs.writeFileSync(
    path.join(project, "coflow.yaml"),
    "schema:\n  - 02_enums_and_flags.cft\n  - 03_types_fields_defaults.cft\n",
    "utf8"
  );
  fs.writeFileSync(enumPath, "enum ExampleRarity {\n  Common = 0,\n}\n", "utf8");
  const source = "type ExampleItem {\n  rarity: ExampleRarity = ExampleRarity.Common;\n}\n";
  fs.writeFileSync(sourcePath, source, "utf8");

  const document = textDocument(sourcePath, source);
  const enumLocations = await extension.__test.localDefinitionLocations(
    document,
    document.positionAt(source.indexOf("ExampleRarity") + 4)
  );
  assert.strictEqual(path.normalize(enumLocations[0].uri.fsPath), path.normalize(enumPath));
  assert.strictEqual(enumLocations[0].range.start.line, 0);
  assert.strictEqual(enumLocations[0].range.start.character, 5);

  const variantLocations = await extension.__test.localDefinitionLocations(
    document,
    document.positionAt(source.indexOf("Common") + 2)
  );
  assert.strictEqual(path.normalize(variantLocations[0].uri.fsPath), path.normalize(enumPath));
  assert.strictEqual(variantLocations[0].range.start.line, 1);
  assert.strictEqual(variantLocations[0].range.start.character, 2);

  fs.rmSync(project, { recursive: true, force: true });

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
