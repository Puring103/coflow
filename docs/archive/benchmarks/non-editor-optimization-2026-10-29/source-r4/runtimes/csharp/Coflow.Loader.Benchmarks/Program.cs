using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using Coflow.Loader;

namespace Coflow.Benchmarks;

internal sealed record Config(string Case, int Records, int Files, int Samples, int QuerySamples,
    int QueryBatch, int Variants, string Density, string Topology, bool Templates,
    string[] Operations, string? Output)
{
    internal int Fields => Case.StartsWith("scalar", StringComparison.Ordinal) ? int.Parse(Case[6..]) : 20;
    internal bool Complex => Case is "complex" or "dimensions";
    internal static Config Parse(string[] args)
    {
        var options = new Dictionary<string, string>(StringComparer.Ordinal);
        for (var i = 0; i < args.Length; i += 2)
        {
            if (!args[i].StartsWith("--", StringComparison.Ordinal) || i + 1 == args.Length)
                throw new ArgumentException("Arguments must be --name value pairs.");
            options.Add(args[i][2..], args[i + 1]);
        }
        string Get(string key, string value) => options.Remove(key, out var found) ? found : value;
        int Number(string key, int value) => int.Parse(Get(key, value.ToString()));
        var result = new Config(Get("case", "scalar20"), Number("records", 1000), Number("files", 10),
            Number("samples", 5), Number("query-samples", 100), Number("query-batch", 10000),
            Number("variants", 1), Get("density", "sparse"), Get("topology", "none"),
            bool.Parse(Get("templates", "false")), Get("operations", "load,io,reload,queries,error").Split(','),
            Get("output", "") is { Length: > 0 } path ? path : null);
        if (options.Count != 0) throw new ArgumentException("Unknown options: " + string.Join(",", options.Keys));
        if (result.Records < 2 || result.Files < 1 || result.Files > result.Records || result.Samples < 1
            || result.QuerySamples < 1 || result.QueryBatch < 1 || result.Variants < 1)
            throw new ArgumentException("Invalid positive size/sample parameters.");
        if (result.Case is not ("scalar8" or "scalar20" or "scalar100" or "tables32" or "complex" or "dimensions"))
            throw new ArgumentException("Unknown --case.");
        if (result.Density is not ("sparse" or "dense") || result.Topology is not ("none" or "pairs" or "hub" or "ring"))
            throw new ArgumentException("Unknown density or topology.");
        if (result.Operations.Except(new[] { "startup", "load", "io", "reload", "queries", "dimensions", "error" }).Any())
            throw new ArgumentException("Unknown operation group.");
        return result;
    }
}

internal sealed record Sample(double ElapsedMs, long AllocatedBytes, int[] Collections, long RetainedDeltaBytes,
    long ManagedLiveBytes, long WorkingSetBytes, long PeakWorkingSetBytes);

internal static class Program
{
    private static StreamWriter? _output;
    private static Config _config = null!;
    private static long _sink;

