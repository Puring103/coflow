const vscode = require("vscode");
const cp = require("child_process");
const fs = require("fs");
const path = require("path");

const IDENT = "[A-Za-z_][A-Za-z0-9_]*";

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
    detail: "type, enum, field, or variant annotation",
    documentation: "Attach a human-readable display name."
  },
  {
    label: "@deprecated",
    insertText: "@deprecated",
    detail: "type, enum, field, or variant annotation",
    documentation: "Mark the target as deprecated for generated code."
  }
];

function activate(context) {
  const selector = { language: "cft" };
  const diagnostics = new CftDiagnostics(context);
  context.subscriptions.push(
    vscode.languages.registerCompletionItemProvider(
      selector,
      new CftCompletionProvider(),
      ".",
      "@",
      ":",
      " ",
      "("
    ),
    vscode.languages.registerHoverProvider(selector, new CftHoverProvider()),
    vscode.languages.registerDocumentSymbolProvider(selector, new CftDocumentSymbolProvider()),
    vscode.languages.registerDefinitionProvider(selector, new CftDefinitionProvider()),
    diagnostics
  );
}

function deactivate() {}

class CftCompletionProvider {
  provideCompletionItems(document, position) {
    const symbols = collectSymbols(document);
    const linePrefix = document.lineAt(position).text.slice(0, position.character);

    if (/@[A-Za-z0-9_]*$/.test(linePrefix)) {
      const range = rangeFromLineMatch(document, position, /@[A-Za-z0-9_]*$/);
      return ANNOTATIONS.map((annotation) => annotationItem(annotation, range));
    }

    const dot = linePrefix.match(new RegExp(`(${IDENT})\\.\\s*(${IDENT})?$`));
    if (dot) {
      const target = dot[1];
      const typed = dot[2] || "";
      const range = new vscode.Range(
        position.line,
        position.character - typed.length,
        position.line,
        position.character
      );
      const variants = symbols.enumVariants.get(target);
      if (variants) {
        return variants.map((variant) =>
          simpleItem(variant.name, vscode.CompletionItemKind.EnumMember, `${target} variant`, range)
        );
      }
      return dotFieldCompletions(symbols, document.offsetAt(position), range);
    }

    if (/\bis\s+[A-Za-z0-9_]*$/.test(linePrefix)) {
      return [
        ...symbols.types.map((type) =>
          simpleItem(type.name, vscode.CompletionItemKind.Class, "CFT type")
        ),
        simpleItem("null", vscode.CompletionItemKind.Keyword, "Null predicate")
      ];
    }

    if (isTypeReferenceContext(linePrefix)) {
      return typeReferenceItems(symbols);
    }

    const offset = document.offsetAt(position);
    const currentType = currentTypeAt(symbols, offset);
    const items = [
      ...KEYWORDS.map(([label, documentation]) =>
        simpleItem(label, vscode.CompletionItemKind.Keyword, "CFT keyword", undefined, documentation)
      ),
      ...PRIMITIVE_TYPES.map(([label, documentation]) =>
        simpleItem(label, vscode.CompletionItemKind.Keyword, "Primitive type", undefined, documentation)
      ),
      ...LITERALS.map(([label, documentation]) =>
        simpleItem(label, vscode.CompletionItemKind.Keyword, "CFT literal", undefined, documentation)
      ),
      ...functionItems(),
      ...symbolItems(symbols)
    ];

    if (currentType) {
      for (const field of currentType.fields) {
        items.push(simpleItem(field.name, vscode.CompletionItemKind.Field, `${currentType.name} field`));
      }
    }

    return items;
  }
}

