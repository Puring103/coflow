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
    private static readonly CoflowGeneratedContractRegistry Registry = new();

    public static void Register(ICoflowGeneratedContract contract) => Registry.Register(contract);

    internal static ICoflowGeneratedContract RequireContract() => Registry.RequireContract();
}

internal sealed class CoflowGeneratedContractRegistry
{
    private readonly object _sync = new();
    private readonly List<ICoflowGeneratedContract> _registrations = new();
    private ICoflowGeneratedContract? _resolvedContract;

    internal void Register(ICoflowGeneratedContract contract)
    {
        if (contract is null) throw new ArgumentNullException(nameof(contract));
        lock (_sync)
        {
            if (_registrations.Any(existing => ReferenceEquals(existing, contract))) return;
            if (_resolvedContract is not null)
                throw new InvalidOperationException(
                    "Generated Coflow contracts cannot be registered after the first runtime load.");
            _registrations.Add(contract);
        }
    }

    internal ICoflowGeneratedContract RequireContract()
    {
        lock (_sync)
        {
            if (_resolvedContract is not null) return _resolvedContract;
            if (_registrations.Count == 0)
                throw new InvalidOperationException(
                    "No generated Coflow contract is registered. Reference the C# sources generated from the project CFT.");
            return _resolvedContract = _registrations.Count == 1
                ? _registrations[0]
                : new CoflowCompositeGeneratedContract(_registrations);
        }
    }
}

internal sealed class CoflowCompositeGeneratedContract : ICoflowGeneratedContract
{
    internal CoflowCompositeGeneratedContract(IEnumerable<ICoflowGeneratedContract> contracts)
    {
        var values = contracts?.ToArray() ?? throw new ArgumentNullException(nameof(contracts));
        if (values.Length == 0)
            throw new ArgumentException("At least one generated contract is required.", nameof(contracts));
        Types = values.SelectMany(contract => contract.Types).ToArray();
        Enums = values.SelectMany(contract => contract.Enums).ToArray();
        Constants = values.SelectMany(contract => contract.Constants).ToArray();
    }

    public IReadOnlyList<ICoflowTypeMetadata> Types { get; }
    public IReadOnlyList<ICoflowEnumMetadata> Enums { get; }
    public IReadOnlyList<CoflowConstant> Constants { get; }
}

internal static class CoflowLoader
{
    public static CoflowModule LoadData(string cfd) => LoadData(new[] { cfd });

    public static CoflowModule LoadData(string[] cfdSources) =>
        Load(cfdSources, Array.Empty<CoflowModule>(),
            CoflowGeneratedRegistry.RequireContract(), functionsCompiled: false);

    public static CoflowModule LoadData(string[] cfdSources, CoflowModule[] children) =>
        Load(cfdSources, children, CoflowGeneratedRegistry.RequireContract(), functionsCompiled: false);

    public static CoflowModule LoadAndCompile(string cfd) => LoadAndCompile(new[] { cfd });

    public static CoflowModule LoadAndCompile(string[] cfdSources) =>
        Load(cfdSources, Array.Empty<CoflowModule>(),
            CoflowGeneratedRegistry.RequireContract(), functionsCompiled: true);

    public static CoflowModule LoadAndCompile(string[] cfdSources, CoflowModule[] children) =>
        Load(cfdSources, children, CoflowGeneratedRegistry.RequireContract(), functionsCompiled: true);

    internal static CoflowModule LoadData(string[] cfdSources, ICoflowGeneratedContract contract)
        => Load(cfdSources, Array.Empty<CoflowModule>(), contract, functionsCompiled: false);

    internal static CoflowModule LoadAndCompile(string[] cfdSources, ICoflowGeneratedContract contract)
        => Load(cfdSources, Array.Empty<CoflowModule>(), contract, functionsCompiled: true);

