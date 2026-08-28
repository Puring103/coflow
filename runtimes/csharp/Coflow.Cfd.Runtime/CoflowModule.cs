namespace CoflowRuntime;

using System.Collections;
using System.ComponentModel;

public enum CoflowAnnotationArgumentKind { Name, String, Int, Float, Bool }

public sealed class CoflowAnnotationArgument
{
    public CoflowAnnotationArgument(CoflowAnnotationArgumentKind kind, object value)
    {
        Kind = kind;
        Value = value ?? throw new ArgumentNullException(nameof(value));
    }
    public CoflowAnnotationArgumentKind Kind { get; }
    public object Value { get; }
}

public sealed class CoflowAnnotation
{
    public CoflowAnnotation(string name, IReadOnlyList<CoflowAnnotationArgument> arguments)
    {
        Name = name ?? throw new ArgumentNullException(nameof(name));
        Arguments = arguments ?? throw new ArgumentNullException(nameof(arguments));
    }
    public string Name { get; }
    public IReadOnlyList<CoflowAnnotationArgument> Arguments { get; }
}

[EditorBrowsable(EditorBrowsableState.Never)]
public sealed class CoflowConstant
{
    private readonly object? _value;
    private readonly Func<CfdLoadContext, object>? _factory;

    public CoflowConstant(string declaredName, Type runtimeType, object value)
    {
        DeclaredName = declaredName ?? throw new ArgumentNullException(nameof(declaredName));
        RuntimeType = runtimeType ?? throw new ArgumentNullException(nameof(runtimeType));
        _value = value ?? throw new ArgumentNullException(nameof(value));
        if (!runtimeType.IsInstanceOfType(value))
            throw new ArgumentException("The constant value does not match its generated type.", nameof(value));
    }

    public CoflowConstant(string declaredName, Type runtimeType, Func<CfdLoadContext, object> factory)
    {
        DeclaredName = declaredName ?? throw new ArgumentNullException(nameof(declaredName));
        RuntimeType = runtimeType ?? throw new ArgumentNullException(nameof(runtimeType));
        _factory = factory ?? throw new ArgumentNullException(nameof(factory));
    }

    public string DeclaredName { get; }
    public Type RuntimeType { get; }
    public object Value => _value ?? throw new InvalidOperationException(
        $"Coflow constant `{DeclaredName}` is resolved while a module is loaded.");

    internal object Resolve(CfdLoadContext context)
    {
        var value = _factory is null ? _value! : _factory(context);
        if (!RuntimeType.IsInstanceOfType(value))
            throw new CoflowLoadException(new[] { new CfdDiagnostic(
                "COFLOW-CONSTANT-TYPE",
                $"constant `{DeclaredName}` produced `{value?.GetType()}` instead of `{RuntimeType}`",
                string.Empty) });
        return value;
    }
}

[EditorBrowsable(EditorBrowsableState.Never)]
public static class CoflowConstantValues
{
    public static IReadOnlyList<T> List<T>(params T[] values) =>
        Array.AsReadOnly(values ?? throw new ArgumentNullException(nameof(values)));

    public static IReadOnlyDictionary<TKey, TValue> Dictionary<TKey, TValue>(
        params KeyValuePair<TKey, TValue>[] entries) where TKey : notnull
    {
        if (entries is null) throw new ArgumentNullException(nameof(entries));
        var values = new Dictionary<TKey, TValue>();
        foreach (var entry in entries)
            if (!values.TryAdd(entry.Key, entry.Value))
                throw new ArgumentException($"Duplicate constant key `{entry.Key}`.", nameof(entries));
        return new System.Collections.ObjectModel.ReadOnlyDictionary<TKey, TValue>(values);
    }
}

[EditorBrowsable(EditorBrowsableState.Never)]
public interface ICoflowGeneratedContract
{
    IReadOnlyList<ICoflowTypeMetadata> Types { get; }
    IReadOnlyList<ICoflowEnumMetadata> Enums { get; }
    IReadOnlyList<CoflowConstant> Constants { get; }
}

