namespace CoflowRuntime;

using System.Collections;
using System.ComponentModel;

public enum CoflowAnnotationArgumentKind
{
    Name,
    String,
    Int,
    Float,
    Bool,
}

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
            throw new ArgumentException("The constant value does not match its generated runtime type.", nameof(value));
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
        $"Coflow constant `{DeclaredName}` is resolved for each loaded generation.");

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
        params KeyValuePair<TKey, TValue>[] entries)
        where TKey : notnull
    {
        if (entries is null) throw new ArgumentNullException(nameof(entries));
        var values = new Dictionary<TKey, TValue>();
        foreach (var entry in entries)
        {
            if (!values.TryAdd(entry.Key, entry.Value))
                throw new ArgumentException($"Duplicate constant dictionary key `{entry.Key}`.", nameof(entries));
        }
        return new System.Collections.ObjectModel.ReadOnlyDictionary<TKey, TValue>(values);
    }
}

[EditorBrowsable(EditorBrowsableState.Never)]
public interface ICoflowGeneratedModule
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
    Type KeyType { get; }
    bool IsSingleton { get; }
    bool IsHost { get; }
    bool IsAbstract { get; }
    bool IsSealed { get; }
    bool IsRecord { get; }
    IReadOnlyList<CoflowAnnotation> Annotations { get; }
    object ParseKey(string key);
    object GetKey(object record);
    IReadOnlyList<string> FieldNames { get; }
    IReadOnlyList<CoflowAnnotation> FieldAnnotations(string fieldName);
    Type GetFieldType(string fieldName);
    object GetField(object record, string fieldName);
    bool HasFieldDefault(string fieldName);
    object CreateObject(CfdLoadContext context, IReadOnlyDictionary<string, object?> fields);
    object CreateHost(CfdLoadContext context);
    void TransferHostState(object source, object target);
}

[EditorBrowsable(EditorBrowsableState.Never)]
public static class CoflowGeneratedRegistry
{
    private static readonly object Sync = new();
    private static ICoflowGeneratedModule? _module;

    public static void Register(ICoflowGeneratedModule module)
    {
        if (module is null) throw new ArgumentNullException(nameof(module));
        lock (Sync)
        {
            if (_module is not null && !ReferenceEquals(_module, module))
                throw new InvalidOperationException("Only one generated Coflow module can be registered in a process.");
            _module = module;
        }
    }

    internal static ICoflowGeneratedModule RequireModule()
    {
        lock (Sync)
        {
            return _module ?? throw new InvalidOperationException(
                "No generated Coflow module is registered. Reference the C# sources generated from the project CFT.");
        }
    }
}

internal static class CoflowLoader
{
    public static CoflowData LoadData(string cfd) => LoadData(new[] { cfd });

    public static CoflowData LoadData(string[] cfdSources) =>
        Load(cfdSources, CoflowGeneratedRegistry.RequireModule(), functionsCompiled: false);

    public static CoflowData LoadAndCompile(string cfd) => LoadAndCompile(new[] { cfd });

    public static CoflowData LoadAndCompile(string[] cfdSources) =>
        Load(cfdSources, CoflowGeneratedRegistry.RequireModule(), functionsCompiled: true);

    internal static CoflowData LoadData(string[] cfdSources, ICoflowGeneratedModule module)
        => Load(cfdSources, module, functionsCompiled: false);

    internal static CoflowData LoadAndCompile(string[] cfdSources, ICoflowGeneratedModule module)
        => Load(cfdSources, module, functionsCompiled: true);

    private static CoflowData Load(
        string[] cfdSources,
        ICoflowGeneratedModule module,
        bool functionsCompiled)
    {
        if (cfdSources is null) throw new ArgumentNullException(nameof(cfdSources));
        if (module is null) throw new ArgumentNullException(nameof(module));
        var sources = cfdSources
            .Select((text, index) => new CfdSource($"source[{index}]", text ??
                throw new ArgumentException("CFD source text cannot be null.", nameof(cfdSources))))
            .ToArray();
        try
        {
            return CoflowData.Load(CfdParser.ParseAll(sources), module, functionsCompiled);
        }
        catch (CoflowLoadException)
        {
            throw;
        }
        catch (CfdLoadException error)
        {
            throw new CoflowLoadException(error.Diagnostics);
        }
    }
}

