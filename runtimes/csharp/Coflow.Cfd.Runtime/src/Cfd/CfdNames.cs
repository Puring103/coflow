namespace CoflowRuntime.Generated;

internal sealed class CfdNameResolver
{
    private readonly string? _namespace;
    private readonly IReadOnlyDictionary<string, string> _uses;

    private CfdNameResolver(string? declaredNamespace, IReadOnlyDictionary<string, string> uses)
    {
        _namespace = declaredNamespace;
        _uses = uses;
    }

    internal static CfdNameResolver Root { get; } = new(
        null,
        new Dictionary<string, string>(StringComparer.Ordinal));

    internal static CfdNameResolver Create(
        CfdDocument document,
        HashSet<string> symbols)
    {
        var uses = new Dictionary<string, string>(StringComparer.Ordinal);
        var diagnostics = new List<CfdDiagnostic>();
        foreach (var directive in document.Uses)
        {
            if (!symbols.Contains(directive.Path))
            {
                diagnostics.Add(new CfdDiagnostic(
                    "CFD-NAME-UNKNOWN-USE",
                    $"unknown use target `{directive.Path}`",
                    document.Path,
                    directive.Span));
                continue;
            }

            var localSymbol = document.Namespace is null
                ? directive.LocalName
                : $"{document.Namespace}::{directive.LocalName}";
            if (symbols.Contains(localSymbol))
            {
                diagnostics.Add(new CfdDiagnostic(
                    "CFD-NAME-USE-CONFLICT",
                    $"use name `{directive.LocalName}` conflicts with `{localSymbol}`",
                    document.Path,
                    directive.Span));
                continue;
            }

            if (!uses.TryAdd(directive.LocalName, directive.Path))
            {
                diagnostics.Add(new CfdDiagnostic(
                    "CFD-NAME-USE-CONFLICT",
                    $"use name `{directive.LocalName}` is declared more than once",
                    document.Path,
                    directive.Span));
            }
        }

        if (diagnostics.Count != 0) throw new CfdLoadException(diagnostics);
        return new CfdNameResolver(document.Namespace, uses);
    }

    internal string Resolve(string name)
    {
        var separator = name.IndexOf("::", StringComparison.Ordinal);
        if (separator >= 0)
        {
            var head = name[..separator];
            return _uses.TryGetValue(head, out var target)
                ? target + name[separator..]
                : name;
        }

        if (_uses.TryGetValue(name, out var imported)) return imported;
        return _namespace is null ? name : $"{_namespace}::{name}";
    }

    internal string ResolveStaticPath(string path) =>
        path.Contains("::", StringComparison.Ordinal) ? Resolve(path) : path;
}

internal sealed class CfdBoundDocuments
{
    internal CfdBoundDocuments(
        IReadOnlyList<CfdDocument> documents,
        IReadOnlyDictionary<CfdRecordNode, CfdNameResolver> recordNames,
        IReadOnlyDictionary<CfdRecordNode, string> recordPaths)
    {
        Documents = documents;
        RecordNames = recordNames;
        RecordPaths = recordPaths;
    }

    internal IReadOnlyList<CfdDocument> Documents { get; }
    internal IReadOnlyDictionary<CfdRecordNode, CfdNameResolver> RecordNames { get; }
    internal IReadOnlyDictionary<CfdRecordNode, string> RecordPaths { get; }
}

internal static class CfdNameBinder
{
    internal static CfdBoundDocuments Bind(
        IReadOnlyList<CfdDocument> documents,
        IEnumerable<string> symbols,
        IReadOnlyDictionary<string, CoflowConstant>? constants = null)
    {
        var knownSymbols = new HashSet<string>(symbols, StringComparer.Ordinal);
        var boundDocuments = new List<CfdDocument>(documents.Count);
        var recordNames = new Dictionary<CfdRecordNode, CfdNameResolver>();
        var recordPaths = new Dictionary<CfdRecordNode, string>();
        foreach (var document in documents)
        {
            var names = CfdNameResolver.Create(document, knownSymbols);
            var records = document.Records.Select(record => BindRecord(record, names, constants)).ToArray();
            foreach (var record in records)
            {
                recordNames.Add(record, names);
                recordPaths.Add(record, document.Path);
            }
            boundDocuments.Add(new CfdDocument(
                document.Path,
                document.Namespace,
                document.Uses,
                records));
        }
        return new CfdBoundDocuments(boundDocuments, recordNames, recordPaths);
    }

