namespace Coflow.Runtime;

using global::Coflow.Runtime.CompilerServices;

public sealed class Coflow
{
    private static int _nextSnapshotId;
    private readonly ICoflowSchema _schema;
    private readonly CoflowOptions _options;
    private readonly List<CoflowModule> _modules = new();
    private readonly Dictionary<Type, object> _hostBindings = new();
    private CoflowSnapshot? _published;
    private long _nextModuleId;
    private uint _generation;
    private int _executionDepth;

    private Coflow(ICoflowSchema schema, CoflowOptions options)
    {
        _schema = schema ?? throw new ArgumentNullException(nameof(schema));
        _options = options ?? throw new ArgumentNullException(nameof(options));
    }

    public static Coflow Create(ICoflowSchema schema, CoflowOptions? options = null) =>
        new(schema, options ?? CoflowOptions.Default);

    public CoflowModule LoadModule(params CoflowSource[] sources) =>
        LoadModule((IEnumerable<CoflowSource>)sources);

    public CoflowModule LoadModule(IEnumerable<CoflowSource> sources)
    {
        EnsureMutable();
        var module = new CoflowModule(this, checked(++_nextModuleId), MaterializeSources(sources));
        _modules.Add(module);
        return module;
    }

    public void ReplaceModule(CoflowModule module, params CoflowSource[] sources) =>
        ReplaceModule(module, (IEnumerable<CoflowSource>)sources);

    public void ReplaceModule(CoflowModule module, IEnumerable<CoflowSource> sources)
    {
        EnsureMutable();
        ValidateModule(module);
        module.Sources = MaterializeSources(sources);
        module.InvalidateUnit();
    }

    public void RemoveModule(CoflowModule module)
    {
        EnsureMutable();
        ValidateModule(module);
        _modules.Remove(module);
        module.Removed = true;
    }

    public void Bind<THost>(THost host) where THost : class
    {
        EnsureMutable();
        if (host is null) throw new ArgumentNullException(nameof(host));
        _hostBindings[typeof(THost)] = host;
    }

    public CoflowCompileResult Compile()
    {
        EnsureMutable();
        try
        {
            // 候选快照独占编译期补全的 closed type 布局，编译失败时随候选整体丢弃。
            var layouts = new CoflowLayoutRegistry();
            using var layoutCompilation = CoflowLayoutCompilation.Enter(layouts);
            // Module 只定义管理边界；解析和链接必须看到同一个全局源码集合。
            var documents = _modules.SelectMany(module => module.Documents()).ToArray();
            var generation = checked(_generation + 1);
            var snapshotId = unchecked((uint)System.Threading.Interlocked.Increment(ref _nextSnapshotId));
            if (snapshotId == 0) throw new InvalidOperationException("Coflow snapshot identity space is exhausted.");
            var candidate = CoflowSnapshot.Build(
                documents, _schema, _hostBindings, _modules.ToDictionary(module => module.Id),
                layouts, _options, generation, snapshotId);
            _published = candidate;
            _generation = generation;
            return CoflowCompileResult.Published(generation);
        }
        catch (CfdLoadException error)
        {
            return CoflowCompileResult.Failed(error.Diagnostics);
        }
    }

    public TTable Table<TTable>(ICoflowTableToken<TTable> token) where TTable : CoflowTable =>
        Snapshot.Table(token);

    public Option<T> Singleton<T>() where T : class => Snapshot.Singleton<T>();

    internal CoflowSnapshot Snapshot => _published ?? throw new CoflowNotCompiledException();
    internal CoflowOptions Options => _options;
    internal bool IsExecuting => _executionDepth != 0;

    internal TResult InvokeFunction<TResult>(CoflowFunctionId functionId, CoflowValueId environmentId)
    { using var scope = EnterExecution(); return Snapshot.Function(functionId, environmentId).Invoke<TResult>(); }
    internal TResult InvokeFunction<T1, TResult>(CoflowFunctionId functionId, CoflowValueId environmentId, T1 a1)
    { using var scope = EnterExecution(); return Snapshot.Function(functionId, environmentId).Invoke<T1, TResult>(a1); }
    internal TResult InvokeFunction<T1, T2, TResult>(CoflowFunctionId functionId, CoflowValueId environmentId, T1 a1, T2 a2)
    { using var scope = EnterExecution(); return Snapshot.Function(functionId, environmentId).Invoke<T1, T2, TResult>(a1, a2); }
    internal TResult InvokeFunction<T1, T2, T3, TResult>(CoflowFunctionId functionId, CoflowValueId environmentId, T1 a1, T2 a2, T3 a3)
    { using var scope = EnterExecution(); return Snapshot.Function(functionId, environmentId).Invoke<T1, T2, T3, TResult>(a1, a2, a3); }
    internal TResult InvokeFunction<T1, T2, T3, T4, TResult>(CoflowFunctionId functionId, CoflowValueId environmentId, T1 a1, T2 a2, T3 a3, T4 a4)
    { using var scope = EnterExecution(); return Snapshot.Function(functionId, environmentId).Invoke<T1, T2, T3, T4, TResult>(a1, a2, a3, a4); }
    internal TResult InvokeFunction<T1, T2, T3, T4, T5, TResult>(CoflowFunctionId functionId, CoflowValueId environmentId, T1 a1, T2 a2, T3 a3, T4 a4, T5 a5)
    { using var scope = EnterExecution(); return Snapshot.Function(functionId, environmentId).Invoke<T1, T2, T3, T4, T5, TResult>(a1, a2, a3, a4, a5); }
    internal TResult InvokeFunction<T1, T2, T3, T4, T5, T6, TResult>(CoflowFunctionId functionId, CoflowValueId environmentId, T1 a1, T2 a2, T3 a3, T4 a4, T5 a5, T6 a6)
    { using var scope = EnterExecution(); return Snapshot.Function(functionId, environmentId).Invoke<T1, T2, T3, T4, T5, T6, TResult>(a1, a2, a3, a4, a5, a6); }
    internal TResult InvokeFunction<T1, T2, T3, T4, T5, T6, T7, TResult>(CoflowFunctionId functionId, CoflowValueId environmentId, T1 a1, T2 a2, T3 a3, T4 a4, T5 a5, T6 a6, T7 a7)
    { using var scope = EnterExecution(); return Snapshot.Function(functionId, environmentId).Invoke<T1, T2, T3, T4, T5, T6, T7, TResult>(a1, a2, a3, a4, a5, a6, a7); }
    internal TResult InvokeFunction<T1, T2, T3, T4, T5, T6, T7, T8, TResult>(CoflowFunctionId functionId, CoflowValueId environmentId, T1 a1, T2 a2, T3 a3, T4 a4, T5 a5, T6 a6, T7 a7, T8 a8)
    { using var scope = EnterExecution(); return Snapshot.Function(functionId, environmentId).Invoke<T1, T2, T3, T4, T5, T6, T7, T8, TResult>(a1, a2, a3, a4, a5, a6, a7, a8); }