public sealed class CoflowLoadException : CfdLoadException
{
    public CoflowLoadException(IReadOnlyList<CfdDiagnostic> diagnostics) : base(diagnostics) { }
}

public readonly record struct ReloadInfo(long PreviousGeneration, long Generation);

public sealed class CoflowReloadError
{
    public CoflowReloadError(IReadOnlyList<CfdDiagnostic> diagnostics) =>
        Diagnostics = diagnostics ?? throw new ArgumentNullException(nameof(diagnostics));

    public IReadOnlyList<CfdDiagnostic> Diagnostics { get; }
}

public sealed class CoflowData : IDisposable
{
    private readonly object _sync = new();
    private IReadOnlyDictionary<Type, object> _tables;
    private IReadOnlyDictionary<Type, object> _singletons;
    private IReadOnlyList<CoflowFunctionSlot> _functions;
    private CoflowGenerationStorage _storage;
    private readonly ICoflowGeneratedModule _module;
    private long _generation = 1;
    private bool _disposed;

    private CoflowData(
        IReadOnlyDictionary<Type, object> tables,
        IReadOnlyDictionary<Type, object> singletons,
        IReadOnlyList<CoflowFunctionSlot> functions,
        CoflowGenerationStorage storage,
        bool functionsCompiled,
        ICoflowGeneratedModule module)
    {
        _tables = tables;
        _singletons = singletons;
        _functions = functions;
        _storage = storage;
        _module = module;
        FunctionsCompiled = functionsCompiled;
    }

    public bool FunctionsCompiled { get; }

    internal CoflowGenerationStorage Storage => _storage;

