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
        => Integration.Config.CoflowData.LoadAndCompile(new[] { characters, scenario });

    internal static void BindHost(CoflowModule module)
    {
        var host = module.Singleton<HostServices>();
        if (!host.HasValue) throw new InvalidOperationException("Benchmark module has no HostServices singleton.");
        host.Value.Configure(
            "benchmark",
            static _ => { },
            static (value, operation) => operation(value + 1),
            static value => value,
            static value => value.HasValue
                ? Result<long, string>.Ok(value.Value)
                : Result<long, string>.Err("missing"));
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
        _scenario = _module.Table(Scenario.Table).Get("fullRoundTrip").Value;
        _vmClosure = _scenario.MakeScaler(3);
    }

    [Benchmark]
    public long IntegerLoop() => _scenario.IntegerLoop(1_000);

    [Benchmark]
    public double FloatLoop() => _scenario.FloatLoop(1_000);

    [Benchmark]
    public long DirectCfdCallChain() => _scenario.DirectCallChain(10);

    [Benchmark]
    public long TailRecursion() => _scenario.TailRecursion(1_000);

    [Benchmark]
    public long TailAccumulator() => _scenario.TailAccumulator(1_000, 0);

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

    [Benchmark]
    public long PrimeTrialDivision() => _scenario.PrimeSum(250);

    [Benchmark]
    public long MatrixKernel() => _scenario.MatrixKernel(12);

    [Benchmark]
    public long NonTailRecursiveFibonacci() => _scenario.Fibonacci(18);

    [Benchmark]
    public bool BuiltinAnalytics() => _scenario.BuiltinSyntax("abc");
}

[MemoryDiagnoser]
[MinColumn, MaxColumn, MedianColumn]
public class ModuleLifecycleBenchmarks
{
    private string _characters = null!;
    private string _charactersAlternative = null!;
    private string _scenario = null!;
    private CoflowModule _module = null!;
    private CoflowModuleSet _modules = null!;
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
        _module = BenchmarkData.LoadCompiled(_characters, _scenario);
        _modules = Coflow.Modules(_module);
        BenchmarkData.BindHost(_module);
    }

    [Benchmark]
    public bool LoadAndCompile()
    {
        var module = BenchmarkData.LoadCompiled(_characters, _scenario);
        return module.FunctionsCompiled;
    }

    [Benchmark]
    public int ReplaceModuleSnapshot()
    {
        _useAlternative = !_useAlternative;
        var replacement = BenchmarkData.LoadCompiled(
            _useAlternative ? _charactersAlternative : _characters,
            _scenario);
        var next = _modules.Replace(_module, replacement);
        _module = replacement;
        _modules = next;
        return next.Modules.Count;
    }
}