[EditorBrowsable(EditorBrowsableState.Never)]
public interface ICoflowEnumMetadata
{
    string DeclaredType { get; }
    Type RuntimeType { get; }
    bool IsFlags { get; }
    IReadOnlyList<CoflowAnnotation> Annotations { get; }
    IReadOnlyDictionary<string, object> Variants { get; }
    IReadOnlyList<CoflowAnnotation> VariantAnnotations(string variantName);
    object FromInt64(long value);
}

[EditorBrowsable(EditorBrowsableState.Never)]
public interface ICoflowTypeMetadata : ICfdTypeBinding
{
    Type RuntimeType { get; }
    bool IsSingleton { get; }
    bool IsAbstract { get; }
    bool IsSealed { get; }
    IReadOnlyList<CoflowAnnotation> Annotations { get; }
    IReadOnlyList<string> FieldNames { get; }
    IReadOnlyList<CoflowAnnotation> FieldAnnotations(string fieldName);
    Type GetFieldType(string fieldName);
    object GetField(object record, string fieldName);
    Delegate GetFieldReader(string fieldName);
    bool HasFieldDefault(string fieldName);
    object CreateObject(CfdLoadContext context, IReadOnlyDictionary<string, object?> fields);
    Delegate CreateVmObjectFactory(CfdLoadContext context);
    Delegate CreateVmDefaultFactory(string fieldName, CfdLoadContext context);
}

[EditorBrowsable(EditorBrowsableState.Never)]
public abstract class CoflowGeneratedTypeMetadata
{
    public abstract CoflowFieldBinding GetFieldBinding(string fieldName);
    public Type GetFieldType(string fieldName) => GetFieldBinding(fieldName).RuntimeType;
    public object GetField(object record, string fieldName) => GetFieldBinding(fieldName).Read(record);
    public Delegate GetFieldReader(string fieldName) => GetFieldBinding(fieldName).Reader;
}

[EditorBrowsable(EditorBrowsableState.Never)]
public interface ICoflowRecordMetadata : ICoflowTypeMetadata
{
    Type KeyType { get; }
    object ParseKey(string key);
    object GetKey(object record);
    Delegate GetKeyReader();
    object CreateRecord(string key, CfdLoadContext context);
    void PopulateRecord(object target, CfdRecordNode record, CfdLoadContext context);
}

[EditorBrowsable(EditorBrowsableState.Never)]
public interface ICoflowHostMetadata : ICoflowTypeMetadata
{
    object CreateHost(CfdLoadContext context);
}

[EditorBrowsable(EditorBrowsableState.Never)]
public static class CoflowGeneratedRegistry
{
    private static readonly object Sync = new();
    private static readonly List<ICoflowGeneratedContract> Contracts = new();
    private static ICoflowGeneratedContract? _resolved;

    public static void Register(ICoflowGeneratedContract contract)
    {
        if (contract is null) throw new ArgumentNullException(nameof(contract));
        lock (Sync)
        {
            if (!Contracts.Any(value => ReferenceEquals(value, contract)))
            {
                Contracts.Add(contract);
                _resolved = null;
            }
        }
    }

    internal static ICoflowGeneratedContract RequireContract()
    {
        lock (Sync)
        {
            if (Contracts.Count == 0)
                throw new InvalidOperationException("No generated Coflow contract is registered.");
            return _resolved ??= Contracts.Count == 1
                ? Contracts[0]
                : new CompositeContract(Contracts.ToArray());
        }
    }