    private static int Main(string[] args)
    {
        string? temporary = null;
        try
        {
            _config = Config.Parse(args);
            if (_config.Output != null)
            {
                Directory.CreateDirectory(Path.GetDirectoryName(Path.GetFullPath(_config.Output))!);
                _output = new StreamWriter(_config.Output, false, new UTF8Encoding(false)) { AutoFlush = true };
            }
            Emit(new { kind = "metadata", config = _config, runtime = RuntimeInformation.FrameworkDescription,
                os = RuntimeInformation.OSDescription, architecture = RuntimeInformation.ProcessArchitecture.ToString(),
                processors = Environment.ProcessorCount, server_gc = System.Runtime.GCSettings.IsServerGC,
                stopwatch_frequency = Stopwatch.Frequency, utc = DateTime.UtcNow,
                protocol = "independent semantic preflight after optional startup; one unreported warmup except startup_load_text; complete raw samples; validation outside timing; forced full GC before heavy samples; no OS cache flush; each process runs one fixture",
                memory = "allocation delta covers operation only; retained heap is post-validation full-GC delta with result alive; process peak is cumulative and includes fixtures, validation, and prior operations" });
            var adapter = Adapters.Create(_config);
            var sources = Fixture.Create(_config);
            Emit(new { kind = "fixture", files = sources.Length, records = _config.Records, fields = _config.Fields,
                utf8_bytes = sources.Sum(source => (long)Encoding.UTF8.GetByteCount(source.Text)),
                utf16_chars = sources.Sum(source => (long)source.Text.Length), ring_group_size = 64,
                declared_dimension_variants = _config.Case == "dimensions" ? _config.Variants * 2 : 0 });
            temporary = Path.Combine(Path.GetTempPath(), "coflow-csharp-perf-" + Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(temporary);
            foreach (var source in sources) File.WriteAllText(Path.Combine(temporary, source.Path), source.Text, new UTF8Encoding(false));
            var groups = _config.Operations.ToHashSet(StringComparer.Ordinal);
            Loaded? loaded = null;
            if (groups.Contains("startup"))
            {
                // 记录本进程首次调用，含生成描述初始化与首次JIT；不是机器/文件系统冷启动。
                loaded = Measure("startup_load_text", 1, () => adapter.Load(sources),
                    result => RequireData(result, _config), true, warmup: false).Data;
                loaded = null;
                Collect();
            }
            ValidateFixture(adapter, sources);
            Collect();
            if (groups.Contains("load")) loaded = Measure("load_text", _config.Samples, () => adapter.Load(sources),
                result => RequireData(result, _config), true).Data;
            if (groups.Contains("io"))
            {
                loaded = null;
                Collect();
                var filePaths = sources.Select(source => (source.Path, Full: Path.Combine(temporary, source.Path))).ToArray();
                loaded = Measure("read_files_and_load", _config.Samples,
                    () => adapter.Load(filePaths.Select(file => new CfdSource(file.Path, File.ReadAllText(file.Full)))),
                    result => RequireData(result, _config), true).Data;
            }
            if (groups.Contains("reload"))
            {
                loaded = null;
                Collect();
                // 业务持有旧结果时重载，新旧对象图同时存活；两代引用身份必须隔离。
                var old = RequireData(adapter.Load(sources), _config);
                var first = old.Record("r0");
                loaded = Measure("reload_with_old_result_alive", _config.Samples, () => adapter.Load(sources), result =>
                {
                    var fresh = RequireData(result, _config);
                    Ensure(!ReferenceEquals(first, fresh.Record("r0")), "Reload returned old record identity.");
                    old.Verify(_config);
                }, true).Data;
                GC.KeepAlive(old);
            }
            if (groups.Contains("queries"))
            {
                loaded ??= RequireData(adapter.Load(sources), _config);
                Queries(loaded);
            }
            if (groups.Contains("dimensions"))
            {
                Ensure(_config.Case == "dimensions", "dimensions operations require --case dimensions");
                Dimensions(adapter, sources);
            }
            if (groups.Contains("error"))
            {
                // 错误置于最后文件，保留前面文件解析成本；错误和修复使用同一份健康夹具。
                var invalid = sources.ToArray();
                invalid[^1] = new CfdSource(invalid[^1].Path, invalid[^1].Text + "\nbroken: MissingType {}\n");
                Measure("reject_unknown_type_at_end", _config.Samples, () => adapter.Load(invalid), result =>
                {
                    Ensure(result.Data == null && result.Diagnostics.Any(d => d.Code == "CFD-TYPE"), "Expected type diagnostic.");
                }, true);
                var syntaxInvalid = sources.ToArray();
                syntaxInvalid[^1] = new CfdSource(syntaxInvalid[^1].Path, syntaxInvalid[^1].Text + "\nbroken: Row {");
                Measure("reject_syntax_at_end", _config.Samples, () => adapter.Load(syntaxInvalid), result =>
                {
                    Ensure(result.Data == null && result.Diagnostics.Any(d => d.Code.StartsWith("CFD-SYNTAX", StringComparison.Ordinal)), "Expected syntax diagnostic.");
                }, true);
                if (_config.Complex)
                {
                    var referenceInvalid = sources.ToArray();
                    var text = referenceInvalid[0].Text;
                    var start = text.IndexOf("next: ", StringComparison.Ordinal);
                    var end = text.IndexOf(',', start);
                    referenceInvalid[0] = new CfdSource(referenceInvalid[0].Path,
                        text[..start] + "next: Some(&Row::missing)" + text[end..]);
                    Measure("reject_missing_reference", _config.Samples, () => adapter.Load(referenceInvalid), result =>
                    {
                        Ensure(result.Data == null && result.Diagnostics.Any(d => d.Code == "CFD-REFERENCE"), "Expected reference diagnostic.");
                    }, true);
                }
                Measure("load_after_invalid_input", _config.Samples, () => adapter.Load(sources), result => RequireData(result, _config), true);
            }
            Emit(new { kind = "complete", success = true, sink = _sink });
            GC.KeepAlive(loaded);
            return 0;
        }
        catch (Exception error)
        {
            Emit(new { kind = "failure", error = error.ToString() });
            return 1;
        }
        finally
        {
            _output?.Dispose();
            if (temporary != null) Directory.Delete(temporary, true);
        }
    }

    private static Loaded RequireData(Outcome outcome, Config config)
    {
        Ensure(outcome.Data != null, string.Join("; ", outcome.Diagnostics));
        outcome.Data!.Verify(config);
        return outcome.Data;
    }

    [System.Runtime.CompilerServices.MethodImpl(System.Runtime.CompilerServices.MethodImplOptions.NoInlining)]
    private static void ValidateFixture(Adapter adapter, CfdSource[] sources)
    {
        var data = RequireData(adapter.Load(sources), _config);
        data.VerifySemantics(_config);
        Emit(new { kind = "validation", success = true, scope = "all base fields and references; all declared variants on boundary/dense/sparse representative records; dimension references and collections; selection order; unknown-variant fallback; derived table identity" });
    }

    private static void Queries(Loaded data)
    {
        var ids = Enumerable.Range(0, _config.Records).Select(i => "r" + i).ToArray();
        var expected = Enumerable.Range(0, _config.QueryBatch).Sum(i => (long)(i % ids.Length));
        Measure("table_lookup_batch", _config.QuerySamples, () =>
        {
            long sum = 0;
            for (var i = 0; i < _config.QueryBatch; i++) sum += data.Value(data.Record(ids[i % ids.Length]));
            return sum;
        }, sum => { Ensure(sum == expected, "Lookup checksum mismatch."); _sink = sum; }, false, _config.QueryBatch);
        var row = data.Record("r1");
        Measure("direct_property_batch", _config.QuerySamples, () =>
        {
            long sum = 0;
            for (var i = 0; i < _config.QueryBatch; i++) sum += data.Value(row);
            return sum;
        }, sum => { Ensure(sum == _config.QueryBatch, "Property checksum mismatch."); _sink = sum; }, false, _config.QueryBatch);
        Measure("table_enumerate", _config.QuerySamples, data.SumValues,
            sum => { Ensure(sum == (long)_config.Records * (_config.Records - 1) / 2, "Enumeration checksum mismatch."); _sink = sum; }, false, _config.Records);
    }

    private static void Dimensions(Adapter adapter, CfdSource[] sources)
    {
        Loaded? fresh = null;
        object? projected = null;
        // 每次首次访问使用独立加载结果，准备过程不计时；加载已校验并释放临时视图。
        Measure("dimension_first_record", _config.Samples, () => fresh!.For(fresh.Record("r0"), "language", "v0"), view =>
        {
            fresh!.VerifyView(view, 0, true, false, _config);
        }, true, 1, () => { fresh = RequireData(adapter.Load(sources), _config); });
        Measure("dimension_first_all", _config.Samples, () =>
        {
            long sum = 0;
            for (var i = 0; i < _config.Records; i++) sum += fresh!.Value(fresh.For(fresh.Record("r" + i), "language", "v0"));
            return sum;
        }, sum =>
        {
            Ensure(sum == (long)_config.Records * (_config.Records - 1) / 2, "Dimension checksum mismatch.");
            for (var i = 0; i < _config.Records; i++)
                fresh!.VerifyView(fresh.For(fresh.Record("r" + i), "language", "v0"), i, true, false, _config);
        },
            true, _config.Records, () => { fresh = RequireData(adapter.Load(sources), _config); });
        fresh = RequireData(adapter.Load(sources), _config);
        var baseRow = fresh.Record("r0");
        projected = fresh.For(baseRow, "language", "v0");
        fresh.VerifyView(projected, 0, true, false, _config);
        Measure("dimension_cached_for_batch", _config.QuerySamples, () =>
        {
            object result = projected;
            for (var i = 0; i < _config.QueryBatch; i++) result = fresh.For(baseRow, "language", "v0");
            return result;
        }, value => Ensure(ReferenceEquals(value, projected), "Cached view identity changed."), false, _config.QueryBatch);
        Measure("dimension_combined_first_record", _config.Samples,
            () => fresh!.For(fresh.For(fresh.Record("r0"), "language", "v0"), "balance", "v0"), view =>
            {
                fresh!.VerifyView(view, 0, true, true, _config);
            }, true, 1, () => { fresh = RequireData(adapter.Load(sources), _config); });
        // 未知变体回退仍有视图选择与缓存成本，和已有变体的首次访问分别计量。
        Measure("dimension_fallback_first_record", _config.Samples,
            () => fresh!.For(fresh.Record("r0"), "language", "unknown"), view =>
            {
                fresh!.VerifyView(view, 0, false, false, _config);
            }, true, 1, () => { fresh = RequireData(adapter.Load(sources), _config); });
        fresh = RequireData(adapter.Load(sources), _config);
        var fallbackRow = fresh.Record("r0");
        var fallback = fresh.For(fallbackRow, "language", "unknown");
        Measure("dimension_fallback_cached_batch", _config.QuerySamples, () =>
        {
            object result = fallback;
            for (var i = 0; i < _config.QueryBatch; i++) result = fresh.For(fallbackRow, "language", "unknown");
            return result;
        }, value => Ensure(ReferenceEquals(value, fallback), "Fallback identity changed."), false, _config.QueryBatch);
        var record = fresh.Record("r0");
        Measure("dimension_variants_batch", _config.QuerySamples, () =>
        {
            long sum = 0;
            for (var i = 0; i < _config.QueryBatch; i++) sum += fresh.Variants(record, "language").Count;
            return sum;
        }, sum => Ensure(sum == (long)_config.QueryBatch * _config.Variants, "Variant count mismatch."), false, _config.QueryBatch);
        Measure("dimension_has_variant_batch", _config.QuerySamples, () =>
        {
            long sum = 0;
            for (var i = 0; i < _config.QueryBatch; i++) if (fresh.HasVariant(record, "name", "v0")) sum++;
            return sum;
        }, sum => Ensure(sum == _config.QueryBatch, "Explicit override missing."), false, _config.QueryBatch);
        GC.KeepAlive(projected);
        GC.KeepAlive(fallback);
    }

    private static T Measure<T>(string name, int samples, Func<T> operation, Action<T> validate,
        bool fullGc, int operationsPerSample = 1, Action? prepare = null, bool warmup = true)
    {
        if (warmup)
        {
            prepare?.Invoke();
            var initial = operation();
            validate(initial);
            initial = default!;
        }
        var raw = new List<Sample>(samples);
        T last = default!;
        for (var sample = 0; sample < samples; sample++)
        {
            last = default!;
            prepare?.Invoke();
            if (fullGc) Collect();
            var beforeHeap = GC.GetTotalMemory(false);
            var beforeGc = Enumerable.Range(0, 3).Select(GC.CollectionCount).ToArray();
            var beforeAlloc = GC.GetTotalAllocatedBytes(true);
            var start = Stopwatch.GetTimestamp();
            last = operation();
            var elapsed = Stopwatch.GetElapsedTime(start).TotalMilliseconds;
            var allocated = GC.GetTotalAllocatedBytes(true) - beforeAlloc;
            var collections = Enumerable.Range(0, 3).Select(i => GC.CollectionCount(i) - beforeGc[i]).ToArray();
            validate(last);
            // 仅重操作采完整回收后的保留堆，查询样本不插入GC打断缓存和JIT状态。
            var live = GC.GetTotalMemory(fullGc);
            using var process = Process.GetCurrentProcess();
            process.Refresh();
            raw.Add(new Sample(elapsed, allocated, collections, live - beforeHeap, live,
                process.WorkingSet64, process.PeakWorkingSet64));
            GC.KeepAlive(last);
        }
        var ordered = raw.Select(sample => sample.ElapsedMs).Order().ToArray();
        var median = ordered.Length % 2 == 0 ? (ordered[ordered.Length / 2 - 1] + ordered[ordered.Length / 2]) / 2 : ordered[ordered.Length / 2];
        Emit(new { kind = "measurement", metric = name, samples, samples_ms = raw.Select(sample => sample.ElapsedMs).ToArray(), operations_per_sample = operationsPerSample,
            gc_before_sample = fullGc, warmups = warmup ? 1 : 0, samples_detail = raw, p50_ms = median,
            p95_ms = samples >= 20 ? ordered[(int)Math.Ceiling(samples * .95) - 1] : (double?)null,
            p99_ms = samples >= 100 ? ordered[(int)Math.Ceiling(samples * .99) - 1] : (double?)null,
            min_ms = ordered[0], max_ms = ordered[^1] });
        return last;
    }

    private static void Collect() { GC.Collect(); GC.WaitForPendingFinalizers(); GC.Collect(); }
    internal static void Ensure(bool success, string message) { if (!success) throw new InvalidOperationException(message); }
    private static void Emit(object value)
    {
        var json = JsonSerializer.Serialize(value);
        Console.WriteLine(json);
        _output?.WriteLine(json);
    }
}
