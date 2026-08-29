namespace CoflowRuntime;

internal sealed class CfdRecordCatalog
{
    private readonly Dictionary<(string DeclaredType, string Key), CfdRecordNode> _byIdentity = new();
    private readonly Dictionary<(string DomainType, string Key), CfdRecordNode> _byDomain = new();
    private readonly Dictionary<string, List<CfdRecordNode>> _byKey = new(StringComparer.Ordinal);
    private readonly Dictionary<string, List<CfdRecordNode>> _byDeclaredType = new(StringComparer.Ordinal);
    private readonly Dictionary<string, List<CfdRecordNode>> _byAssignableType = new(StringComparer.Ordinal);

    internal CfdRecordCatalog(
        IReadOnlyList<CfdDocument> documents,
        IReadOnlyDictionary<string, ICfdTypeBinding> bindings)
    {
        var records = new List<CfdRecordNode>();
        var diagnostics = new List<CfdDiagnostic>();
        foreach (var document in documents)
        foreach (var record in document.Records)
        {
            records.Add(record);
            var duplicate = !_byIdentity.TryAdd((record.DeclaredType, record.Key), record);
            Add(_byKey, record.Key, record);
            Add(_byDeclaredType, record.DeclaredType, record);

            bindings.TryGetValue(record.DeclaredType, out var binding);
            if (record.GroupType is not null && binding is not null &&
                !binding.AssignableTypes.Contains(record.GroupType, StringComparer.Ordinal))
            {
                diagnostics.Add(new CfdDiagnostic(
                    "CFD-RECORD-GROUP-TYPE",
                    $"record type `{record.DeclaredType}` is not assignable to group `{record.GroupType}`",
                    document.Path,
                    record.Span));
            }

            var domains = binding?.AssignableTypes ?? new[] { record.DeclaredType };
            foreach (var domain in domains.Append(record.DeclaredType).Distinct(StringComparer.Ordinal))
            {
                duplicate |= !_byDomain.TryAdd((domain, record.Key), record);
                Add(_byAssignableType, domain, record);
            }
            if (duplicate)
            {
                diagnostics.Add(new CfdDiagnostic(
                    "CFD-SYNTAX-DUPLICATE-RECORD",
                    $"record key `{record.Key}` is declared more than once in an assignable type domain",
                    document.Path,
                    record.Span));
            }
        }
        if (diagnostics.Count != 0) throw new CfdLoadException(diagnostics);
        All = records.ToArray();
    }

    internal IReadOnlyList<CfdRecordNode> All { get; }
    internal CfdRecordNode? Find(string declaredType, string key) =>
        _byIdentity.TryGetValue((declaredType, key), out var record) ? record : null;
    internal CfdRecordNode? FindAssignable(string declaredType, string key) =>
        _byDomain.TryGetValue((declaredType, key), out var record) ? record : null;
    internal IReadOnlyList<CfdRecordNode> WithKey(string key) =>
        _byKey.TryGetValue(key, out var records) ? records : Array.Empty<CfdRecordNode>();
    internal IReadOnlyList<CfdRecordNode> OfType(string declaredType) =>
        _byDeclaredType.TryGetValue(declaredType, out var records) ? records : Array.Empty<CfdRecordNode>();
    internal IReadOnlyList<CfdRecordNode> AssignableTo(string declaredType) =>
        _byAssignableType.TryGetValue(declaredType, out var records) ? records : Array.Empty<CfdRecordNode>();

    private static void Add(Dictionary<string, List<CfdRecordNode>> index, string key, CfdRecordNode record)
    {
        if (!index.TryGetValue(key, out var records)) index.Add(key, records = new List<CfdRecordNode>());
        records.Add(record);
    }
}