    private sealed class CompositeContract : ICoflowGeneratedContract
    {
        internal CompositeContract(IReadOnlyList<ICoflowGeneratedContract> contracts)
        {
            Types = contracts.SelectMany(value => value.Types).ToArray();
            Enums = contracts.SelectMany(value => value.Enums).ToArray();
            Constants = contracts.SelectMany(value => value.Constants).ToArray();
        }
        public IReadOnlyList<ICoflowTypeMetadata> Types { get; }
        public IReadOnlyList<ICoflowEnumMetadata> Enums { get; }
        public IReadOnlyList<CoflowConstant> Constants { get; }
    }
}

public sealed class CoflowLoadException : CfdLoadException
{
    public CoflowLoadException(IReadOnlyList<CfdDiagnostic> diagnostics) : base(diagnostics) { }
}

public abstract class CoflowTable : IEnumerable
{
    public abstract Type RecordType { get; }
    public abstract Type KeyType { get; }
    public abstract int Count { get; }
    internal abstract IEnumerable<object> UntypedRecords { get; }
    internal abstract object KeyOf(object record);
    public abstract IEnumerator GetEnumerator();
}

public interface ICoflowTableToken<TTable> where TTable : CoflowTable
{
    Type RecordType { get; }
    Type KeyType { get; }
    TTable Empty { get; }
}

public sealed class CoflowStringTableToken<T> : ICoflowTableToken<CoflowStringTable<T>> where T : class
{
    private static readonly CoflowStringTable<T> EmptyTable = new(Array.Empty<T>(), static _ => string.Empty);
    public Type RecordType => typeof(T);
    public Type KeyType => typeof(string);
    public CoflowStringTable<T> Empty => EmptyTable;
}

public sealed class CoflowEnumTableToken<T, TKey> : ICoflowTableToken<CoflowEnumTable<T, TKey>>
    where T : class where TKey : struct, Enum
{
    private static readonly CoflowEnumTable<T, TKey> EmptyTable = new(Array.Empty<T>(), static _ => default);
    public Type RecordType => typeof(T);
    public Type KeyType => typeof(TKey);
    public CoflowEnumTable<T, TKey> Empty => EmptyTable;
}

public sealed class CoflowStringTable<T> : CoflowTable, IReadOnlyList<T> where T : class
{
    private readonly IReadOnlyList<T>[] _segments;
    private readonly Dictionary<string, T> _index;
    private readonly Func<T, string> _key;

    internal CoflowStringTable(IReadOnlyList<T> records, Func<T, string> key)
        : this(new[] { records }, key) { }

    internal CoflowStringTable(IReadOnlyList<T>[] segments, Func<T, string> key)
    {
        _segments = segments;
        _key = key;
        _index = new Dictionary<string, T>(StringComparer.Ordinal);
        foreach (var record in segments.SelectMany(value => value))
            if (!_index.TryAdd(key(record), record)) Duplicate(key(record));
    }

    public override Type RecordType => typeof(T);
    public override Type KeyType => typeof(string);
    public override int Count => _segments.Sum(value => value.Count);
    public T this[int index] => SegmentItem(_segments, index);
    public Option<T> Get(string key) => _index.TryGetValue(key, out var value) ? Option<T>.Some(value) : Option<T>.None;
    internal override IEnumerable<object> UntypedRecords => this.Cast<object>();
    internal override object KeyOf(object record) => _key((T)record);
    IEnumerator<T> IEnumerable<T>.GetEnumerator() => _segments.SelectMany(value => value).GetEnumerator();
    public override IEnumerator GetEnumerator() => _segments.SelectMany(value => value).GetEnumerator();
    private static void Duplicate(string key) => throw new CoflowLoadException(new[] {
        new CfdDiagnostic("CFD-SYNTAX-DUPLICATE-RECORD", $"record key `{key}` is declared more than once for `{typeof(T)}`", string.Empty) });
    private static TItem SegmentItem<TItem>(IReadOnlyList<TItem>[] segments, int index)
    {
        foreach (var segment in segments) { if (index < segment.Count) return segment[index]; index -= segment.Count; }
        throw new ArgumentOutOfRangeException(nameof(index));
    }
}

