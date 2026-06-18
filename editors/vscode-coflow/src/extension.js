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
  ["declaration", "reference", "path", "record", "schema"]
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
  const selector = [{ language: "cft" }, { language: "cfd" }];
  const diagnostics = new CftDiagnostics(context);
  const inspectorController = new CfdInspectorController(diagnostics);
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
    vscode.window.onDidChangeActiveTextEditor((editor) => inspectorController.followEditor(editor)),
    registerCfdInspectorCommand(inspectorController),
    inspectorController,
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

const CFD_INSPECTOR_VIEW_TYPE = "coflow.cfdInspector";

function registerCfdInspectorCommand(controller) {
  return vscode.commands.registerCommand("coflow.openCfdInspector", async (uri) => {
    const document = await cfdDocumentForInspector(uri);
    if (!document) {
      await vscode.window.showErrorMessage("Open a .cfd file to use the CFD inspector.");
      return undefined;
    }
    return controller.open(document);
  });
}

async function cfdDocumentForInspector(uri) {
  if (uri?.scheme === "file") {
    const open = vscode.workspace.textDocuments.find((document) =>
      document.uri.toString() === uri.toString()
    );
    if (open) {
      return isCfdDocument(open) ? open : undefined;
    }
    const document = await vscode.workspace.openTextDocument(uri);
    return isCfdDocument(document) ? document : undefined;
  }

  const document = vscode.window.activeTextEditor?.document;
  return isCfdDocument(document) ? document : undefined;
}

function isCfdDocument(document) {
  return document?.languageId === "cfd" && document.uri?.scheme === "file";
}

class CfdInspectorController {
  constructor(diagnostics, options = {}) {
    this.diagnostics = diagnostics;
    this.refreshDebounceMs = options.refreshDebounceMs ?? 120;
    this.session = undefined;
  }

  async open(document) {
    if (!isCfdDocument(document)) {
      await vscode.window.showErrorMessage("Open a .cfd file to use the CFD inspector.");
      return undefined;
    }

    const sourceViewColumn = sourceViewColumnForDocument(document);
    if (this.session) {
      this.session.panel.reveal(vscode.ViewColumn.Beside, true);
      await this.session.showDocument(document, sourceViewColumn);
      return this.session.panel;
    }

    const session = new CfdInspectorPanelSession(
      document,
      this.diagnostics,
      this.refreshDebounceMs,
      sourceViewColumn,
      () => {
        this.session = undefined;
      }
    );
    this.session = session;
    await session.refresh();
    return session.panel;
  }

  followEditor(editor) {
    const document = editor?.document;
    if (!this.session || !isCfdDocument(document)) {
      return undefined;
    }
    this.session.panel.reveal(vscode.ViewColumn.Beside, true);
    return this.session.showDocument(document, editor.viewColumn || vscode.ViewColumn.One);
  }

  dispose() {
    if (this.session) {
      this.session.dispose();
    }
    this.session = undefined;
  }
}

class CfdInspectorPanelSession {
  constructor(document, diagnostics, refreshDebounceMs, sourceViewColumn, onDispose) {
    this.document = document;
    this.diagnostics = diagnostics;
    this.refreshDebounceMs = refreshDebounceMs;
    this.sourceViewColumn = sourceViewColumn;
    this.onDispose = onDispose;
    this.disposables = [];
    this.refreshTimer = undefined;
    this.disposed = false;
    this.lastModel = undefined;
    this.panel = vscode.window.createWebviewPanel(
      CFD_INSPECTOR_VIEW_TYPE,
      `CFD Inspector: ${path.basename(document.uri.fsPath)}`,
      { viewColumn: vscode.ViewColumn.Beside, preserveFocus: true },
      {
        enableScripts: true,
        retainContextWhenHidden: true
      }
    );
    this.panel.webview.html = buildCfdInspectorHtml(undefined);
    this.disposables.push(
      this.panel.webview.onDidReceiveMessage((message) => this.onMessage(message)),
      vscode.workspace.onDidChangeTextDocument((event) => {
        if (event.document.uri.toString() === this.document.uri.toString()) {
          return this.scheduleRefresh();
        }
        return undefined;
      }),
      this.panel.onDidDispose(() => this.dispose(false))
    );
  }

  async showDocument(document, sourceViewColumn = sourceViewColumnForDocument(document)) {
    if (this.disposed || !isCfdDocument(document)) {
      return undefined;
    }
    const previousUri = this.document.uri.toString();
    this.document = document;
    this.sourceViewColumn = sourceViewColumn;
    this.panel.title = `CFD Inspector: ${path.basename(document.uri.fsPath)}`;
    if (previousUri === document.uri.toString()) {
      return this.refresh();
    }
    return this.refresh();
  }

  async refresh() {
    if (this.disposed) {
      return;
    }
    const model = await buildCfdInspectorModel(this.document, this.diagnostics);
    if (!model || this.disposed) {
      return;
    }
    this.lastModel = model;
    this.panel.webview.html = buildCfdInspectorHtml(model);
    await this.panel.webview.postMessage({ type: "model", model });
  }

  scheduleRefresh() {
    if (this.disposed) {
      return undefined;
    }
    if (this.refreshTimer) {
      clearTimeout(this.refreshTimer);
    }
    if (this.refreshDebounceMs <= 0) {
      return this.refresh();
    }
    this.refreshTimer = setTimeout(() => {
      this.refreshTimer = undefined;
      this.refresh();
    }, this.refreshDebounceMs);
    return undefined;
  }

  async onMessage(message) {
    if (message?.type === "jump") {
      await jumpToCfdInspectorLocation(message, this.sourceViewColumn);
    }
  }

