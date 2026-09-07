using BenchmarkDotNet.Attributes;
using BenchmarkDotNet.Columns;
using BenchmarkDotNet.Running;
using Coflow.Runtime;

BenchmarkSwitcher.FromAssembly(typeof(Program).Assembly).Run(args);

internal static class BenchmarkData
{
    internal static string Read(string fileName) =>
        File.ReadAllText(Path.Combine(AppContext.BaseDirectory, "Data", fileName));

    internal static global::Coflow.Runtime.Coflow CreateCompiled(string characters, string scenario)
    {
        var coflow = Schema.Create();
        coflow.LoadModule(new CoflowSource("characters.cfd", characters));
        coflow.LoadModule(new CoflowSource("scenario.cfd", scenario));
        BindHost(coflow);
        Compile(coflow);
        return coflow;
    }

    internal static void BindHost(global::Coflow.Runtime.Coflow coflow) => coflow.Bind(new HostServices(
            "benchmark",
            static _ => { },
            (value, operation) => operation.Invoke(coflow, value + 1),
            static value => value,
            static value => value,
            static value => value.HasValue
                ? Result<long, string>.Ok(value.Value)
                : Result<long, string>.Err("missing"),
            static value => value));

    internal static void Compile(global::Coflow.Runtime.Coflow coflow)
    {
        var result = coflow.Compile();
        if (result.Success) return;
        throw new CoflowLoadException(result.Diagnostics);
    }
}

[MemoryDiagnoser]
[MinColumn, MaxColumn, MedianColumn]
public class VmExecutionBenchmarks
{
    private global::Coflow.Runtime.Coflow _coflow = null!;
    private Scenario _scenario = null!;
    private CoflowFunction<long, long> _vmClosure;

    [GlobalSetup]
    public void Setup()
    {
        _coflow = BenchmarkData.CreateCompiled(
            BenchmarkData.Read("characters.cfd"),
            BenchmarkData.Read("scenario.cfd"));
        _scenario = _coflow.Table(Scenario.Table).Get("fullRoundTrip").Value;
        _vmClosure = _scenario.makeScaler(_coflow, 3);
    }

    [Benchmark]
    public long IntegerLoop() => _scenario.integerLoop(_coflow, 1_000);

    [Benchmark]
    public double FloatLoop() => _scenario.floatLoop(_coflow, 1_000);

    [Benchmark]
    public long DirectCfdCallChain() => _scenario.directCallChain(_coflow, 10);

    [Benchmark]
    public long TailRecursion() => _scenario.tailRecursion(_coflow, 1_000);

    [Benchmark]
    public long TailAccumulator() => _scenario.tailAccumulator(_coflow, 1_000, 0);

    [Benchmark]
    public long GeneratedFieldRead() => _scenario.fieldReadLoop(_coflow, 1_000);

    [Benchmark]
    public long MapFilterFold() => _scenario.collectionPipeline(_coflow, 6);

    [Benchmark]
    public long CfdToHost() => _scenario.hostCall(_coflow, 4);

    [Benchmark]
    public long VmHostVmClosure() => _scenario.callHost(_coflow, 4);

    [Benchmark]
    public long ReturnedVmClosure() => _vmClosure.Invoke(_coflow, 4);

    [Benchmark]
    public long PrimeTrialDivision() => _scenario.primeSum(_coflow, 250);

    [Benchmark]
    public long MatrixKernel() => _scenario.matrixKernel(_coflow, 12);

    [Benchmark]
    public long NonTailRecursiveFibonacci() => _scenario.fibonacci(_coflow, 18);

    [Benchmark]
    public bool BuiltinAnalytics() => _scenario.builtinSyntax(_coflow, "abc");
}

[MemoryDiagnoser]
[MinColumn, MaxColumn, MedianColumn]
public class ModuleLifecycleBenchmarks
{
    private string _characters = null!;
    private string _charactersAlternative = null!;
    private string _scenario = null!;
    private global::Coflow.Runtime.Coflow _coflow = null!;
    private CoflowModule _charactersModule = null!;
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
        _coflow = Schema.Create();
        _charactersModule = _coflow.LoadModule(new CoflowSource("characters.cfd", _characters));
        _coflow.LoadModule(new CoflowSource("scenario.cfd", _scenario));
        BenchmarkData.BindHost(_coflow);
        BenchmarkData.Compile(_coflow);
    }

    [Benchmark]
    public bool CreateAndCompile()
    {
        _ = BenchmarkData.CreateCompiled(_characters, _scenario);
        return true;
    }

    [Benchmark]
    public int ReplaceModuleSnapshot()
    {
        _useAlternative = !_useAlternative;
        _coflow.ReplaceModule(_charactersModule, new CoflowSource(
            "characters.cfd",
            _useAlternative ? _charactersAlternative : _characters));
        BenchmarkData.Compile(_coflow);
        return _coflow.Table(Character.Table).Count;
    }
}