public sealed class CoflowEnumTable<T, TKey> : CoflowTable, IReadOnlyList<T>
    where T : class where TKey : struct, Enum
{
    private readonly IReadOnlyList<T>[] _segments;
    private readonly Dictionary<TKey, T> _index;
    private readonly Func<T, TKey> _key;

    internal CoflowEnumTable(IReadOnlyList<T> records, Func<T, TKey> key) : this(new[] { records }, key) { }
    internal CoflowEnumTable(IReadOnlyList<T>[] segments, Func<T, TKey> key)
    {
        _segments = segments;
        _key = key;
        _index = new Dictionary<TKey, T>();
        foreach (var record in segments.SelectMany(value => value))
            if (!_index.TryAdd(key(record), record)) Duplicate(key(record));
    }

    public override Type RecordType => typeof(T);
    public override Type KeyType => typeof(TKey);
    public override int Count => _segments.Sum(value => value.Count);
    public T this[int index] { get { var offset = index; foreach (var segment in _segments) { if (offset < segment.Count) return segment[offset]; offset -= segment.Count; } throw new ArgumentOutOfRangeException(nameof(index)); } }
    public Option<T> Get(TKey key) => _index.TryGetValue(key, out var value) ? Option<T>.Some(value) : Option<T>.None;
    internal override IEnumerable<object> UntypedRecords => this.Cast<object>();
    internal override object KeyOf(object record) => _key((T)record);
    IEnumerator<T> IEnumerable<T>.GetEnumerator() => _segments.SelectMany(value => value).GetEnumerator();
    public override IEnumerator GetEnumerator() => _segments.SelectMany(value => value).GetEnumerator();
    private static void Duplicate(TKey key) => throw new CoflowLoadException(new[] {
        new CfdDiagnostic("CFD-SYNTAX-DUPLICATE-RECORD", $"record key `{key}` is declared more than once for `{typeof(T)}`", string.Empty) });
}

public sealed class CoflowModule
{
    private readonly IReadOnlyDictionary<Type, CoflowTable> _tables;
    private readonly IReadOnlyDictionary<Type, object> _singletons;
    private readonly IReadOnlyDictionary<CoflowFunctionIdentity, CoflowFunctionEntry> _functions;

    private CoflowModule(
        ICoflowGeneratedContract contract,
        IReadOnlyDictionary<Type, CoflowTable> tables,
        IReadOnlyDictionary<Type, object> singletons,
        IReadOnlyList<CoflowFunctionEntry> functions,
        bool functionsCompiled)
    {
        Contract = contract;
        _tables = tables;
        _singletons = singletons;
        _functions = functions.ToDictionary(value => value.Identity);
        FunctionsCompiled = functionsCompiled;
    }

    internal ICoflowGeneratedContract Contract { get; }
    internal IReadOnlyDictionary<Type, CoflowTable> Tables => _tables;
    internal IReadOnlyDictionary<Type, object> Singletons => _singletons;
    internal IReadOnlyDictionary<CoflowFunctionIdentity, CoflowFunctionEntry> Functions => _functions;
    public bool FunctionsCompiled { get; }

    public TTable Table<TTable>(ICoflowTableToken<TTable> token) where TTable : CoflowTable
    {
        if (token is null) throw new ArgumentNullException(nameof(token));
        return _tables.TryGetValue(token.RecordType, out var table) ? (TTable)table : token.Empty;
    }

    public Option<T> Singleton<T>() where T : class => _singletons.TryGetValue(typeof(T), out var value)
        ? Option<T>.Some((T)value) : Option<T>.None;

