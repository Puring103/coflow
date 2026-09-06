namespace Coflow.Runtime.CompilerServices;

internal sealed class CoflowCompilerCatalog
{
    internal CoflowCompilerCatalog(ICoflowSchema contract)
    {
        Metadata = contract.Types.ToDictionary(item => item.DeclaredType, StringComparer.Ordinal);
        Enums = contract.Enums.ToDictionary(item => item.DeclaredType, StringComparer.Ordinal);
        Constants = contract.Constants.ToDictionary(item => item.DeclaredName, StringComparer.Ordinal);
        SchemaTypeNames = contract.Types
            .Select(item => (item.RuntimeType, item.DeclaredType))
            .Concat(contract.Enums.Select(item => (item.RuntimeType, item.DeclaredType)))
            .ToDictionary(item => item.RuntimeType, item => item.DeclaredType);
        MetadataByRuntimeType = contract.Types.ToDictionary(item => item.RuntimeType);
        EnumsByRuntimeType = contract.Enums.ToDictionary(item => item.RuntimeType);
    }

    internal IReadOnlyDictionary<string, ICoflowTypeMetadata> Metadata { get; }
    internal IReadOnlyDictionary<string, ICoflowEnumMetadata> Enums { get; }
    internal IReadOnlyDictionary<string, CoflowConstant> Constants { get; }
    internal IReadOnlyDictionary<Type, string> SchemaTypeNames { get; }
    internal IReadOnlyDictionary<Type, ICoflowTypeMetadata> MetadataByRuntimeType { get; }
    internal IReadOnlyDictionary<Type, ICoflowEnumMetadata> EnumsByRuntimeType { get; }
}

internal readonly record struct CoflowRecord(string DeclaredType, object Value);

internal sealed class CoflowRecordCatalog
{
    private readonly Dictionary<(string DeclaredType, string Key), object> _byIdentity = new();
    private readonly Dictionary<string, List<CoflowRecord>> _byKey = new(StringComparer.Ordinal);

    internal void Add(string declaredType, string key, object value)
    {
        _byIdentity.Add((declaredType, key), value);
        if (!_byKey.TryGetValue(key, out var records)) _byKey.Add(key, records = new List<CoflowRecord>());
        records.Add(new CoflowRecord(declaredType, value));
    }

    internal object Get(string declaredType, string key) => _byIdentity[(declaredType, key)];
    internal bool TryGet(string declaredType, string key, out object value) =>
        _byIdentity.TryGetValue((declaredType, key), out value!);
    internal IReadOnlyList<CoflowRecord> WithKey(string key) =>
        _byKey.TryGetValue(key, out var records) ? records : Array.Empty<CoflowRecord>();
}