class CftHoverProvider {
  provideHover(document, position) {
    const range =
      document.getWordRangeAtPosition(position, /@[A-Za-z_][A-Za-z0-9_]*/) ||
      document.getWordRangeAtPosition(position, /[A-Za-z_][A-Za-z0-9_]*/);
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
  provideDocumentSymbols(document) {
    const symbols = collectSymbols(document);
    const output = [];

    for (const item of symbols.consts) {
      output.push(documentSymbol(document, item, vscode.SymbolKind.Constant));
    }

    for (const item of symbols.enums) {
      const symbol = documentSymbol(document, item, vscode.SymbolKind.Enum);
      for (const variant of symbols.enumVariants.get(item.name) || []) {
        symbol.children.push(documentSymbol(document, variant, vscode.SymbolKind.EnumMember));
      }
      output.push(symbol);
    }

    for (const item of symbols.types) {
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
  async provideDefinition(document, position) {
    const range = document.getWordRangeAtPosition(position, /[A-Za-z_][A-Za-z0-9_]*/);
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
}

class CftDiagnostics {
  constructor(context) {
    this.context = context;
    this.collection = vscode.languages.createDiagnosticCollection("cft");
    this.timers = new Map();

    context.subscriptions.push(
      this.collection,
      vscode.workspace.onDidOpenTextDocument((document) => this.schedule(document)),
      vscode.workspace.onDidChangeTextDocument((event) => this.schedule(event.document)),
      vscode.workspace.onDidSaveTextDocument((document) => this.schedule(document)),
      vscode.workspace.onDidCloseTextDocument((document) => {
        if (document.languageId === "cft") {
          this.collection.delete(document.uri);
        }
      }),
      vscode.workspace.onDidChangeConfiguration((event) => {
        if (event.affectsConfiguration("coflowCft.diagnostics")) {
          this.validateAllOpenDocuments();
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
    this.collection.dispose();
  }

  validateAllOpenDocuments() {
    for (const document of vscode.workspace.textDocuments) {
      this.schedule(document);
    }
  }

  schedule(document) {
    if (document.languageId !== "cft" || document.uri.scheme !== "file") {
      return;
    }

    const config = vscode.workspace.getConfiguration("coflowCft.diagnostics", document.uri);
    if (!config.get("enabled", true)) {
      this.collection.delete(document.uri);
      return;
    }

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
        this.validate(document).catch((error) => {
          const diagnostic = new vscode.Diagnostic(
            new vscode.Range(0, 0, 0, 1),
            `CFT diagnostics failed: ${error.message || error}`,
            vscode.DiagnosticSeverity.Warning
          );
          diagnostic.source = "cft";
          this.collection.set(document.uri, [diagnostic]);
        });
      }, debounceMs)
    );
  }

  async validate(document) {
    const documentVersion = document.version;
    const config = vscode.workspace.getConfiguration("coflowCft.diagnostics", document.uri);
    const command = config.get("command", "cargo");
    const baseArgs = config.get("args", [
      "run",
      "--quiet",
      "-p",
      "coflow-cft-cli",
      "--",
      "diagnostics"
    ]);
    const cwd = findDiagnosticsCwd(document.uri.fsPath, this.context.extensionPath);
    const paths = await collectCftPaths(document.uri);
    const currentPath = normalizePath(document.uri.fsPath);
    if (!paths.includes(currentPath)) {
      paths.push(currentPath);
    }

    const args = [...baseArgs, "--stdin-path", currentPath, ...paths];
    const result = await runDiagnosticsCommand(command, args, cwd, document.getText());
    if (document.version !== documentVersion) {
      return;
    }
    const output = parseDiagnosticsOutput(result.stdout);
    const byUri = new Map();

    for (const raw of output.diagnostics || []) {
      const uri = vscode.Uri.file(raw.path);
      const diagnostic = toVsCodeDiagnostic(raw);
      const key = uri.toString();
      const items = byUri.get(key) || [];
      items.push(diagnostic);
      byUri.set(key, items);
    }

    const touchedUris = new Set([document.uri.toString()]);
    for (const cftPath of paths) {
      touchedUris.add(vscode.Uri.file(cftPath).toString());
    }

    for (const uriString of touchedUris) {
      const uri = vscode.Uri.parse(uriString);
      this.collection.set(uri, byUri.get(uriString) || []);
    }
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

function symbolItems(symbols) {
  const items = [];
  for (const type of symbols.types) {
    items.push(simpleItem(type.name, vscode.CompletionItemKind.Class, "CFT type"));
  }
  for (const enumDef of symbols.enums) {
    items.push(simpleItem(enumDef.name, vscode.CompletionItemKind.Enum, "CFT enum"));
    for (const variant of symbols.enumVariants.get(enumDef.name) || []) {
      const item = simpleItem(
        `${enumDef.name}.${variant.name}`,
        vscode.CompletionItemKind.EnumMember,
        "CFT enum variant"
      );
      item.insertText = `${enumDef.name}.${variant.name}`;
      items.push(item);
    }
  }
  for (const constant of symbols.consts) {
    items.push(simpleItem(constant.name, vscode.CompletionItemKind.Constant, "CFT constant"));
  }
  return items;
}

function typeReferenceItems(symbols) {
  return [
    ...PRIMITIVE_TYPES.map(([label, documentation]) =>
      simpleItem(label, vscode.CompletionItemKind.Keyword, "Primitive type", undefined, documentation)
    ),
    ...symbols.types.map((type) => simpleItem(type.name, vscode.CompletionItemKind.Class, "CFT type")),
    ...symbols.enums.map((enumDef) => simpleItem(enumDef.name, vscode.CompletionItemKind.Enum, "CFT enum"))
  ];
}

function dotFieldCompletions(symbols, offset, range) {
  const items = [
    simpleItem("key", vscode.CompletionItemKind.Field, "Dict entry key", range),
    simpleItem("value", vscode.CompletionItemKind.Field, "Dict entry value", range)
  ];
  const currentType = currentTypeAt(symbols, offset);
  if (currentType) {
    for (const field of currentType.fields) {
      items.push(simpleItem(field.name, vscode.CompletionItemKind.Field, `${currentType.name} field`, range));
    }
  }
  return items;
}

function isTypeReferenceContext(linePrefix) {
  return /:\s*[\[{A-Za-z0-9_]*$/.test(linePrefix) || /@\s*ref\s*\(\s*[A-Za-z0-9_]*$/.test(linePrefix);
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

  for (const match of masked.matchAll(new RegExp(`\\bconst\\s+(${IDENT})\\b`, "g"))) {
    const name = match[1];
    const start = match.index + match[0].lastIndexOf(name);
    const end = start + name.length;
    consts.push({ name, start, end, uri: document.uri });
  }

  const enumRegex = new RegExp(`\\benum\\s+(${IDENT})\\b`, "g");
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

  const typeRegex = new RegExp(`\\b(?:(?:abstract|sealed)\\s+)*type\\s+(${IDENT})\\b`, "g");
  for (const match of masked.matchAll(typeRegex)) {
    const name = match[1];
    const nameStart = match.index + match[0].lastIndexOf(name);
    const open = masked.indexOf("{", match.index + match[0].length);
    const close = open >= 0 ? findMatchingBrace(masked, open) : -1;
    const end = close >= 0 ? close + 1 : nameStart + name.length;
    const fields = open >= 0 && close >= 0 ? parseFields(masked, open + 1, close) : [];
    types.push({ name, start: nameStart, end, fields, uri: document.uri });
  }

  return { types, enums, consts, enumVariants };
}

function parseEnumVariants(masked, bodyStart, bodyEnd, uri) {
  const body = masked.slice(bodyStart, bodyEnd).replace(/@[A-Za-z_][A-Za-z0-9_]*(?:\([^)]*\))?/g, " ");
  const variants = [];
  const variantRegex = new RegExp(`(?:^|,)\\s*(${IDENT})\\b`, "g");
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
    "g"
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
      typeName: namedTypeFromTypeRef(rawType),
      rawType,
      refTarget: refTargetFromAnnotations(annotations)
    });
  }
  return fields;
}

function namedTypeFromTypeRef(rawType) {
  const text = rawType.trim().replace(/\?$/, "").trim();
  const named = text.match(new RegExp(`^(${IDENT})$`));
  return named ? named[1] : undefined;
}

function refTargetFromAnnotations(annotations) {
  const match = annotations.match(new RegExp(`@ref\\s*\\(\\s*(${IDENT})\\s*\\)`));
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
    } else if (char === "/" && chars[index + 1] === "/") {
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
  let typeName = typeOfNameAt(workspace, document, position, root);
  if (!typeName) {
    return [];
  }

  let field;
  for (const part of chain.slice(1)) {
    field = fieldByType(workspace, typeName, part.name);
    if (!field) {
      return [];
    }
    typeName = field.item.refTarget || field.item.typeName;
  }

  return field ? [locationForItem(field.document, field.item)] : [];
}

function typeOfNameAt(workspace, document, position, name) {
  const entry = workspace.documents.get(document.uri.toString());
  if (!entry) {
    return undefined;
  }
  const currentType = currentTypeAt(entry.symbols, document.offsetAt(position));
  const field = currentType?.fields.find((item) => item.name === name);
  if (field) {
    return field.refTarget || field.typeName;
  }
  return undefined;
}

function fieldByType(workspace, typeName, fieldName) {
  for (const entry of workspace.types.get(typeName) || []) {
    const field = entry.item.fields.find((item) => item.name === fieldName);
    if (field) {
      return { item: field, document: entry.document };
    }
  }
  return undefined;
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
  const leftMatch = left.match(new RegExp(`(${IDENT}(?:\\s*\\.\\s*${IDENT})*)$`));
  const rightMatch = right.match(new RegExp(`^(?:\\s*\\.\\s*${IDENT})*`));
  const text = `${leftMatch ? leftMatch[1] : ""}${rightMatch ? rightMatch[0] : ""}`;
  return [...text.matchAll(new RegExp(IDENT, "g"))].map((match) => ({ name: match[0] }));
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

function findDiagnosticsCwd(documentPath, extensionPath) {
  const workspace = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
  const candidates = [
    workspace,
    path.resolve(extensionPath, "..", ".."),
    path.dirname(documentPath)
  ].filter(Boolean);

  for (const candidate of candidates) {
    if (fs.existsSync(path.join(candidate, "Cargo.toml"))) {
      return candidate;
    }
  }

  return workspace || path.dirname(documentPath);
}

function runDiagnosticsCommand(command, args, cwd, stdin) {
  return new Promise((resolve, reject) => {
    const child = cp.spawn(command, args, {
      cwd,
      shell: process.platform === "win32"
    });
    let stdout = "";
    let stderr = "";

    child.stdout.setEncoding("utf8");
    child.stderr.setEncoding("utf8");
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
    });
    child.on("error", reject);
    child.on("close", (code) => {
      if (code === 0) {
        resolve({ stdout, stderr });
      } else {
        reject(new Error(stderr.trim() || `${command} exited with code ${code}`));
      }
    });

    child.stdin.end(stdin);
  });
}

function parseDiagnosticsOutput(stdout) {
  const text = stdout.trim();
  if (!text) {
    return { diagnostics: [] };
  }
  return JSON.parse(text);
}

function toVsCodeDiagnostic(raw) {
  const range = new vscode.Range(
    raw.startLine || 0,
    raw.startCharacter || 0,
    raw.endLine || raw.startLine || 0,
    raw.endCharacter || Math.max((raw.startCharacter || 0) + 1, 1)
  );
  const diagnostic = new vscode.Diagnostic(
    range,
    `${raw.code}: ${raw.message}`,
    vscode.DiagnosticSeverity.Error
  );
  diagnostic.code = raw.code;
  diagnostic.source = `cft ${raw.stage}`;
  if (Array.isArray(raw.related)) {
    diagnostic.relatedInformation = raw.related.map((related) => {
      const relatedRange = new vscode.Range(
        related.startLine || 0,
        related.startCharacter || 0,
        related.endLine || related.startLine || 0,
        related.endCharacter || Math.max((related.startCharacter || 0) + 1, 1)
      );
      return new vscode.DiagnosticRelatedInformation(
        new vscode.Location(vscode.Uri.file(related.path), relatedRange),
        related.label || "related location"
      );
    });
  }
  return diagnostic;
}

function normalizePath(filePath) {
  return path.resolve(filePath);
}

module.exports = {
  activate,
  deactivate
};