    internal static CoflowModule Load(string[] sources, ICoflowGeneratedContract contract, bool compile)
    {
        if (sources is null) throw new ArgumentNullException(nameof(sources));
        ValidateContract(contract);
        var documents = CfdParser.ParseAll(sources.Select((text, index) =>
            new CfdSource($"source[{index}]", text ?? throw new ArgumentException("CFD source cannot be null.", nameof(sources)))));
        var context = new CfdLoadContext(
            documents, null, contract.Types, contract.Enums, contract.Constants, compile);
        documents = context.Documents;
        var metadataByName = contract.Types.ToDictionary(value => value.DeclaredType, StringComparer.Ordinal);
        foreach (var record in documents.SelectMany(value => value.Records))
        {
            if (!metadataByName.TryGetValue(record.DeclaredType, out var metadata))
                throw Error("CFD-REF-UNKNOWN-TYPE", $"unknown record type `{record.DeclaredType}`", record.Span);
            if (metadata is ICoflowHostMetadata)
                throw Error("CFD-HOST-RECORD", $"CFD cannot declare @Host `{record.DeclaredType}`", record.Span);
            if (metadata is not ICoflowRecordMetadata)
                throw Error("CFD-REF-UNKNOWN-TYPE", $"type `{record.DeclaredType}` cannot be declared as a record", record.Span);
        }

        var records = new Dictionary<(string, string), object>();
        var singletons = new Dictionary<Type, object>();
        foreach (var record in documents.SelectMany(value => value.Records))
        {
            var metadata = (ICoflowRecordMetadata)metadataByName[record.DeclaredType];
            var shell = metadata.CreateRecord(record.Key, context);
            records.Add((record.DeclaredType, record.Key), shell);
            context.RegisterRecord(record.DeclaredType, record.Key, shell);
        }
        foreach (var record in documents.SelectMany(value => value.Records))
            ((ICoflowRecordMetadata)metadataByName[record.DeclaredType]).PopulateRecord(
                records[(record.DeclaredType, record.Key)], record, context);
        foreach (var metadata in contract.Types)
        {
            if (metadata is ICoflowHostMetadata hostMetadata)
            {
                var host = hostMetadata.CreateHost(context);
                singletons.Add(metadata.RuntimeType, host);
                records.Add((metadata.DeclaredType, string.Empty), host);
                continue;
            }
            var nodes = documents.SelectMany(value => value.Records)
                .Where(value => value.DeclaredType == metadata.DeclaredType).ToArray();
            if (metadata.IsSingleton && nodes.Length > 1)
                throw Error("CFD-SINGLETON-COUNT", $"singleton `{metadata.DeclaredType}` appears more than once");
            if (metadata.IsSingleton && nodes.Length == 1)
                singletons.Add(metadata.RuntimeType, records[(metadata.DeclaredType, nodes[0].Key)]);
        }

        var tables = new Dictionary<Type, CoflowTable>();
        foreach (var metadata in contract.Types.OfType<ICoflowRecordMetadata>()
                     .Where(value => !value.IsSingleton && !value.IsAbstract))
        {
            var values = documents.SelectMany(value => value.Records)
                .Where(value => metadataByName[value.DeclaredType].AssignableTypes.Contains(metadata.DeclaredType, StringComparer.Ordinal))
                .Select(value => records[(value.DeclaredType, value.Key)]).ToArray();
            if (values.Length != 0) tables.Add(metadata.RuntimeType, CreateTable(metadata, values));
        }
        if (compile) CoflowCompiler.Compile(context.Functions, contract, records, context);
        return new CoflowModule(contract, tables, singletons, context.Functions, compile);
    }

    private static CoflowTable CreateTable(ICoflowRecordMetadata metadata, object[] values)
    {
        var method = typeof(CoflowModule).GetMethod(nameof(CreateTypedTable), System.Reflection.BindingFlags.Static | System.Reflection.BindingFlags.NonPublic)!
            .MakeGenericMethod(metadata.RuntimeType, metadata.KeyType);
        return InvokeTableFactory(method, new object[] { metadata, values });
    }