  dispose(closePanel = true) {
    if (this.disposed) {
      return;
    }
    this.disposed = true;
    if (this.refreshTimer) {
      clearTimeout(this.refreshTimer);
      this.refreshTimer = undefined;
    }
    this.onDispose?.();
    for (const disposable of this.disposables.splice(0)) {
      disposable.dispose?.();
    }
    if (closePanel) {
      this.panel.dispose?.();
    }
  }
}

async function openCfdInspector(document, options = {}) {
  const controller = new CfdInspectorController(options.diagnostics, {
    refreshDebounceMs: options.refreshDebounceMs
  });
  return controller.open(document);
}

async function buildCfdInspectorModel(document, diagnostics) {
  return diagnostics.request(document, "coflow/inspectorModel", {
    textDocument: {
      uri: document.uri.toString()
    }
  });
}

function sourceViewColumnForDocument(document) {
  const editor = vscode.window.activeTextEditor;
  if (editor?.document?.uri?.toString() === document.uri.toString()) {
    return editor.viewColumn || vscode.ViewColumn.One;
  }
  return vscode.ViewColumn.One;
}

async function jumpToCfdInspectorLocation(message, viewColumn = vscode.ViewColumn.One) {
  const uri = lspUriToVsCode(message?.uri);
  if (!uri) {
    return;
  }
  const range = lspRangeToVsCode(message.range, true);
  await vscode.window.showTextDocument(uri, {
    viewColumn,
    selection: range,
    preview: false
  });
}

function computeGraphColumns(records, refs) {
  const recordById = new Map(records.map((record) => [record.id, record]));
  const graphRefs = refs.filter((ref) =>
    recordById.has(ref.sourceRecordId) && recordById.has(ref.targetRecordId)
  );
  const inbound = new Map(records.map((record) => [record.id, 0]));
  for (const ref of graphRefs) {
    inbound.set(ref.targetRecordId, (inbound.get(ref.targetRecordId) || 0) + 1);
  }

  const roots = records.filter((record) => !inbound.get(record.id));
  const depth = new Map();
  const queue = roots.length
    ? roots.map((record) => [record.id, 0])
    : records.map((record, index) => [record.id, index]);
  while (queue.length) {
    const [id, currentDepth] = queue.shift();
    if (!recordById.has(id)) {
      continue;
    }
    if (depth.has(id) && depth.get(id) <= currentDepth) {
      continue;
    }
    depth.set(id, currentDepth);
    for (const ref of graphRefs) {
      if (ref.sourceRecordId === id) {
        queue.push([ref.targetRecordId, currentDepth + 1]);
      }
    }
  }

  const cols = new Map();
  for (const record of records) {
    const recordDepth = depth.get(record.id) || 0;
    const col = cols.get(recordDepth) || [];
    col.push(record);
    cols.set(recordDepth, col);
  }

  for (const col of cols.values()) {
    col.sort(compareGraphRecords);
  }

  const sortedDepths = [...cols.keys()].sort((left, right) => left - right);
  let positions = graphColumnPositions(cols);
  for (const recordDepth of sortedDepths) {
    sortGraphColumn(cols.get(recordDepth), graphRefs, positions, recordDepth);
    positions = graphColumnPositions(cols);
  }

  return cols;
}

function sortGraphColumn(records, refs, positions, depth) {
  if (!records || records.length < 2) {
    return;
  }
  records.sort((left, right) => {
    const leftScore = graphNeighborAverage(left.id, refs, positions, depth);
    const rightScore = graphNeighborAverage(right.id, refs, positions, depth);
    if (leftScore !== undefined && rightScore !== undefined && leftScore !== rightScore) {
      return leftScore - rightScore;
    }
    if (leftScore !== undefined && rightScore === undefined) {
      return -1;
    }
    if (leftScore === undefined && rightScore !== undefined) {
      return 1;
    }
    return compareGraphRecords(left, right);
  });
}

function graphNeighborAverage(id, refs, positions, depth) {
  let total = 0;
  let count = 0;
  for (const ref of refs) {
    const neighborId = ref.targetRecordId === id ? ref.sourceRecordId : undefined;
    if (!neighborId) {
      continue;
    }
    const neighbor = positions.get(neighborId);
    if (!neighbor) {
      continue;
    }
    if (neighbor.depth >= depth) {
      continue;
    }
    total += neighbor.index;
    count += 1;
  }
  return count > 0 ? total / count : undefined;
}

function graphColumnPositions(cols) {
  const positions = new Map();
  for (const [depth, records] of cols.entries()) {
    records.forEach((record, index) => {
      positions.set(record.id, { depth, index });
    });
  }
  return positions;
}

function compareGraphRecords(left, right) {
  return String(left.key || left.id).localeCompare(String(right.key || right.id));
}

function graphPathKey(path) {
  return (path || []).join(".");
}

function bestGraphAnchor(anchors, path, isVisible = () => true) {
  for (let end = (path || []).length; end > 0; end -= 1) {
    const candidate = path.slice(0, end);
    const element = anchors.get(graphPathKey(candidate));
    if (element && isVisible(element)) {
      return { element, path: candidate };
    }
  }
  return undefined;
}

function graphAnchorLocalBox(nodeRect, anchorRect, nodeWidth, nodeHeight) {
  const scaleX = nodeRect.width ? nodeWidth / nodeRect.width : 1;
  const scaleY = nodeRect.height ? nodeHeight / nodeRect.height : 1;
  return {
    left: (anchorRect.left - nodeRect.left) * scaleX,
    top: (anchorRect.top - nodeRect.top) * scaleY,
    width: anchorRect.width * scaleX,
    height: anchorRect.height * scaleY
  };
}