    private static CoflowModule Load(
        string[] cfdSources,
        IReadOnlyList<CoflowModule> children,
        ICoflowGeneratedContract contract,
        bool functionsCompiled)
    {
        if (cfdSources is null) throw new ArgumentNullException(nameof(cfdSources));
        if (contract is null) throw new ArgumentNullException(nameof(contract));
        var sourcePart = CoflowSourcePart.Create(cfdSources);
        var sourceParts = new List<CoflowSourcePart>();
        var sourceIds = new HashSet<Guid>();
        var sourceOwners = new HashSet<Guid>();
        foreach (var child in children ?? throw new ArgumentNullException(nameof(children)))
        {
            if (child is null) throw new ArgumentException("A child CFD module cannot be null.", nameof(children));
            var childParts = child.SnapshotSourceParts(contract);
            var childOwners = childParts.SelectMany(part => part.Owners).ToHashSet();
            if (sourceOwners.Overlaps(childOwners))
                throw new ArgumentException("The same child CFD module is included more than once.", nameof(children));
            sourceOwners.UnionWith(childOwners);
            foreach (var childPart in childParts)
            {
                if (!sourceIds.Add(childPart.Id))
                    throw new ArgumentException("The same child CFD module is included more than once.", nameof(children));
                sourceParts.Add(childPart);
            }
        }
        sourceParts.Add(sourcePart);
        try
        {
            return CoflowModule.Load(sourceParts, contract, functionsCompiled);
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

internal sealed record CoflowSourcePart(Guid Id, string[] Sources, IReadOnlyList<Guid> Owners)
{
    internal static CoflowSourcePart Create(string[] sources)
    {
        if (sources is null) throw new ArgumentNullException(nameof(sources));
        return new CoflowSourcePart(Guid.NewGuid(), sources.Select(source => source ??
            throw new ArgumentException("CFD source text cannot be null.", nameof(sources))).ToArray(),
            Array.Empty<Guid>());
    }

    internal CoflowSourcePart WithOwner(Guid owner) => Owners.Contains(owner)
        ? this
        : this with { Owners = Owners.Append(owner).ToArray() };
}

public sealed class CoflowModule : IDisposable
{
    private readonly object _sync = new();
    private IReadOnlyDictionary<Type, object> _tables;
    private IReadOnlyDictionary<Type, object> _singletons;
    private IReadOnlyList<CoflowFunctionSlot> _functions;
    private CoflowGenerationStorage? _storage;
    private CoflowGenerationGate _generationGate;
    private readonly ICoflowGeneratedContract _contract;
    private readonly Guid _identity = Guid.NewGuid();
    private IReadOnlyList<CoflowSourcePart> _sourceParts;
    private long _generation = 1;
    private bool _disposed;

    private CoflowModule(
        IReadOnlyDictionary<Type, object> tables,
        IReadOnlyDictionary<Type, object> singletons,
        IReadOnlyList<CoflowFunctionSlot> functions,
        CoflowGenerationStorage storage,
        CoflowGenerationGate generationGate,
        bool functionsCompiled,
        ICoflowGeneratedContract contract,
        IReadOnlyList<CoflowSourcePart> sourceParts)
    {
        _tables = tables;
        _singletons = singletons;
        _functions = functions;
        _storage = storage;
        _generationGate = generationGate;
        _contract = contract;
        _sourceParts = sourceParts;
        FunctionsCompiled = functionsCompiled;
    }

    public bool FunctionsCompiled { get; }

    internal CoflowGenerationStorage Storage => _storage ??
        throw new ObjectDisposedException(nameof(CoflowModule));

    internal static CoflowModule Load(
        IReadOnlyList<CoflowSourcePart> sourceParts,
        ICoflowGeneratedContract contract,
        bool functionsCompiled)
    {
        var documents = ParseSources(sourceParts);
        ValidateContract(contract);
        var generationGate = new CoflowGenerationGate();
        var context = new CfdLoadContext(
            documents,
            bindings: contract.Types,
            enums: contract.Enums,
            constants: contract.Constants,
            functionsCompiled: functionsCompiled);
        context.GenerationGate = generationGate;
        documents = context.Documents;
        var metadataByName = contract.Types.ToDictionary(type => type.DeclaredType, StringComparer.Ordinal);
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
        foreach (var metadata in contract.Types)
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
        foreach (var metadata in contract.Types.Where(type => !type.IsHost && !type.IsSingleton))
        {
            var values = documents.SelectMany(document => document.Records)
                .Where(record => metadataByName[record.DeclaredType].AssignableTypes
                    .Contains(metadata.DeclaredType, StringComparer.Ordinal))
                .Select(record => recordsByIdentity[(record.DeclaredType, record.Key)])
                .ToArray();
            tables.Add(metadata.RuntimeType, CreateTable(metadata, values));
        }
        var storage = CoflowGenerationStorage.Build(contract.Types, recordsByIdentity, context.Functions);
        if (functionsCompiled)
            CoflowCompiler.Compile(context.Functions, contract, recordsByIdentity, context, storage);
        return new CoflowModule(
            tables, singletons, context.Functions, storage, generationGate,
            functionsCompiled, contract, sourceParts);
    }

    private static void ValidateContract(ICoflowGeneratedContract contract)
    {
        var duplicateDeclaredType = contract.Types.Select(type => type.DeclaredType)
            .Concat(contract.Enums.Select(value => value.DeclaredType))
            .GroupBy(name => name, StringComparer.Ordinal)
            .FirstOrDefault(group => group.Count() > 1);
        if (duplicateDeclaredType is not null)
            throw ContractError("COFLOW-METADATA-DUPLICATE-NAME",
                $"generated name `{duplicateDeclaredType.Key}` is registered more than once");

        var duplicateRuntimeType = contract.Types.Select(type => type.RuntimeType)
            .Concat(contract.Enums.Select(value => value.RuntimeType))
            .GroupBy(type => type)
            .FirstOrDefault(group => group.Count() > 1);
        if (duplicateRuntimeType is not null)
            throw ContractError("COFLOW-METADATA-DUPLICATE-TYPE",
                $"generated runtime type `{duplicateRuntimeType.Key}` is registered more than once");

        var duplicateConstant = contract.Constants.GroupBy(
                constant => constant.DeclaredName, StringComparer.Ordinal)
            .FirstOrDefault(group => group.Count() > 1);
        if (duplicateConstant is not null)
            throw ContractError("COFLOW-METADATA-DUPLICATE-CONSTANT",
                $"generated constant `{duplicateConstant.Key}` is registered more than once");
    }

    private static CoflowLoadException ContractError(string code, string message) =>
        new(new[] { new CfdDiagnostic(code, message, string.Empty) });

    private static IReadOnlyList<CfdDocument> ParseSources(IReadOnlyList<CoflowSourcePart> sourceParts)
    {
        var combined = sourceParts.Count > 1;
        var sources = sourceParts.SelectMany((part, partIndex) => part.Sources.Select((text, sourceIndex) =>
            new CfdSource(combined ? $"module[{partIndex}]/source[{sourceIndex}]" : $"source[{sourceIndex}]", text)));
        return CfdParser.ParseAll(sources);
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

    internal static CoflowModule Combine(IReadOnlyList<CoflowModule> modules)
    {
        if (modules is null) throw new ArgumentNullException(nameof(modules));
        if (modules.Count == 0)
            throw new ArgumentException("At least one CFD module is required.", nameof(modules));

        ICoflowGeneratedContract? contract = null;
        var compiled = true;
        var sourceParts = new List<CoflowSourcePart>();
        var sourceIds = new HashSet<Guid>();
        var sourceOwners = new HashSet<Guid>();
        foreach (var module in modules)
        {
            if (module is null) throw new ArgumentException("A CFD module cannot be null.", nameof(modules));
            lock (module._sync)
            {
                module.ThrowIfDisposed();
                contract ??= module._contract;
                if (!ReferenceEquals(contract, module._contract))
                    throw new ArgumentException("CFD modules use different generated contracts.", nameof(modules));
                compiled &= module.FunctionsCompiled;
                var moduleParts = module._sourceParts
                    .Select(sourcePart => sourcePart.WithOwner(module._identity))
                    .ToArray();
                var moduleOwners = moduleParts.SelectMany(part => part.Owners).ToHashSet();
                if (sourceOwners.Overlaps(moduleOwners))
                    throw new ArgumentException("The same CFD module is included more than once.", nameof(modules));
                sourceOwners.UnionWith(moduleOwners);
                foreach (var sourcePart in moduleParts)
                {
                    if (!sourceIds.Add(sourcePart.Id))
                        throw new ArgumentException("The same CFD module is included more than once.", nameof(modules));
                    sourceParts.Add(sourcePart);
                }
            }
        }
        return Load(sourceParts, contract!, compiled);
    }

    internal IReadOnlyList<CoflowSourcePart> SnapshotSourceParts(ICoflowGeneratedContract contract)
    {
        lock (_sync)
        {
            ThrowIfDisposed();
            if (!ReferenceEquals(_contract, contract))
                throw new ArgumentException("The child CFD module uses a different generated contract.");
            return _sourceParts.Select(part => part.WithOwner(_identity)).ToArray();
        }
    }

    public CoflowModule Compile()
    {
        lock (_sync)
        {
            ThrowIfDisposed();
            return Load(_sourceParts, _contract, functionsCompiled: true);
        }
    }

    public Result<ReloadInfo, CoflowReloadError> Reload(string cfd) => Reload(new[] { cfd });

    public Result<ReloadInfo, CoflowReloadError> Reload(string[] cfdSources)
    {
        if (cfdSources is null) throw new ArgumentNullException(nameof(cfdSources));
        IReadOnlyList<CoflowSourcePart> replacement;
        lock (_sync)
        {
            ThrowIfDisposed();
            var identity = _sourceParts.Count == 1 ? _sourceParts[0].Id : Guid.NewGuid();
            replacement = new[] { new CoflowSourcePart(
                identity, ValidateSources(cfdSources), Array.Empty<Guid>()) };
        }
        return ReloadParts(replacement);
    }

    public Result<ReloadInfo, CoflowReloadError> Reload(CoflowModule child, string cfd) =>
        Reload(child, new[] { cfd });

    public Result<ReloadInfo, CoflowReloadError> Reload(CoflowModule child, string[] cfdSources)
    {
        if (child is null) throw new ArgumentNullException(nameof(child));
        if (cfdSources is null) throw new ArgumentNullException(nameof(cfdSources));
        Guid childId;
        lock (child._sync)
        {
            child.ThrowIfDisposed();
            if (!ReferenceEquals(_contract, child._contract))
                throw new ArgumentException("The child CFD module uses a different generated contract.", nameof(child));
            childId = child._identity;
        }

        IReadOnlyList<CoflowSourcePart> replacement;
        lock (_sync)
        {
            ThrowIfDisposed();
            var matched = _sourceParts.Where(part => part.Owners.Contains(childId)).ToArray();
            if (matched.Length == 0)
                throw new ArgumentException("The target is not a child of this CFD module.", nameof(child));
            var preservedOwners = matched[0].Owners
                .Where(owner => matched.All(part => part.Owners.Contains(owner)))
                .ToArray();
            var replacementPart = new CoflowSourcePart(
                Guid.NewGuid(), ValidateSources(cfdSources), preservedOwners);
            var replaced = false;
            replacement = _sourceParts.SelectMany(part =>
            {
                if (!part.Owners.Contains(childId)) return new[] { part };
                if (replaced) return Array.Empty<CoflowSourcePart>();
                replaced = true;
                return new[] { replacementPart };
            }).ToArray();
        }
        return ReloadParts(replacement);
    }

    private Result<ReloadInfo, CoflowReloadError> ReloadParts(IReadOnlyList<CoflowSourcePart> sourceParts)
    {
        long previousGeneration;
        CoflowGenerationGate previousGate;
        lock (_sync)
        {
            ThrowIfDisposed();
            previousGeneration = _generation;
            previousGate = _generationGate;
        }

        CoflowModule candidate;
        try
        {
            candidate = Load(sourceParts, _contract, FunctionsCompiled);
        }
        catch (CfdLoadException error)
        {
            return Result<ReloadInfo, CoflowReloadError>.Err(new CoflowReloadError(error.Diagnostics));
        }

        lock (previousGate.Sync)
        {
            lock (_sync)
            {
                if (_disposed)
                {
                    candidate.Dispose();
                    throw new ObjectDisposedException(nameof(CoflowModule));
                }
                if (_generation != previousGeneration || !ReferenceEquals(_generationGate, previousGate))
                {
                    candidate.Dispose();
                    return Result<ReloadInfo, CoflowReloadError>.Err(new CoflowReloadError(new[]
                    {
                        new CfdDiagnostic("COFLOW-RELOAD-CONCURRENT",
                            "the active generation changed while reload was being prepared", string.Empty),
                    }));
                }

                try
                {
                    foreach (var metadata in _contract.Types.Where(type => type.IsHost))
                    {
                        metadata.TransferHostState(
                            _singletons[metadata.RuntimeType],
                            candidate._singletons[metadata.RuntimeType]);
                    }
                }
                catch (InvalidOperationException error)
                {
                    candidate.Dispose();
                    return Result<ReloadInfo, CoflowReloadError>.Err(new CoflowReloadError(new[]
                    {
                        new CfdDiagnostic("COFLOW-RELOAD-BINDING", error.Message, string.Empty),
                    }));
                }

                previousGate.Retire();
                _tables = candidate._tables;
                _singletons = candidate._singletons;
                _functions = candidate._functions;
                _storage = candidate._storage;
                _generationGate = candidate._generationGate;
                _sourceParts = sourceParts;
                _generation++;
                return Result<ReloadInfo, CoflowReloadError>.Ok(
                    new ReloadInfo(previousGeneration, _generation));
            }
        }
    }

    public void Dispose()
    {
        while (true)
        {
            CoflowGenerationGate gate;
            lock (_sync)
            {
                if (_disposed) return;
                gate = _generationGate;
            }
            lock (gate.Sync)
            {
                lock (_sync)
                {
                    if (_disposed) return;
                    if (!ReferenceEquals(_generationGate, gate)) continue;
                    gate.Retire();
                    _disposed = true;
                    _tables = new Dictionary<Type, object>();
                    _singletons = new Dictionary<Type, object>();
                    _functions = Array.Empty<CoflowFunctionSlot>();
                    _storage = null;
                    _sourceParts = Array.Empty<CoflowSourcePart>();
                    return;
                }
            }
        }
    }

    private static string[] ValidateSources(string[] sources) => sources.Select(source => source ??
        throw new ArgumentException("CFD source text cannot be null.", nameof(sources))).ToArray();

    private void ThrowIfDisposed()
    {
        if (_disposed) throw new ObjectDisposedException(nameof(CoflowModule));
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