    private static CoflowTable CreateTypedTable<T, TKey>(ICoflowRecordMetadata metadata, object[] values) where T : class
    {
        var records = values.Cast<T>().ToArray();
        return typeof(TKey) == typeof(string)
            ? new CoflowStringTable<T>(records, value => (string)metadata.GetKey(value))
            : CreateEnumTable<T, TKey>(metadata, records);
    }

    private static CoflowTable CreateEnumTable<T, TKey>(ICoflowRecordMetadata metadata, IReadOnlyList<T> records) where T : class
    {
        var method = typeof(CoflowModule).GetMethod(nameof(CreateEnumTableCore), System.Reflection.BindingFlags.Static | System.Reflection.BindingFlags.NonPublic)!
            .MakeGenericMethod(typeof(T), typeof(TKey));
        return InvokeTableFactory(method, new object[] { metadata, records });
    }

    private static CoflowTable CreateEnumTableCore<T, TKey>(ICoflowRecordMetadata metadata, IReadOnlyList<T> records)
        where T : class where TKey : struct, Enum =>
        new CoflowEnumTable<T, TKey>(records, value => (TKey)metadata.GetKey(value));

    internal static CoflowTable InvokeTableFactory(System.Reflection.MethodInfo method, object[] arguments)
    {
        try { return (CoflowTable)method.Invoke(null, arguments)!; }
        catch (System.Reflection.TargetInvocationException error) when (error.InnerException is not null)
        {
            System.Runtime.ExceptionServices.ExceptionDispatchInfo.Capture(error.InnerException).Throw();
            throw;
        }
    }

    private static void ValidateContract(ICoflowGeneratedContract contract)
    {
        if (contract is null) throw new ArgumentNullException(nameof(contract));
        var duplicateName = contract.Types.Select(value => value.DeclaredType)
            .Concat(contract.Enums.Select(value => value.DeclaredType)).GroupBy(value => value, StringComparer.Ordinal)
            .FirstOrDefault(value => value.Count() > 1);
        if (duplicateName is not null) throw Error("COFLOW-METADATA-DUPLICATE-NAME", $"generated name `{duplicateName.Key}` is duplicated");
        var duplicateType = contract.Types.Select(value => value.RuntimeType)
            .Concat(contract.Enums.Select(value => value.RuntimeType)).GroupBy(value => value).FirstOrDefault(value => value.Count() > 1);
        if (duplicateType is not null) throw Error("COFLOW-METADATA-DUPLICATE-TYPE", $"runtime type `{duplicateType.Key}` is duplicated");
    }

    private static CoflowLoadException Error(string code, string message, CfdSpan? span = null) =>
        new(new[] { new CfdDiagnostic(code, message, string.Empty, span) });
}

public sealed class CoflowModuleSet
{
    private readonly IReadOnlyList<CoflowModule> _modules;
    private readonly IReadOnlyDictionary<Type, CoflowTable> _tables;
    private readonly IReadOnlyDictionary<Type, object> _singletons;
    private readonly IReadOnlyDictionary<CoflowFunctionIdentity, CoflowFunctionEntry> _functions;

    public CoflowModuleSet(params CoflowModule[] modules) : this((IReadOnlyList<CoflowModule>)modules) { }

    private CoflowModuleSet(IReadOnlyList<CoflowModule> modules)
    {
        if (modules is null) throw new ArgumentNullException(nameof(modules));
        _modules = modules.ToArray();
        if (_modules.Any(value => value is null)) throw new ArgumentException("A module cannot be null.", nameof(modules));
        if (_modules.Select(value => value.Contract).Distinct(ContractReferenceComparer.Instance).Skip(1).Any())
            throw new ArgumentException("All modules must use the same generated contract.", nameof(modules));
        _singletons = MergeUnique(_modules.SelectMany(value => value.Singletons), "singleton");
        _functions = MergeUnique(_modules.SelectMany(value => value.Functions), "function");
        _tables = CombineTables(_modules);
    }