    private static CfdRecordNode BindRecord(
        CfdRecordNode record,
        CfdNameResolver names,
        IReadOnlyDictionary<string, CoflowConstant>? constants) =>
        new(
            record.Key,
            names.Resolve(record.DeclaredType),
            record.Fields.Select(field => new CfdFieldNode(
                field.Name,
                BindValue(field.Value, names, constants),
                field.Span)).ToArray(),
            record.Span,
            record.GroupType is null ? null : names.Resolve(record.GroupType));

    private static CfdValueNode BindValue(
        CfdValueNode value,
        CfdNameResolver names,
        IReadOnlyDictionary<string, CoflowConstant>? constants) => value switch
    {
        CfdSomeValue some => new CfdSomeValue(BindValue(some.Value, names, constants), some.Span),
        CfdOkValue ok => new CfdOkValue(BindValue(ok.Value, names, constants), ok.Span),
        CfdErrValue error => new CfdErrValue(BindValue(error.Value, names, constants), error.Span),
        CfdScalarValue scalar => BindScalar(scalar, names, constants),
        CfdFormattedStringValue formatted => new CfdFormattedStringValue(
            formatted.Source,
            formatted.Segments.Select(segment => BindFormatSegment(segment, names)).ToArray(),
            formatted.Span),
        CfdBitExpressionValue bits => new CfdBitExpressionValue(BindBits(bits.Expression, names), bits.Span),
        CfdReferenceValue reference => new CfdReferenceValue(
            reference.TypeName is null ? null : names.Resolve(reference.TypeName),
            reference.Key,
            reference.Span),
        CfdObjectValue objectValue => new CfdObjectValue(
            objectValue.DeclaredType is null ? null : names.Resolve(objectValue.DeclaredType),
            objectValue.Fields.Select(field => new CfdFieldNode(
                field.Name,
                BindValue(field.Value, names, constants),
                field.Span)).ToArray(),
            objectValue.Span),
        CfdArrayValue array => new CfdArrayValue(
            array.Items.Select(item => BindValue(item, names, constants)).ToArray(),
            array.Span),
        CfdDictionaryValue dictionary => new CfdDictionaryValue(
            dictionary.Entries.Select(entry => new CfdEntryNode(
                BindValue(entry.Key, names, constants),
                BindValue(entry.Value, names, constants),
                entry.Span)).ToArray(),
            dictionary.Span),
        _ => value,
    };

    private static CfdValueNode BindScalar(
        CfdScalarValue scalar,
        CfdNameResolver names,
        IReadOnlyDictionary<string, CoflowConstant>? constants)
    {
        var constantName = names.Resolve(scalar.Value);
        if (constants is not null && constants.TryGetValue(constantName, out var constant))
            return new CfdConstantValue(constant, scalar.Span);
        return new CfdScalarValue(names.ResolveStaticPath(scalar.Value), scalar.Span);
    }

    private static CfdFormatSegment BindFormatSegment(CfdFormatSegment segment, CfdNameResolver names) =>
        segment is CfdFormatReference reference
            ? new CfdFormatReference(
                reference.TypeName is null ? null : names.Resolve(reference.TypeName),
                reference.Key,
                reference.Path)
            : segment;

    private static CfdBitExpression BindBits(CfdBitExpression expression, CfdNameResolver names) =>
        expression.Kind switch
        {
            CfdBitExpressionKind.Value value => CfdBitExpression.Value(
                names.ResolveStaticPath(value.Text),
                expression.Span),
            CfdBitExpressionKind.Binary binary => CfdBitExpression.Binary(
                binary.Operator,
                BindBits(binary.Left, names),
                BindBits(binary.Right, names),
                expression.Span),
            _ => throw new InvalidOperationException("Unknown CFD bit expression."),
        };
}
