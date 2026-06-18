const assert = require("assert");
const fs = require("fs");
const Module = require("module");
const os = require("os");
const path = require("path");

const vscodeMock = {
  panels: [],
  activeTextEditor: undefined
};

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
    ViewColumn: {
      One: 1,
      Beside: -2
    },
    commands: {
      registerCommand() {
        return disposable();
      }
    },
    languages: {},
    window: {
      get activeTextEditor() {
        return vscodeMock.activeTextEditor;
      },
      set activeTextEditor(editor) {
        vscodeMock.activeTextEditor = editor;
      },
      createWebviewPanel(viewType, title, column, options) {
        const panel = {
          viewType,
          title,
          column,
          options,
          revealCalls: [],
          disposed: false,
          webview: {
            html: "",
            postMessages: [],
            onDidReceiveMessage() {
              return disposable();
            },
            async postMessage(message) {
              this.postMessages.push(message);
              return true;
            }
          },
          onDidDispose() {
            return disposable();
          },
          reveal(viewColumn, revealOptions) {
            this.revealCalls.push({ viewColumn, revealOptions });
          },
          dispose() {
            this.disposed = true;
          }
        };
        vscodeMock.panels.push(panel);
        return panel;
      },
      showErrorMessage: async () => undefined,
      showTextDocument: async () => undefined
    },
    workspace: {
      textDocuments: [],
      getWorkspaceFolder() {
        return undefined;
      },
      findFiles: async () => [],
      onDidChangeTextDocument() {
        return disposable();
      },
      onDidCloseTextDocument() {
        return disposable();
      }
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

  const singleLocation = extension.__test.lspDefinitionLocations({
    uri: `file://${sourcePath.replace(/\\/g, "/")}`,
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
  const cfdInspectorCommand = extensionPackage.contributes.commands?.find(
    (command) => command.command === "coflow.openCfdInspector"
  );
  assert(cfdInspectorCommand, "CFD inspector command should be contributed");
  assert.strictEqual(cfdInspectorCommand.title, "Open CFD Inspector");
  assert.strictEqual(cfdInspectorCommand.category, "Coflow");
  assert.strictEqual(cfdInspectorCommand.icon, "$(open-preview)");
  const cfdInspectorTitleMenu = extensionPackage.contributes.menus?.["editor/title"]?.find(
    (menu) => menu.command === "coflow.openCfdInspector"
  );
  assert(cfdInspectorTitleMenu, "CFD inspector should be available from the editor title toolbar");
  assert.strictEqual(cfdInspectorTitleMenu.when, "resourceLangId == cfd");
  assert.strictEqual(cfdInspectorTitleMenu.group, "navigation@1");
  assert(
    extensionPackage.activationEvents.includes("onCommand:coflow.openCfdInspector"),
    "CFD inspector command should activate the extension when invoked"
  );
  const graphColumns = extension.__test.computeGraphColumns(
    [
      { id: "A", key: "A" },
      { id: "B", key: "B" },
      { id: "X", key: "X" },
      { id: "Y", key: "Y" }
    ],
    [
      { sourceRecordId: "A", targetRecordId: "Y" },
      { sourceRecordId: "B", targetRecordId: "X" }
    ]
  );
  assert.deepStrictEqual(
    [...graphColumns.entries()].map(([depth, records]) => [depth, records.map((record) => record.id)]),
    [
      [0, ["A", "B"]],
      [1, ["Y", "X"]]
    ],
    "graph layout should order columns by connected neighbor positions instead of key only"
  );
  const inspectorHtml = extension.__test.buildCfdInspectorHtml({
    recordsInFile: [],
    references: [],
    graph: {
      canShow: true,
      records: [{ id: "A", key: "A" }],
      references: [],
      hiddenIsolatedAnchors: []
    }
  });
  const scriptStart = inspectorHtml.indexOf("<script");
  const scriptBodyStart = inspectorHtml.indexOf(">", scriptStart) + 1;
  const scriptEnd = inspectorHtml.indexOf("</script>", scriptBodyStart);
  const inspectorScript = inspectorHtml.slice(scriptBodyStart, scriptEnd);
  assert(
    inspectorScript.includes("function computeGraphColumns"),
    "webview script should define computeGraphColumns inside its own runtime"
  );
  assert(
    inspectorScript.includes("function graphAnchorLocalBox"),
    "webview script should define graphAnchorLocalBox for field endpoint placement"
  );
  assert(
    inspectorScript.indexOf("function computeGraphColumns") < inspectorScript.indexOf("function graphView"),
    "computeGraphColumns should be defined before graphView can call it"
  );
  assert(
    inspectorHtml.includes("grid-template-columns: minmax(44px, 30%) minmax(0, 1fr);"),
    "field rows should leave more room for nested values in narrow graph nodes"
  );
  assert(
    inspectorHtml.includes(".nested-fields { margin-top: 4px; padding-left: 4px; border-left: 1px solid var(--border); display: grid; gap: 4px; }"),
    "nested values should use only a slight indentation"
  );
  assert(
    inspectorHtml.includes("grid-template-columns: 22px minmax(0, 1fr);"),
    "array items should keep index indentation compact"
  );
  assert(
    inspectorHtml.includes(".field-fold { display: block; }"),
    "foldable fields should not reserve the field-name column below their summary row"
  );
  assert(
    inspectorHtml.includes(".field-fold-body { margin-top: 4px; padding-left: 4px;"),
    "foldable field bodies should use only a slight full-width indentation"
  );
  assert(
    inspectorScript.includes("function renderFoldableField"),
    "composite field values should render as one foldable field"
  );
  assert(
    inspectorScript.includes("anchors?.set(graphPathKey(path), summary);"),
    "foldable field anchors should stay on the summary row instead of the expanded body"
  );
  assert(
    inspectorScript.includes("if (isFoldableFieldValue(field.value))"),
    "renderField should route composite values to the foldable-field layout"
  );
  assert.deepStrictEqual(
    extension.__test.graphAnchorLocalBox(
      { left: 100, top: 200, width: 480, height: 300 },
      { left: 340, top: 500, width: 120, height: 30 },
      240,
      150
    ),
    { left: 120, top: 150, width: 60, height: 15 },
    "graph field anchors should be measured from rendered bounds relative to the node"
  );
  const visibleAnchors = new Map([
    ["drop", { id: "drop-anchor" }],
    ["drop.rewards", { id: "rewards-anchor" }]
  ]);
  assert.deepStrictEqual(
    extension.__test.bestGraphAnchor(visibleAnchors, ["drop", "rewards", "[0]", "item"]),
    { element: { id: "rewards-anchor" }, path: ["drop", "rewards"] },
    "graph field anchors should fall back to the nearest visible parent path"
  );
  assert.strictEqual(
    extension.__test.bestGraphAnchor(new Map(), ["drop", "rewards"]),
    undefined,
    "missing field anchors should fall back to the record anchor"
  );
  const hiddenChildAnchors = new Map([
    ["drop", { id: "drop-anchor", visible: true }],
    ["drop.rewards", { id: "rewards-anchor", visible: true }],
    ["drop.rewards.[0].item", { id: "item-anchor", visible: false }]
  ]);
  assert.deepStrictEqual(
    extension.__test.bestGraphAnchor(
      hiddenChildAnchors,
      ["drop", "rewards", "[0]", "item"],
      (element) => element.visible
    ),
    { element: { id: "rewards-anchor", visible: true }, path: ["drop", "rewards"] },
    "hidden nested anchors should be skipped so edges fall back to the nearest visible parent"
  );
  vscodeMock.panels.length = 0;
  const firstCfd = textDocument(path.join(os.tmpdir(), "first.cfd"), "a: Item {}\n", "cfd");
  const secondCfd = textDocument(path.join(os.tmpdir(), "second.cfd"), "b: Item {}\n", "cfd");
  vscodeMock.activeTextEditor = { document: firstCfd, viewColumn: 1 };
  const requestedUris = [];
  const inspectorController = new extension.__test.CfdInspectorController(
    {
      request: async (document) => {
        requestedUris.push(document.uri.toString());
        return {
          recordsInFile: [{ id: document.uri.toString(), uri: document.uri.toString(), key: path.basename(document.uri.fsPath), fields: [] }],
          references: [],
          graph: { canShow: false, records: [], references: [], hiddenIsolatedAnchors: [] }
        };
      }
    },
    { refreshDebounceMs: 0 }
  );
  const panel = await inspectorController.open(firstCfd);
  assert.strictEqual(vscodeMock.panels.length, 1, "CFD inspector should create one webview panel");
  assert.deepStrictEqual(
    panel.column,
    { viewColumn: extension.__test.vscodeViewColumn.Beside, preserveFocus: true },
    "CFD inspector should open beside without stealing focus from the current file"
  );
  assert.strictEqual(requestedUris.at(-1), firstCfd.uri.toString());

  await inspectorController.followEditor({ document: secondCfd, viewColumn: 1 });
  assert.strictEqual(vscodeMock.panels.length, 1, "opening another CFD file should not create another inspector panel");
  assert.strictEqual(requestedUris.at(-1), secondCfd.uri.toString());
  assert.deepStrictEqual(
    panel.revealCalls.at(-1),
    { viewColumn: extension.__test.vscodeViewColumn.Beside, revealOptions: true },
    "CFD inspector should reveal beside without taking focus when the active CFD file changes"
  );

  const reusedPanel = await inspectorController.open(firstCfd);
  assert.strictEqual(reusedPanel, panel, "CFD inspector command should reuse the existing panel");
  assert.strictEqual(vscodeMock.panels.length, 1, "opening another CFD file should not create another inspector panel");
  assert.strictEqual(requestedUris.at(-1), firstCfd.uri.toString());
  inspectorController.dispose();

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

  const completionSource = "type Item {}\ntype Holder {\n  item: Item;\n  @ref(\n}\n";
  const completionDocument = textDocument(path.join(os.tmpdir(), "completion.cft"), completionSource);
  extension.__test.vscodeWorkspace.textDocuments.push(completionDocument);
  const completionProvider = new extension.__test.CftCompletionProvider({
    request: async () => undefined
  });
  const refItems = await completionProvider.provideCompletionItems(
    completionDocument,
    completionDocument.positionAt(completionSource.indexOf("@ref(") + "@ref(".length)
  );
  assert(
    !refItems.some((item) => item.label === "Item"),
    "legacy @ref annotation context must not offer type completions"
  );
  extension.__test.vscodeWorkspace.textDocuments.pop();
}

function textDocument(filePath, text, languageId = "cft") {
  const uri = extension.__test.vscodeUriFile(filePath);
  const lines = text.split(/\r?\n/);
  return {
    uri,
    languageId,
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

function disposable() {
  return { dispose() {} };
}

function isIdentContinue(char) {
  return Boolean(char) && /[_\p{ID_Continue}]/u.test(char);
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