    public IReadOnlyList<CoflowModule> Modules => _modules;
    public TTable Table<TTable>(ICoflowTableToken<TTable> token) where TTable : CoflowTable =>
        _tables.TryGetValue(token.RecordType, out var table) ? (TTable)table : token.Empty;
    public Option<T> Singleton<T>() where T : class => _singletons.TryGetValue(typeof(T), out var value)
        ? Option<T>.Some((T)value) : Option<T>.None;
    public CoflowModuleSet Add(CoflowModule module) => new(_modules.Append(module).ToArray());
    public CoflowModuleSet Remove(CoflowModule module) => new(_modules.Where(value => !ReferenceEquals(value, module)).ToArray());
    public CoflowModuleSet Replace(CoflowModule current, CoflowModule replacement) =>
        new(_modules.Select(value => ReferenceEquals(value, current) ? replacement : value).ToArray());

    private sealed class ContractReferenceComparer : IEqualityComparer<ICoflowGeneratedContract>
    {
        internal static ContractReferenceComparer Instance { get; } = new();
        public bool Equals(ICoflowGeneratedContract? left, ICoflowGeneratedContract? right) =>
            ReferenceEquals(left, right);
        public int GetHashCode(ICoflowGeneratedContract value) =>
            System.Runtime.CompilerServices.RuntimeHelpers.GetHashCode(value);
    }

    private static Dictionary<TKey, TValue> MergeUnique<TKey, TValue>(IEnumerable<KeyValuePair<TKey, TValue>> values, string kind) where TKey : notnull
    {
        var result = new Dictionary<TKey, TValue>();
        foreach (var pair in values)
            if (!result.TryAdd(pair.Key, pair.Value)) throw new ArgumentException($"Duplicate Coflow {kind} `{pair.Key}`.");
        return result;
    }

    private static IReadOnlyDictionary<Type, CoflowTable> CombineTables(IReadOnlyList<CoflowModule> modules)
    {
        var result = new Dictionary<Type, CoflowTable>();
        foreach (var group in modules.SelectMany(value => value.Tables).GroupBy(value => value.Key))
        {
            var tables = group.Select(value => value.Value).ToArray();
            var method = typeof(CoflowModuleSet).GetMethod(nameof(CombineTyped), System.Reflection.BindingFlags.Static | System.Reflection.BindingFlags.NonPublic)!
                .MakeGenericMethod(group.Key, tables[0].KeyType);
            result.Add(group.Key, CoflowModule.InvokeTableFactory(method, new object[] { tables }));
        }
        return result;
    }

    private static CoflowTable CombineTyped<T, TKey>(CoflowTable[] tables) where T : class
    {
        var segments = tables.Select(value => (IReadOnlyList<T>)value).ToArray();
        if (typeof(TKey) == typeof(string))
            return new CoflowStringTable<T>(segments, value => (string)tables[0].KeyOf(value));
        var method = typeof(CoflowModuleSet).GetMethod(nameof(CombineEnum), System.Reflection.BindingFlags.Static | System.Reflection.BindingFlags.NonPublic)!
            .MakeGenericMethod(typeof(T), typeof(TKey));
        return CoflowModule.InvokeTableFactory(method, new object[] { tables, segments });
    }

    private static CoflowTable CombineEnum<T, TKey>(CoflowTable[] tables, IReadOnlyList<T>[] segments)
        where T : class where TKey : struct, Enum =>
        new CoflowEnumTable<T, TKey>(segments, value => (TKey)tables[0].KeyOf(value));
}

internal static class CoflowLoader
{
    internal static CoflowModule LoadData(string[] sources, ICoflowGeneratedContract? contract = null) =>
        CoflowModule.Load(sources, contract ?? CoflowGeneratedRegistry.RequireContract(), false);
    internal static CoflowModule LoadAndCompile(string[] sources, ICoflowGeneratedContract? contract = null) =>
        CoflowModule.Load(sources, contract ?? CoflowGeneratedRegistry.RequireContract(), true);
}
