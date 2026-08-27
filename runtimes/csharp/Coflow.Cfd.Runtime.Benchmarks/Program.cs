using BenchmarkDotNet.Attributes;
using BenchmarkDotNet.Columns;
using BenchmarkDotNet.Running;
using CoflowRuntime;
using Integration.Config.integration.runtime;

BenchmarkSwitcher.FromAssembly(typeof(Program).Assembly).Run(args);

internal static class BenchmarkData
{
    internal static string Read(string fileName) =>
        File.ReadAllText(Path.Combine(AppContext.BaseDirectory, "Data", fileName));

    internal static CoflowModule LoadCompiled(string characters, string scenario)
    {
        using var charactersModule = Coflow.LoadData(characters);
        return Coflow.LoadAndCompile(scenario, charactersModule);
    }

    internal static void BindHost(CoflowModule module)
    {
        var result = module.Singleton<HostServices>().Bind(
            "benchmark",
            static _ => { },
            static (value, operation) => operation(value + 1),
            static value => value);
        if (result.IsErr)
            throw new InvalidOperationException($"Unable to bind benchmark host: {result.Error}");
    }
}

[MemoryDiagnoser]
[MinColumn, MaxColumn, MedianColumn]
public class VmExecutionBenchmarks
{
    private CoflowModule _module = null!;
    private Scenario _scenario = null!;
    private Func<long, long> _vmClosure = null!;

    [GlobalSetup]
    public void Setup()
    {
        _module = BenchmarkData.LoadCompiled(
            BenchmarkData.Read("characters.cfd"),
            BenchmarkData.Read("scenario.cfd"));
        BenchmarkData.BindHost(_module);
        _scenario = _module.Table<Scenario>().Get("fullRoundTrip").Value;
        _vmClosure = _scenario.MakeScaler(3);
    }

    [GlobalCleanup]
    public void Cleanup() => _module.Dispose();

    [Benchmark]
    public long IntegerLoop() => _scenario.IntegerLoop(1_000);

    [Benchmark]
    public long DirectCfdCallChain() => _scenario.DirectCallChain(10);

    [Benchmark]
    public long TailRecursion() => _scenario.TailRecursion(1_000);

    [Benchmark]
    public long GeneratedFieldRead() => _scenario.FieldReadLoop(1_000);

    [Benchmark]
    public long MapFilterFold() => _scenario.CollectionPipeline(6);

    [Benchmark]
    public long CfdToHost() => _scenario.HostCall(4);

    [Benchmark]
    public long VmHostVmClosure() => _scenario.CallHost(4);

    [Benchmark]
    public long ReturnedVmClosure() => _vmClosure(4);
}

[MemoryDiagnoser]
[MinColumn, MaxColumn, MedianColumn]
public class ModuleLifecycleBenchmarks
{
    private string _characters = null!;
    private string _charactersAlternative = null!;
    private string _scenario = null!;
    private CoflowModule _charactersModule = null!;
    private CoflowModule _module = null!;
    private bool _useAlternative;

    [GlobalSetup]
    public void Setup()
    {
        _characters = BenchmarkData.Read("characters.cfd");
        _charactersAlternative = _characters.Replace(
            "attack: 19",
            "attack: 20",
            StringComparison.Ordinal);
        _scenario = BenchmarkData.Read("scenario.cfd");
        _charactersModule = Coflow.LoadData(_characters);
        _module = Coflow.LoadAndCompile(_scenario, _charactersModule);
        BenchmarkData.BindHost(_module);
    }

    [GlobalCleanup]
    public void Cleanup()
    {
        _module.Dispose();
        _charactersModule.Dispose();
    }

    [Benchmark]
    public bool LoadAndCompile()
    {
        using var module = BenchmarkData.LoadCompiled(_characters, _scenario);
        return module.FunctionsCompiled;
    }

    [Benchmark]
    public long ReloadAndRecompile()
    {
        _useAlternative = !_useAlternative;
        var result = _module.Reload(
            _charactersModule,
            _useAlternative ? _charactersAlternative : _characters);
        if (result.IsErr)
            throw new InvalidOperationException(
                "Reload failed: " + string.Join(Environment.NewLine, result.Error.Diagnostics));
        return result.Value.Generation;
    }
}
