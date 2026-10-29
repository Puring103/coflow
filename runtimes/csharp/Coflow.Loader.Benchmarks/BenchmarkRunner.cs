using System.Diagnostics;
using System.Runtime.CompilerServices;
using System.Text.Json;
using Coflow.Loader;

sealed class BenchmarkRunner(BenchmarkOptions options)
{
    internal void Parser(Fixture fixture, string suite)
        => Measure(fixture, suite, "parser-only", "Parse all source text into syntax documents; excludes schema binding and object construction.",
            () => 0, _ => CfdParser.ParseAll(fixture.Sources), (_, documents) => fixture.VerifyDocuments(documents));

    internal void LoadPhases(Fixture fixture, string suite)
    {
        _ = fixture.Documents;
        Measure(fixture, suite, "construction", "Use parsed documents; includes input discovery, binding, base construction, declared variant validation and table registration.",
            () => 0, _ => Construct(fixture), (_, data) => fixture.VerifyData(data));
        Measure(fixture, suite, "full-load", "Parse source text, bind, construct base objects, validate declared variants and register tables.",
            () => 0, _ => CfdLoader.Load(fixture.Sources, fixture.Types, data => data), (_, loaded) => fixture.VerifyData(RequireData(loaded)));
    }

    internal void TableRegistration(Fixture fixture, string suite)
        => Measure(fixture, suite, "table-registration", "Register typed tables from a prepared, built load context; excludes parsing, context construction and Build.",
            () => BuildContext(fixture), context => Register(fixture.Types, context), (_, data) => fixture.VerifyData(data));

    internal void Views(Fixture fixture, string suite)
    {
        Measure(fixture, suite, "first-for", "Fresh loaded data per sample; select language[v0] once for every record. Preparation, arrays and assertions are excluded.",
            () => fixture.CreateViews(LoadChecked(fixture), false, 1), state => { state.Run(); return state; }, (_, state) => state.Verify(), fixture.RecordCount);
        Measure(fixture, suite, "cached-for", "Prepare and cache language[v0] for every record, then repeat For calls. Includes selection and lookup; excludes load, priming, arrays and assertions.",
            () => fixture.CreateViews(LoadChecked(fixture), true, options.ForIterations), state => { state.Run(); return state; }, (_, state) => state.Verify(),
            checked((long)fixture.RecordCount * options.ForIterations));
    }

    private static CfdData LoadChecked(Fixture fixture)
    {
        var data = RequireData(CfdLoader.Load(fixture.Sources, fixture.Types, value => value));
        fixture.VerifyData(data);
        return data;
    }

    private static CfdData RequireData(LoadResult<CfdData> loaded)
        => loaded.Success ? loaded.Data : throw new InvalidOperationException(string.Join("; ", loaded.Diagnostics));

    private static LoadContext BuildContext(Fixture fixture)
    {
        var context = new LoadContext(fixture.Documents, fixture.Types, null);
        context.Build();
        return context;
    }

    private static CfdData Construct(Fixture fixture) => Register(fixture.Types, BuildContext(fixture));

    private static CfdData Register(CfdType[] types, LoadContext context)
    {
        // 同一工具链接两版源码，只在内部表注册边界切换构造签名。
#if CFD_BOUND_LOADER
        return new CfdData(types, context);
#else
        return new CfdData(types, context.Records);
#endif
    }

    private void Measure<TState, TResult>(Fixture fixture, string suite, string phase, string phaseDescription,
        Func<TState> prepare, Func<TState, TResult> run, Action<TState, TResult> verify, long operationsPerSample = 1)
    {
        for (var warmup = 0; warmup < options.Warmups; warmup++)
        {
            var state = prepare();
            verify(state, run(state));
        }
        var samples = new Sample[options.Samples];
        for (var repeat = 0; repeat < samples.Length; repeat++) samples[repeat] = MeasureOne(fixture, prepare, run, verify);
        Console.WriteLine(JsonSerializer.Serialize(new
        {
            suite, fixture = fixture.Name, phase, phaseDescription,
            recordCount = fixture.RecordCount, fieldCount = fixture.FieldCount, variantCount = fixture.VariantCount,
            typeCount = fixture.Types.Length, overrideDensity = fixture.OverrideDensity,
            overrideRecordCount = fixture.OverrideRecordCount, overrideFieldCount = fixture.OverrideFieldCount,
            sourceCharacters = fixture.Sources.Sum(source => source.Text.Length), sourceSha256 = fixture.SourceSha256,
            readerAccess = "string", boundLoader = BoundLoader,
            countDescriptors = fixture.CountedFields != null, schemaDescriptorReads = fixture.SchemaDescriptorReads,
            warmups = options.Warmups, sampleCount = options.Samples, meetsSevenSampleMinimum = options.Samples >= 7,
            gcBeforeSample = true, allocationMetric = "current-thread cumulative allocated bytes during timed operation; excludes preparation and validation",
            operationsPerSample, samplesMs = samples.Select(sample => sample.Milliseconds).ToArray(),
            allocatedBytes = samples.Select(sample => sample.AllocatedBytes).ToArray(),
            sourceDescriptorReads = fixture.CountedFields == null ? null : samples.Select(sample => sample.DescriptorReads).ToArray(),
        }));
    }

    [MethodImpl(MethodImplOptions.NoInlining)]
    private static Sample MeasureOne<TState, TResult>(Fixture fixture, Func<TState> prepare, Func<TState, TResult> run, Action<TState, TResult> verify)
    {
        var state = prepare();
        // 准备对象保持存活；GC 只统一采样起点，不用于推断峰值或常驻内存。
        GC.Collect();
        GC.WaitForPendingFinalizers();
        if (fixture.CountedFields != null) fixture.CountedFields.Reads = 0;
        var before = GC.GetAllocatedBytesForCurrentThread();
        var started = Stopwatch.GetTimestamp();
        var result = run(state);
        var ended = Stopwatch.GetTimestamp();
        var allocated = GC.GetAllocatedBytesForCurrentThread() - before;
        var reads = fixture.CountedFields?.Reads ?? 0;
        // 断言、结果列表和 JSON 均在计时与分配计数之外。
        verify(state, result);
        GC.KeepAlive(state);
        GC.KeepAlive(result);
        return new Sample((ended - started) * 1000.0 / Stopwatch.Frequency, allocated, reads);
    }

#if CFD_BOUND_LOADER
    private const bool BoundLoader = true;
#else
    private const bool BoundLoader = false;
#endif
    private readonly record struct Sample(double Milliseconds, long AllocatedBytes, long DescriptorReads);
}