    internal static CoflowData Load(
        IReadOnlyList<CfdDocument> documents,
        ICoflowGeneratedModule module,
        bool functionsCompiled)
    {
        var duplicateRuntimeType = module.Types
            .GroupBy(type => type.RuntimeType)
            .FirstOrDefault(group => group.Count() > 1);
        if (duplicateRuntimeType is not null)
            throw new CoflowLoadException(new[] { new CfdDiagnostic(
                "COFLOW-METADATA-DUPLICATE-TYPE",
                $"generated type `{duplicateRuntimeType.Key}` is registered more than once",
                string.Empty) });

        var context = new CfdLoadContext(
            documents,
            bindings: module.Types,
            enums: module.Enums,
            constants: module.Constants,
            functionsCompiled: functionsCompiled);
        documents = context.Documents;
        var metadataByName = module.Types.ToDictionary(type => type.DeclaredType, StringComparer.Ordinal);
        foreach (var document in documents)
        {
            foreach (var record in document.Records)
            {
                if (!metadataByName.TryGetValue(record.DeclaredType, out var recordMetadata))
                    throw new CoflowLoadException(new[] { new CfdDiagnostic(
                        "CFD-REF-UNKNOWN-TYPE",
                        $"CFD record `{record.Key}` uses unknown type `{record.DeclaredType}`",
                        document.Path,
                        record.Span) });
                if (recordMetadata.IsHost)
                    throw new CoflowLoadException(new[] { new CfdDiagnostic(
                        "CFD-HOST-RECORD",
                        $"CFD cannot declare @Host type `{record.DeclaredType}`",
                        document.Path,
                        record.Span) });
            }
        }
        var tables = new Dictionary<Type, object>();
        var singletons = new Dictionary<Type, object>();
        var recordsByIdentity = new Dictionary<(string DeclaredType, string Key), object>();
        foreach (var metadata in module.Types)
        {
            if (metadata.IsHost)
            {
                var host = metadata.CreateHost(context);
                singletons.Add(metadata.RuntimeType, host);
                recordsByIdentity.Add((metadata.DeclaredType, string.Empty), host);
                continue;
            }
            var records = documents.SelectMany(document => document.Records)
                .Where(record => string.Equals(record.DeclaredType, metadata.DeclaredType, StringComparison.Ordinal))
                .ToArray();
            if (metadata.IsSingleton)
            {
                if (records.Length != 1)
                    throw new CoflowLoadException(new[] { new CfdDiagnostic(
                        "CFD-SINGLETON-COUNT",
                        $"singleton `{metadata.DeclaredType}` must have exactly one record",
                        string.Empty) });
                var singleton = context.Resolve<object>(metadata.DeclaredType, records[0].Key);
                singletons.Add(metadata.RuntimeType, singleton);
                recordsByIdentity.Add((metadata.DeclaredType, records[0].Key), singleton);
                continue;
            }

            var values = records.Select(record => context.Resolve<object>(metadata.DeclaredType, record.Key)).ToArray();
            for (var index = 0; index < records.Length; index++)
                recordsByIdentity.Add((metadata.DeclaredType, records[index].Key), values[index]);
        }
        foreach (var metadata in module.Types.Where(type => !type.IsHost && !type.IsSingleton))
        {
            var values = documents.SelectMany(document => document.Records)
                .Where(record => metadataByName[record.DeclaredType].AssignableTypes
                    .Contains(metadata.DeclaredType, StringComparer.Ordinal))
                .Select(record => recordsByIdentity[(record.DeclaredType, record.Key)])
                .ToArray();
            tables.Add(metadata.RuntimeType, CreateTable(metadata, values));
        }
        var storage = CoflowGenerationStorage.Build(module.Types, recordsByIdentity, context.Functions);
        if (functionsCompiled)
            CoflowCompiler.Compile(context.Functions, module, recordsByIdentity, context, storage);
        return new CoflowData(tables, singletons, context.Functions, storage, functionsCompiled, module);
    }

    private static object CreateTable(ICoflowTypeMetadata metadata, object[] values)
    {
        var tableType = typeof(CoflowTable<>).MakeGenericType(metadata.RuntimeType);
        return Activator.CreateInstance(tableType, metadata, values)!;
    }

    public CoflowTable<T> Table<T>()
    {
        lock (_sync)
        {
            ThrowIfDisposed();
            return _tables.TryGetValue(typeof(T), out var table)
                ? (CoflowTable<T>)table
                : throw new InvalidOperationException($"`{typeof(T)}` is not a generated non-singleton Coflow type.");
        }
    }

    public T Singleton<T>()
    {
        lock (_sync)
        {
            ThrowIfDisposed();
            return _singletons.TryGetValue(typeof(T), out var singleton)
                ? (T)singleton
                : throw new InvalidOperationException($"`{typeof(T)}` is not a loaded generated singleton Coflow type.");
        }
    }

    public Result<ReloadInfo, CoflowReloadError> Reload(string cfd) => Reload(new[] { cfd });