    internal ExecutionScope EnterExecution()
    {
        var invocation = CoflowInvocationContext.Enter(this, Snapshot);
        _executionDepth++;
        return new ExecutionScope(this, invocation);
    }

    private void ExitExecution() => _executionDepth--;

    private void EnsureMutable()
    {
        if (_executionDepth != 0) throw new InvalidOperationException("Coflow cannot be modified while a function is executing.");
    }

    private void ValidateModule(CoflowModule module)
    {
        if (module is null) throw new ArgumentNullException(nameof(module));
        if (!ReferenceEquals(module.Owner, this) || module.Removed || !_modules.Contains(module))
            throw new ArgumentException("The module does not belong to this Coflow.", nameof(module));
    }

    private static CoflowSource[] MaterializeSources(IEnumerable<CoflowSource> sources)
    {
        if (sources is null) throw new ArgumentNullException(nameof(sources));
        var result = sources.ToArray();
        if (result.Any(source => source.Path is null || source.Text is null))
            throw new ArgumentException("Module sources must contain a path and text.", nameof(sources));
        return result;
    }

    internal readonly struct ExecutionScope : IDisposable
    {
        private readonly Coflow _owner;
        private readonly CoflowInvocationContext.Scope _invocation;
        internal ExecutionScope(Coflow owner, CoflowInvocationContext.Scope invocation)
        {
            _owner = owner;
            _invocation = invocation;
        }
        public void Dispose()
        {
            _invocation.Dispose();
            _owner.ExitExecution();
        }
    }
}

public sealed class CoflowModule
{
    private CfdDocument[]? _documents;
    private readonly Dictionary<CoflowFunctionIdentity, CoflowProgramTemplate> _templates = new();

    internal CoflowModule(Coflow owner, long id, CoflowSource[] sources)
    {
        Owner = owner;
        Id = id;
        Sources = sources;
    }

    internal Coflow Owner { get; }
    internal long Id { get; }
    internal CoflowSource[] Sources { get; set; }
    internal bool Removed { get; set; }
    internal int ParseCount { get; private set; }
    internal int TemplateCompileCount { get; private set; }

    internal IReadOnlyList<CfdDocument> Documents()
    {
        if (_documents is not null) return _documents;
        var sources = Sources.Select(source => new CfdSource(source.Path, source.Text));
        _documents = CfdParser.ParseAll(sources)
            .Select(document => new CfdDocument(document.Path, document.Records, Id))
            .ToArray();
        ParseCount++;
        return _documents;
    }

    internal bool TryGetTemplate(
        CoflowFunctionIdentity identity,
        out CoflowProgramTemplate template) => _templates.TryGetValue(identity, out template!);

    internal void PublishTemplate(CoflowFunctionIdentity identity, CoflowProgramTemplate template)
    {
        _templates[identity] = template;
        TemplateCompileCount++;
    }

    internal void InvalidateUnit()
    {
        _documents = null;
        _templates.Clear();
    }
}

public readonly struct CoflowSource
{
    public CoflowSource(string path, string text)
    {
        Path = path ?? throw new ArgumentNullException(nameof(path));
        Text = text ?? throw new ArgumentNullException(nameof(text));
    }

    public string Path { get; }
    public string Text { get; }
}

public sealed class CoflowCompileResult
{
    private CoflowCompileResult(bool success, uint generation, IReadOnlyList<CfdDiagnostic> diagnostics)
    {
        Success = success;
        Generation = generation;
        Diagnostics = diagnostics;
    }

    public bool Success { get; }
    public uint Generation { get; }
    public IReadOnlyList<CfdDiagnostic> Diagnostics { get; }

    internal static CoflowCompileResult Published(uint generation) =>
        new(true, generation, Array.Empty<CfdDiagnostic>());

    internal static CoflowCompileResult Failed(IReadOnlyList<CfdDiagnostic> diagnostics) =>
        new(false, 0, diagnostics);
}

public sealed class CoflowNotCompiledException : InvalidOperationException
{
    public CoflowNotCompiledException() : base("Coflow has no successfully compiled snapshot.") { }
}