function cfdInspectorGraphLayoutScript() {
  return [
    computeGraphColumns,
    sortGraphColumn,
    graphNeighborAverage,
    graphColumnPositions,
    compareGraphRecords,
    graphPathKey,
    bestGraphAnchor,
    graphAnchorLocalBox
  ].map((fn) => fn.toString()).join("\n\n");
}

function buildCfdInspectorHtml(model) {
  const nonce = randomNonce();
  const state = JSON.stringify(model || emptyCfdInspectorModel()).replace(/</g, "\\u003c");
  return `<!doctype html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; script-src 'nonce-${nonce}';">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <style>
    :root {
      color-scheme: light dark;
      --border:       var(--vscode-panel-border, #3c3c3c);
      --muted:        var(--vscode-descriptionForeground, #8b949e);
      --accent:       var(--vscode-textLink-foreground, #3794ff);
      --surface:      var(--vscode-editor-background, #1e1e1e);
      --surface-alt:  var(--vscode-sideBar-background, #252526);
      --surface-hi:   var(--vscode-list-hoverBackground, #2a2d2e);
      --text:         var(--vscode-editor-foreground, #d4d4d4);
      --badge-bg:     var(--vscode-badge-background, #4d4d4d);
      --badge-fg:     var(--vscode-badge-foreground, #cccccc);
      --r: 5px;
    }
    *, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }
    [hidden] { display: none !important; }
    html, body { height: 100%; }
    body {
      color: var(--text);
      background: var(--surface);
      font: 13px/1.5 var(--vscode-font-family, sans-serif);
      overflow: hidden;
    }
    button {
      border: 1px solid var(--border);
      color: var(--text);
      background: transparent;
      border-radius: var(--r);
      padding: 3px 10px;
      cursor: pointer;
      font: inherit;
      font-size: 12px;
      line-height: 1.4;
    }
    button:hover:not(:disabled) { background: var(--surface-hi); }
    button:disabled { opacity: 0.38; cursor: default; }
    button[aria-pressed="true"] { border-color: var(--accent); color: var(--accent); }

    /* ── Shell ───────────────────────────────────── */
    .root { display: flex; flex-direction: column; height: 100vh; }

    .toolbar {
      flex: 0 0 auto;
      display: flex;
      align-items: center;
      gap: 5px;
      padding: 6px 14px;
      border-bottom: 1px solid var(--border);
      background: var(--surface);
    }
    .tb-title { font-weight: 600; font-size: 13px; }
    .tb-sep   { width: 1px; height: 13px; background: var(--border); margin: 0 4px; }
    .tb-hint  { margin-left: auto; color: var(--muted); font-size: 11px; }

    .view      { flex: 1 1 0; overflow: auto; padding: 14px; }
    .view-graph { flex: 1 1 0; overflow: hidden; padding: 14px; display: flex; flex-direction: column; }

    .empty {
      display: flex; align-items: center; justify-content: center;
      padding: 40px 16px; color: var(--muted); font-size: 13px;
    }

    /* ── Shared card ─────────────────────────────── */
    /* Primary zone: header — key & type are the main identity  */
    /* Secondary zone: body — field rows are supporting detail   */
    .card {
      border: 1px solid var(--border);
      border-radius: var(--r);
      background: var(--surface-alt);
      overflow: hidden;
    }
    .card-header {
      display: flex;
      align-items: center;
      gap: 7px;
      padding: 9px 12px 8px;
      border-bottom: 1px solid var(--border);
    }
    /* key: largest, boldest — the primary label */
    .card-key {
      flex: 1 1 0;
      min-width: 0;
      font-weight: 700;
      font-size: 13px;
      color: var(--text);
      overflow-wrap: anywhere;
      text-align: left;
    }
    /* graph node key is larger */
    .node .card-key { font-size: 15px; }
    /* badge: secondary label — type identifier, smaller & dimmer */
    .card-badge {
      flex-shrink: 0;
      font-size: 10px;
      font-weight: 500;
      letter-spacing: 0.04em;
      padding: 1px 6px;
      border-radius: 3px;
      background: var(--badge-bg);
      color: var(--badge-fg);
    }
    .node .card-badge { font-size: 11px; padding: 2px 7px; }
    /* source link: tertiary, fades in on hover */
    .card-src {
      flex-shrink: 0;
      border-color: transparent;
      font-size: 11px;
      color: var(--muted);
      padding: 1px 4px;
      opacity: 0;
      transition: opacity 0.12s;
    }
    .card:hover .card-src { opacity: 1; }
    .card-src:hover        { color: var(--accent); opacity: 1; }

    /* body: secondary zone — slightly smaller, value-first */
    .card-body { padding: 8px 12px; display: grid; gap: 5px; }

    /* expand/collapse toggle at card foot */
    .card-toggle {
      display: block; width: 100%;
      padding: 4px 12px;
      font-size: 11px; color: var(--muted);
      border-top: 1px solid var(--border);
      border-left: 0; border-right: 0; border-bottom: 0; border-radius: 0;
      text-align: left; background: transparent;
    }
    .card-toggle:hover { color: var(--accent); background: var(--surface-hi); }
    .card-chevron {
      flex-shrink: 0;
      border: 0;
      padding: 0 4px 0 0;
      background: transparent;
      font-size: 9px;
      color: var(--muted);
      cursor: pointer;
      line-height: 1;
      transition: color 0.1s;
    }
    .card-chevron:hover { color: var(--accent); background: transparent; }

    /* ── Field rows (inside card-body and nested values) ── */
    .field {
      display: grid;
      grid-template-columns: minmax(44px, 30%) minmax(0, 1fr);
      gap: 6px;
      align-items: baseline;
    }
    .field-name {
      font-size: 11px;
      color: var(--accent);
      opacity: 0.88;
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
    }
    .field-value { min-width: 0; font-size: 12px; color: var(--text); overflow-wrap: anywhere; }
    .field-fold { display: block; }
    .field-fold-summary {
      cursor: pointer;
      list-style: none;
      display: grid;
      grid-template-columns: 10px minmax(44px, 30%) minmax(0, 1fr);
      gap: 4px;
      align-items: baseline;
    }
    .field-fold-summary::before { content: "▶"; font-size: 8px; color: var(--muted); }
    .field-fold[open] > .field-fold-summary::before { content: "▼"; }
    .field-fold-label { color: var(--muted); }
    .field-fold-body { margin-top: 4px; padding-left: 4px; border-left: 1px solid var(--border); display: grid; gap: 4px; }

    /* ── Inline jump button ── */
    .jump {
      border: 0; padding: 0;
      background: transparent; text-align: left;
      font: inherit; color: inherit; cursor: pointer;
    }
    .jump:hover { color: var(--accent); }

    /* ── Expandable value blocks ── */
    .value-card {
      border: 1px solid var(--border);
      border-radius: 4px;
      padding: 3px 4px;
      font-size: 12px;
    }
    .value-card > summary {
      cursor: pointer; list-style: none;
      display: flex; align-items: center; gap: 4px;
      color: var(--muted);
    }
    .value-card > summary::before         { content: "▶"; font-size: 8px; }
    details[open] > summary::before       { content: "▼"; }
    .nested-fields { margin-top: 4px; padding-left: 4px; border-left: 1px solid var(--border); display: grid; gap: 4px; }
    .array-item {
      display: grid; grid-template-columns: 22px minmax(0, 1fr);
      gap: 4px; margin-top: 4px;
    }
    .array-item-fold { display: block; margin-top: 4px; }
    .array-item-summary {
      cursor: pointer;
      list-style: none;
      display: grid;
      grid-template-columns: 10px 22px minmax(0, 1fr);
      gap: 4px;
      align-items: baseline;
    }
    .array-item-summary::before { content: "▶"; font-size: 8px; color: var(--muted); }
    .array-item-fold[open] > .array-item-summary::before { content: "▼"; }
    .array-item-body { margin-top: 4px; padding-left: 4px; border-left: 1px solid var(--border); display: grid; gap: 4px; }
    .idx-chip  { color: var(--muted); font-size: 11px; }
    .path-chip { color: var(--accent); font-size: 11px; }

    /* inline reference expansion */
    .ref-mini {
      margin-top: 5px;
      padding: 5px 9px;
      border-left: 2px solid var(--accent);
      background: var(--surface);
      border-radius: 0 4px 4px 0;
      display: flex; align-items: center; gap: 7px;
      cursor: pointer; width: 100%;
      text-align: left;
    }
    .ref-mini:hover .ref-key { color: var(--accent); }
    .ref-key { font-weight: 600; font-size: 12px; }

    /* ── Table view ──────────────────────────────── */
    .table-wrap {
      border: 1px solid var(--border);
      border-radius: var(--r);
      overflow: auto;
    }
    .cfd-table {
      border-collapse: collapse;
      width: max-content; min-width: 100%;
      background: var(--surface);
    }
    .cfd-table th, .cfd-table td {
      padding: 6px 10px;
      border-bottom: 1px solid var(--border);
      border-right: 1px solid var(--border);
      vertical-align: top;
      min-width: 120px; max-width: 280px;
    }
    .cfd-table thead th {
      position: sticky; top: 0; z-index: 2;
      background: var(--surface-alt);
      font-weight: 600; font-size: 12px; text-align: left;
    }
    .cfd-table .col-key {
      position: sticky; left: 0; z-index: 3;
      background: var(--surface-alt); min-width: 120px;
    }
    .cfd-table tbody td.col-key { background: var(--surface); z-index: 1; }
    .cell-empty { color: var(--muted); }

    /* ── Records view ────────────────────────────── */
    .records-list { display: flex; flex-direction: column; gap: 10px; }

    /* ── Graph view ──────────────────────────────── */
    .graph {
      flex: 1; position: relative;
      overflow: hidden;
      border: 1px solid var(--border); border-radius: var(--r);
      cursor: grab; user-select: none; touch-action: none;
      min-height: 160px;
    }
    .graph.dragging { cursor: grabbing; }
    .graph-canvas   { position: relative; transform-origin: 0 0; }

    svg.edges {
      position: absolute; inset: 0;
      overflow: visible; pointer-events: none;
      color: var(--accent);
    }
    .edge {
      stroke: currentColor; stroke-width: 1.5; fill: none;
      opacity: .6; pointer-events: stroke; cursor: pointer;
    }
    .edge:hover { opacity: 1; stroke-width: 2; }
    .edge-dot {
      fill: currentColor; opacity: .75;
      pointer-events: none;
    }

    /* graph nodes use shared .card, positioned absolutely */
    .node {
      position: absolute; width: 240px;
      user-select: text;
    }
    .node .card-src { opacity: 0.7; }
  </style>
</head>
<body>
  <div class="root" id="cfd-app"></div>
  <script nonce="${nonce}">
    const vscode = typeof acquireVsCodeApi === "function" ? acquireVsCodeApi() : undefined;
    let model = ${state};
    let mode = "table";
    const root = document.getElementById("cfd-app");

    ${cfdInspectorGraphLayoutScript()}

    window.addEventListener("message", (event) => {
      if (event.data?.type === "model") { model = event.data.model; render(); }
    });

    function postJump(target) {
      vscode?.postMessage({
        type: "jump",
        uri: target.uri || target.sourceUri || target.targetUri,
        range: target.range
      });
    }

    function render() {
      root.innerHTML = "";
      root.append(buildToolbar());
      if (mode === "graph" && model.graph?.canShow) root.append(graphView());
      else if (mode === "records")                  root.append(recordsView());
      else                                           root.append(tableView());
    }

    function buildToolbar() {
      const bar = el("div", "toolbar");
      bar.append(
        el("span", "tb-title", "CFD Inspector"),
        el("div", "tb-sep"),
        modeBtn("table",   "Table"),
        modeBtn("records", "Records"),
        modeBtn("graph",   "Graph", !model.graph?.canShow),
        el("span", "tb-hint", hintText())
      );
      return bar;
    }

    function modeBtn(value, label, disabled) {
      const b = el("button", "", label);
      b.type = "button";
      b.disabled = Boolean(disabled);
      b.setAttribute("aria-pressed", String(mode === value));
      b.addEventListener("click", () => { mode = value; render(); });
      return b;
    }

    function hintText() {
      const n = model.recordsInFile?.length || 0;
      const h = model.graph?.hiddenIsolatedAnchors?.length || 0;
      return h > 0 ? n + " records · " + h + " isolated" : n + " record" + (n === 1 ? "" : "s");
    }

    // ── Shared card component ─────────────────────────────────────
    // All cards start collapsed; chevron button toggles the body.
    // onToggle callback fires after each toggle (used by graph to re-layout).

    function buildCard(record, opts) {
      const onToggle = opts?.onToggle;
      const anchors = opts?.anchors;
      let expanded = false;

      const card = el("div", "card");

      // ── Header ──
      const header = el("div", "card-header");

      // Chevron: sole owner of the toggle action
      const chevron = el("button", "card-chevron jump", "▶");
      chevron.type = "button";
      chevron.title = "Expand";
      chevron.addEventListener("click", () => {
        expanded = !expanded;
        if (body) body.hidden = !expanded;
        chevron.textContent = expanded ? "▼" : "▶";
        chevron.title = expanded ? "Collapse" : "Expand";
        onToggle?.();
      });
      header.append(chevron);

      // Key: jump to source only
      const keyBtn = el("button", "card-key jump", record.key);
      keyBtn.type = "button";
      keyBtn.addEventListener("click", () => postJump(record));
      header.append(keyBtn);

      if (record.type) header.append(el("span", "card-badge", record.type));

      const srcBtn = el("button", "card-src jump", "↗");
      srcBtn.type = "button"; srcBtn.title = "Open source";
      srcBtn.addEventListener("click", () => postJump(record));
      header.append(srcBtn);

      card.append(header);

      // ── Body (collapsed by default) ──
      const allFields = record.fields || [];
      let body = null;
      if (allFields.length) {
        body = el("div", "card-body");
        body.hidden = true;
        for (const f of allFields) body.append(renderField(record, f, [f.name], anchors));
        card.append(body);
      }

      return card;
    }

    // ── Table view ────────────────────────────────────────────────
    function tableView() {
      const view = el("div", "view");
      const records = model.recordsInFile || [];
      if (!records.length) { view.append(el("div", "empty", "No records in this file.")); return view; }
      const wrap = el("div", "table-wrap");
      const table = el("table", "cfd-table");
      const cols = tableColumns(records);
      const thead = document.createElement("thead");
      const hrow = document.createElement("tr");
      for (const c of cols) {
        const th = document.createElement("th");
        th.textContent = c.label;
        if (c.kind === "key") th.classList.add("col-key");
        hrow.append(th);
      }
      thead.append(hrow);
      const tbody = document.createElement("tbody");
      for (const record of records) {
        const row = document.createElement("tr");
        for (const c of cols) {
          const td = document.createElement("td");
          if (c.kind === "key") {
            td.classList.add("col-key");
            const btn = el("button", "jump", record.key);
            btn.type = "button";
            btn.addEventListener("click", () => postJump(record));
            td.append(btn);
          } else if (c.kind === "type") {
            td.textContent = record.type || "";
          } else {
            const f = (record.fields || []).find((x) => x.name === c.name);
            td.append(f ? renderValue(record, f.value, [f.name]) : el("span", "cell-empty", "—"));
          }
          row.append(td);
        }
        tbody.append(row);
      }
      table.append(thead, tbody);
      wrap.append(table);
      view.append(wrap);
      return view;
    }

    // ── Records view ──────────────────────────────────────────────
    function recordsView() {
      const view = el("div", "view");
      const records = model.recordsInFile || [];
      if (!records.length) { view.append(el("div", "empty", "No records in this file.")); return view; }
      const list = el("div", "records-list");
      for (const r of records) list.append(buildCard(r));
      view.append(list);
      return view;
    }

    // ── Graph view ────────────────────────────────────────────────
    const NODE_W = 240;
    const COL_GAP = 80;
    const ROW_GAP = 14;
    const CANVAS_PAD = 24;

    function graphView() {
      const view = el("div", "view-graph");
      const graph = el("div", "graph");
      const canvas = el("div", "graph-canvas");

      const graphRefs = model.graph?.references || [];
      const graphRecords = model.graph?.records || [];

      // Depth-based column layout
      const colMap = computeGraphColumns(graphRecords, graphRefs);
      const nodeInfos = [];  // { element, record, depth, x, y }
      const edgeInfos = [];  // { element (path), sourceId, targetId }

      // Build SVG with arrowhead marker
      const svg = svgEl("svg");
      svg.classList.add("edges");
      const defs = svgEl("defs");
      const marker = svgEl("marker");
      marker.id = "arw";
      marker.setAttribute("markerWidth", "9");
      marker.setAttribute("markerHeight", "7");
      marker.setAttribute("refX", "8");
      marker.setAttribute("refY", "3.5");
      marker.setAttribute("orient", "auto");
      marker.setAttribute("markerUnits", "strokeWidth");
      const arrowPoly = svgEl("polygon");
      arrowPoly.setAttribute("points", "0 0, 9 3.5, 0 7");
      arrowPoly.setAttribute("fill", "currentColor");
      arrowPoly.setAttribute("opacity", "0.7");
      marker.append(arrowPoly);
      defs.append(marker);
      svg.append(defs);
      canvas.append(svg);

      // Place nodes column by column
      let colX = CANVAS_PAD;
      for (const [, records] of [...colMap.entries()].sort((a, b) => a[0] - b[0])) {
        for (const record of records) {
          const nodeEl = el("div", "node");
          const fieldAnchors = new Map();
          nodeEl.style.left = colX + "px";
          nodeEl.style.top = "0px";
          nodeEl.append(buildCard(record, {
            anchors: fieldAnchors,
            compact: true,
            onToggle: () => requestAnimationFrame(reLayout)
          }));
          canvas.append(nodeEl);
          nodeInfos.push({ element: nodeEl, fieldAnchors, record, x: colX, y: 0 });
        }
        colX += NODE_W + COL_GAP;
      }

      // Build edge paths + endpoint dots
      for (const ref of graphRefs) {
        const pathEl = svgEl("path");
        pathEl.classList.add("edge");
        pathEl.setAttribute("marker-end", "url(#arw)");
        pathEl.addEventListener("click", () => postJump({ uri: ref.sourceUri, range: ref.range }));
        svg.append(pathEl);
        // source dot and target dot (drawn on top of paths)
        const dotSrc = svgEl("circle");
        dotSrc.classList.add("edge-dot");
        dotSrc.setAttribute("r", "3.5");
        const dotTgt = svgEl("circle");
        dotTgt.classList.add("edge-dot");
        dotTgt.setAttribute("r", "3");
        svg.append(dotSrc, dotTgt);
        edgeInfos.push({
          element: pathEl,
          dotSrc,
          dotTgt,
          sourceId: ref.sourceRecordId,
          sourcePath: ref.sourcePath || [],
          targetId: ref.targetRecordId,
          targetPath: ref.targetPath || []
        });
      }

      function reLayout() {
        // Group nodes by column X
        const byX = new Map();
        for (const ni of nodeInfos) {
          const arr = byX.get(ni.x) || [];
          arr.push(ni);
          byX.set(ni.x, arr);
        }
        // Stack nodes vertically within each column
        let maxBottom = 0;
        for (const col of byX.values()) {
          let y = CANVAS_PAD;
          for (const ni of col) {
            ni.y = y;
            ni.element.style.top = y + "px";
            y += ni.element.offsetHeight + ROW_GAP;
          }
          maxBottom = Math.max(maxBottom, y - ROW_GAP);
        }
        const maxRight = colX - COL_GAP + CANVAS_PAD;
        // Resize canvas
        const W = maxRight;
        const H = maxBottom + CANVAS_PAD;
        canvas.style.width  = W + "px";
        canvas.style.height = H + "px";
        svg.style.width  = W + "px";
        svg.style.height = H + "px";
        // Redraw edges + dots
        const posMap = new Map(nodeInfos.map((ni) => [ni.record.id, ni]));
        for (const ei of edgeInfos) {
          const src = posMap.get(ei.sourceId);
          const tgt = posMap.get(ei.targetId);
          if (!src || !tgt) {
            ei.element.setAttribute("d", "");
            ei.dotSrc.setAttribute("cx", "-999"); ei.dotTgt.setAttribute("cx", "-999");
            continue;
          }
          const sourceAnchor = graphAnchorPoint(src, ei.sourcePath, "source");
          const targetAnchor = graphAnchorPoint(tgt, ei.targetPath, "target");
          ei.element.setAttribute("d", cubicEdgePath(sourceAnchor.x, sourceAnchor.y, targetAnchor.x, targetAnchor.y));
          ei.dotSrc.setAttribute("cx", String(sourceAnchor.x)); ei.dotSrc.setAttribute("cy", String(sourceAnchor.y));
          ei.dotTgt.setAttribute("cx", String(targetAnchor.x)); ei.dotTgt.setAttribute("cy", String(targetAnchor.y));
        }
      }

      // React to node size changes (card expand/collapse)
      const ro = new ResizeObserver(() => requestAnimationFrame(reLayout));
      for (const ni of nodeInfos) ro.observe(ni.element);

      graph.append(canvas);
      panZoomGraph(graph, canvas);
      view.append(graph);
      requestAnimationFrame(reLayout);
      return view;
    }

    function cubicEdgePath(sx, sy, tx, ty) {
      const dx = Math.max(50, Math.abs(tx - sx) * 0.45);
      return "M " + sx + " " + sy +
             " C " + (sx + dx) + " " + sy +
             " " + (tx - dx) + " " + ty +
             " " + tx + " " + ty;
    }

    function panZoomGraph(graph, canvas) {
      const s = { scale: 1, x: 0, y: 0, dragging: false, sx: 0, sy: 0 };
      const apply = () => {
        canvas.style.transform = "translate(" + s.x + "px," + s.y + "px) scale(" + s.scale + ")";
      };
      graph.addEventListener("wheel", (e) => {
        e.preventDefault();
        const next = Math.min(2.5, Math.max(0.3, s.scale * (e.deltaY > 0 ? 0.9 : 1.1)));
        const r = graph.getBoundingClientRect();
        const mx = e.clientX - r.left, my = e.clientY - r.top;
        s.x = mx - ((mx - s.x) / s.scale) * next;
        s.y = my - ((my - s.y) / s.scale) * next;
        s.scale = next; apply();
      }, { passive: false });
      graph.addEventListener("pointerdown", (e) => {
        if (e.target.closest(".card")) return;
        s.dragging = true; s.sx = e.clientX - s.x; s.sy = e.clientY - s.y;
        graph.classList.add("dragging"); graph.setPointerCapture(e.pointerId);
      });
      graph.addEventListener("pointermove", (e) => {
        if (!s.dragging) return;
        s.x = e.clientX - s.sx; s.y = e.clientY - s.sy; apply();
      });
      graph.addEventListener("pointerup", (e) => {
        s.dragging = false; graph.classList.remove("dragging"); graph.releasePointerCapture(e.pointerId);
      });
      apply();
    }

    // ── Field & value renderers ───────────────────────────────────
    function graphAnchorPoint(nodeInfo, path, side) {
      const anchor = bestGraphAnchor(nodeInfo.fieldAnchors, path, isGraphAnchorVisible)?.element;
      if (!anchor) {
        return {
          x: side === "source" ? nodeInfo.x + NODE_W : nodeInfo.x,
          y: nodeInfo.y + nodeInfo.element.offsetHeight / 2
        };
      }
      const box = graphAnchorLocalBox(
        nodeInfo.element.getBoundingClientRect(),
        anchor.getBoundingClientRect(),
        nodeInfo.element.offsetWidth || NODE_W,
        nodeInfo.element.offsetHeight
      );
      return {
        x: nodeInfo.x + (side === "source" ? box.left + box.width : box.left),
        y: nodeInfo.y + box.top + box.height / 2
      };
    }

    function isGraphAnchorVisible(anchor) {
      return anchor.offsetParent !== null;
    }

    function renderField(record, field, path, anchors) {
      if (isFoldableFieldValue(field.value)) {
        return renderFoldableField(record, field, path, anchors);
      }
      const row = el("div", "field");
      anchors?.set(graphPathKey(path), row);
      const name = el("button", "field-name jump", field.name);
      name.type = "button";
      name.addEventListener("click", () => postJump({ uri: record.uri, range: field.range }));
      const val = el("div", "field-value");
      val.append(renderValue(record, field.value, path, anchors));
      row.append(name, val);
      return row;
    }

    function renderFoldableField(record, field, path, anchors) {
      const details = el("details", "field-fold");
      const summary = el("summary", "field-fold-summary");
      anchors?.set(graphPathKey(path), summary);
      const name = el("button", "field-name jump", field.name);
      name.type = "button";
      name.addEventListener("click", (event) => {
        event.preventDefault();
        event.stopPropagation();
        postJump({ uri: record.uri, range: field.range });
      });
      summary.append(name, el("span", "field-fold-label", foldableValueLabel(field.value)));
      details.append(summary);
      details.append(renderFoldableBody(record, field.value, path, anchors, "field-fold-body"));
      return details;
    }

    function renderFoldableBody(record, value, path, anchors, className) {
      const body = el("div", className);
      if (!value) return body;
      if (value.kind === "block") {
        for (const f of value.fields || []) body.append(renderField(record, f, path.concat(f.name), anchors));
        return body;
      }
      if (value.kind === "array") {
        (value.items || []).forEach((item, i) => {
          const itemPath = path.concat("[" + i + "]");
          if (isFoldableFieldValue(item)) {
            body.append(renderFoldableArrayItem(record, item, itemPath, i, anchors));
          } else {
            const row = el("div", "array-item");
            row.append(el("div", "idx-chip", "[" + i + "]"), renderValue(record, item, itemPath, anchors));
            body.append(row);
          }
        });
        return body;
      }
      if (value.kind === "spread") {
        body.append(renderValue(record, value.value, path.concat("..."), anchors));
        return body;
      }
      body.append(renderValue(record, value, path, anchors));
      return body;
    }

    function renderFoldableArrayItem(record, value, path, index, anchors) {
      const details = el("details", "array-item-fold");
      const summary = el("summary", "array-item-summary");
      anchors?.set(graphPathKey(path), summary);
      summary.append(el("span", "idx-chip", "[" + index + "]"), el("span", "field-fold-label", foldableValueLabel(value)));
      details.append(summary);
      details.append(renderFoldableBody(record, value, path, anchors, "array-item-body"));
      return details;
    }

    function isFoldableFieldValue(value) {
      return Boolean(value && (value.kind === "block" || value.kind === "array" || value.kind === "spread"));
    }

    function foldableValueLabel(value) {
      if (!value) return "";
      if (value.kind === "array") return "[" + (value.items?.length || 0) + "]";
      if (value.kind === "block") return value.type ? value.type + " {…}" : "{…}";
      if (value.kind === "spread") return "…spread";
      return valueText(value);
    }

    function renderValue(record, value, path, anchors) {
      if (!value) return document.createTextNode("");
      if (value.kind === "block") {
        const d = el("details", "value-card");
        d.append(el("summary", "", value.type ? value.type + " {…}" : "{…}"));
        const nf = el("div", "nested-fields");
        for (const f of value.fields || []) nf.append(renderField(record, f, path.concat(f.name), anchors));
        d.append(nf);
        return d;
      }
      if (value.kind === "array") {
        const d = el("details", "value-card");
        d.append(el("summary", "", "[" + (value.items?.length || 0) + "]"));
        (value.items || []).forEach((item, i) => {
          const row = el("div", "array-item");
          row.append(el("div", "idx-chip", "[" + i + "]"), renderValue(record, item, path.concat("[" + i + "]"), anchors));
          d.append(row);
        });
        return d;
      }
      if (value.kind === "spread") {
        const d = el("details", "value-card");
        d.append(el("summary", "", "…spread"));
        d.append(renderValue(record, value.value, path.concat("..."), anchors));
        return d;
      }
      if (value.kind === "ref") return renderRefValue(record, value, path);
      const btn = el("button", "jump", valueText(value));
      btn.type = "button";
      btn.addEventListener("click", () => postJump({ uri: record.uri, range: value.range }));
      return btn;
    }

    function renderRefValue(record, value, path) {
      const d = el("details", "value-card");
      const summary = el("summary", "", valueText(value));
      summary.addEventListener("dblclick", () => postJump({ uri: record.uri, range: value.range }));
      d.append(summary);
      const edge = referenceForPath(record, path);
      if (edge) {
        const target = recordForId(edge.targetRecordId);
        const mini = el("button", "ref-mini");
        mini.type = "button";
        mini.append(
          el("span", "ref-key",   target ? target.key  : (edge.targetRecordKey || "?")),
          el("span", "card-badge", target ? target.type : (edge.targetRecordType || ""))
        );
        if (edge.targetPath?.length) mini.append(el("span", "path-chip", edge.targetPath.join(".")));
        mini.addEventListener("click", () => postJump(target || { uri: edge.targetUri, range: edge.range }));
        d.append(mini);
      }
      return d;
    }

    function referenceForPath(record, path) {
      return (model.references || []).find(
        (e) => e.sourceRecordId === record.id && samePath(e.sourcePath || [], path)
      );
    }

    function recordForId(id) {
      return (model.graph?.records || []).find((r) => r.id === id)
          || (model.recordsInFile  || []).find((r) => r.id === id);
    }

    function samePath(a, b) {
      return a.length === b.length && a.every((v, i) => v === b[i]);
    }

    function valueText(v) {
      if (!v) return "";
      if (v.kind === "ref")    return (v.refKind === "typed" && v.type ? "@" + v.type + "." : "&") + v.key + (v.path?.length ? "." + v.path.join(".") : "");
      if (v.kind === "array")  return "[" + (v.items || []).map(valueText).join(", ") + "]";
      if (v.kind === "block")  return "{ " + (v.fields || []).map((f) => f.name + ": " + valueText(f.value)).join(", ") + " }";
      if (v.kind === "spread") return "…" + valueText(v.value);
      return v.text ?? v.kind;
    }

    function tableColumns(records) {
      const names = [], seen = new Set();
      for (const r of records)
        for (const f of r.fields || [])
          if (!seen.has(f.name)) { seen.add(f.name); names.push(f.name); }
      return [
        { kind: "key",  label: "key"  },
        { kind: "type", label: "type" },
        ...names.map((n) => ({ kind: "field", name: n, label: n }))
      ];
    }

    function el(tag, cls, text) {
      const n = document.createElement(tag);
      if (cls)             n.className   = cls;
      if (text !== undefined) n.textContent = text;
      return n;
    }

    function svgEl(tag) {
      return document.createElementNS("http://www.w3.org/2000/svg", tag);
    }

    render();
  </script>
</body>
</html>`;
}