    public Result<ReloadInfo, CoflowReloadError> Reload(string[] cfdSources)
    {
        if (cfdSources is null) throw new ArgumentNullException(nameof(cfdSources));
        IReadOnlyDictionary<Type, object> previousHosts;
        IReadOnlyList<CoflowFunctionSlot> previousFunctions;
        long previousGeneration;
        lock (_sync)
        {
            ThrowIfDisposed();
            previousHosts = _module.Types.Where(type => type.IsHost)
                .ToDictionary(type => type.RuntimeType, type => _singletons[type.RuntimeType]);
            previousFunctions = _functions;
            previousGeneration = _generation;
        }

        CoflowData candidate;
        try
        {
            var sources = cfdSources.Select((text, index) => new CfdSource(
                $"source[{index}]", text ?? throw new ArgumentException(
                    "CFD source text cannot be null.", nameof(cfdSources)))).ToArray();
            candidate = Load(CfdParser.ParseAll(sources), _module, FunctionsCompiled);
        }
        catch (CfdLoadException error)
        {
            return Result<ReloadInfo, CoflowReloadError>.Err(new CoflowReloadError(error.Diagnostics));
        }

        var candidateByIdentity = candidate._functions.ToDictionary(function => function.Identity);
        foreach (var previous in previousFunctions.Where(function => function.HasBoundImplementation))
        {
            if (!candidateByIdentity.TryGetValue(previous.Identity, out var target) ||
                !previous.TransferBindingTo(target))
            {
                return Result<ReloadInfo, CoflowReloadError>.Err(new CoflowReloadError(new[]
                {
                    new CfdDiagnostic("COFLOW-RELOAD-BINDING",
                        $"bound function `{previous.Identity.DeclaredType}.{previous.Identity.RecordKey}.{previous.Identity.FieldName}` cannot be reused by the candidate generation",
                        string.Empty),
                }));
            }
        }

        foreach (var metadata in _module.Types.Where(type => type.IsHost))
        {
            metadata.TransferHostState(
                previousHosts[metadata.RuntimeType],
                candidate._singletons[metadata.RuntimeType]);
        }

        lock (_sync)
        {
            ThrowIfDisposed();
            if (_generation != previousGeneration)
                return Result<ReloadInfo, CoflowReloadError>.Err(new CoflowReloadError(new[]
                {
                    new CfdDiagnostic("COFLOW-RELOAD-CONCURRENT",
                        "the active generation changed while reload was being prepared", string.Empty),
                }));
            _tables = candidate._tables;
            _singletons = candidate._singletons;
            _functions = candidate._functions;
            _storage = candidate._storage;
            _generation++;
            return Result<ReloadInfo, CoflowReloadError>.Ok(
                new ReloadInfo(previousGeneration, _generation));
        }
    }

    public void Dispose()
    {
        lock (_sync) _disposed = true;
    }

    private void ThrowIfDisposed()
    {
        if (_disposed) throw new ObjectDisposedException(nameof(CoflowData));
    }
}

public sealed class CoflowTable<T> : IReadOnlyList<T>
{
    private readonly T[] _records;
    private readonly IReadOnlyDictionary<object, T> _index;
    private readonly ICoflowTypeMetadata _metadata;

    [EditorBrowsable(EditorBrowsableState.Never)]
    public CoflowTable(ICoflowTypeMetadata metadata, object[] records)
    {
        _metadata = metadata;
        _records = records.Cast<T>().ToArray();
        var index = new Dictionary<object, T>();
        foreach (var record in _records)
        {
            var key = metadata.GetKey(record!);
            if (!index.TryAdd(key, record))
                throw new CoflowLoadException(new[] { new CfdDiagnostic(
                    "CFD-SYNTAX-DUPLICATE-RECORD",
                    $"record key `{key}` is declared more than once for `{metadata.DeclaredType}`",
                    string.Empty) });
        }
        _index = index;
    }

    public int Count => _records.Length;
    public T this[int index] => _records[index];

    public Option<T> Get(string key)
    {
        if (_metadata.KeyType != typeof(string))
            throw new ArgumentException($"`{_metadata.DeclaredType}` uses enum key `{_metadata.KeyType}`.", nameof(key));
        return Find(key);
    }

    public Option<T> Get<TKey>(TKey key) where TKey : struct, Enum
    {
        if (_metadata.KeyType != typeof(TKey))
            throw new ArgumentException($"`{_metadata.DeclaredType}` does not use enum key `{typeof(TKey)}`.", nameof(key));
        return Find(key);
    }

    private Option<T> Find(object key) => _index.TryGetValue(key, out var value)
        ? Option<T>.Some(value)
        : Option<T>.None;

    public IEnumerator<T> GetEnumerator() => ((IEnumerable<T>)_records).GetEnumerator();
    IEnumerator IEnumerable.GetEnumerator() => GetEnumerator();
}