function emptyCfdInspectorModel() {
  return {
    recordsInFile: [],
    references: [],
    graph: {
      canShow: false,
      records: [],
      references: [],
      hiddenIsolatedAnchors: []
    }
  };
}

function randomNonce() {
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
  let value = "";
  for (let index = 0; index < 24; index += 1) {
    value += alphabet[Math.floor(Math.random() * alphabet.length)];
  }
  return value;
}

class CftDiagnostics {
  constructor(context) {
    this.context = context;
    this.collection = vscode.languages.createDiagnosticCollection("cft");
    this.timers = new Map();
    this.sessions = new Map();
    this.documentSessions = new Map();

    const configWatcher = vscode.workspace.createFileSystemWatcher("**/coflow.{yaml,yml}");
    configWatcher.onDidChange((uri) => this.restartSessionsForProject(uri));
    configWatcher.onDidCreate((uri) => this.restartSessionsForProject(uri));
    configWatcher.onDidDelete((uri) => this.restartSessionsForProject(uri));

    context.subscriptions.push(
      this.collection,
      configWatcher,
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
      session = new CftLspSession(command, args, cwd, this.collection);
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
      rawType
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

  // Collect CFT paths for this document's project first, so we can filter
  // open documents to only those belonging to the same coflow.yaml project.
  const projectCftPaths = new Set(await collectCftPaths(document.uri));

  for (const openDocument of vscode.workspace.textDocuments) {
    if (
      openDocument.languageId === "cft" &&
      openDocument.uri.scheme === "file" &&
      projectCftPaths.has(normalizePath(openDocument.uri.fsPath))
    ) {
      openDocuments.set(openDocument.uri.toString(), openDocument);
    }
  }

  for (const openDocument of openDocuments.values()) {
    symbolsByUri.set(openDocument.uri.toString(), {
      document: openDocument,
      symbols: collectSymbols(openDocument)
    });
  }

  for (const filePath of projectCftPaths) {
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
    CfdInspectorController,
    CftLspSession,
    collectConfiguredSchemaPaths,
    buildCfdInspectorHtml,
    bestGraphAnchor,
    graphAnchorLocalBox,
    computeGraphColumns,
    openCfdInspector,
    semanticTokensLegend: CFT_SEMANTIC_TOKENS_LEGEND,
    localDefinitionLocations,
    schemaEntriesFromCoflowConfigText,
    lspDefinitionLocations,
    vscodeApi: vscode,
    vscodePosition: vscode.Position,
    vscodeRange: vscode.Range,
    vscodeUriFile: vscode.Uri.file,
    vscodeViewColumn: vscode.ViewColumn,
    vscodeWorkspace: vscode.workspace
  }
};
