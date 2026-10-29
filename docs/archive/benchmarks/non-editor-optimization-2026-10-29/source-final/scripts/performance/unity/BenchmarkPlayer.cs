#nullable enable
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Text;
using Coflow.Loader;
using UnityEngine;
using UnityEngine.Profiling;

namespace Coflow.Benchmarks
{
    internal sealed class Config
    {
        internal string Case = "scalar20", Density = "sparse", Topology = "none";
        internal int Records = 1000, Files = 10, Samples = 5, QuerySamples = 100, QueryBatch = 10000, Variants = 1;
        internal bool Templates;
        internal string[] Operations = { "load", "io", "reload", "queries", "error" };
        internal string? Output;
        internal int Fields => Case.StartsWith("scalar", StringComparison.Ordinal) ? int.Parse(Case.Substring(6)) : 20;
        internal bool Complex => Case == "complex" || Case == "dimensions";

        internal static Config Parse(string[] args)
        {
            var options = new Dictionary<string, string>(StringComparer.Ordinal);
            // Unity 自身的单横线参数由宿主处理；基准只接受显式双横线成对参数。
            for (var i = 1; i < args.Length; i++)
            {
                if (!args[i].StartsWith("--", StringComparison.Ordinal)) continue;
                if (i + 1 == args.Length || args[i + 1].StartsWith("--", StringComparison.Ordinal))
                    throw new ArgumentException("Benchmark arguments require --name value pairs.");
                options.Add(args[i].Substring(2), args[++i]);
            }
            string Get(string key, string fallback)
            {
                if (!options.TryGetValue(key, out var found)) return fallback;
                options.Remove(key); return found;
            }
            int Number(string key, int fallback) => int.Parse(Get(key, fallback.ToString(CultureInfo.InvariantCulture)), CultureInfo.InvariantCulture);
            var result = new Config
            {
                Case = Get("case", "scalar20"), Records = Number("records", 1000), Files = Number("files", 10),
                Samples = Number("samples", 5), QuerySamples = Number("query-samples", 100), QueryBatch = Number("query-batch", 10000),
                Variants = Number("variants", 1), Density = Get("density", "sparse"), Topology = Get("topology", "none"),
                Templates = bool.Parse(Get("templates", "false")), Operations = Get("operations", "load,io,reload,queries,error").Split(','),
                Output = Get("output", "")
            };
            if (options.Count != 0) throw new ArgumentException("Unknown options: " + string.Join(",", options.Keys));
            if (result.Records < 2 || result.Files < 1 || result.Files > result.Records || result.Samples < 1 || result.QuerySamples < 1 || result.QueryBatch < 1 || result.Variants < 1)
                throw new ArgumentException("Invalid size/sample parameters.");
            if (!new[] { "scalar8", "scalar20", "scalar100", "tables32", "complex", "dimensions" }.Contains(result.Case)) throw new ArgumentException("Unknown case.");
            if (!new[] { "sparse", "dense" }.Contains(result.Density) || !new[] { "none", "pairs", "hub", "ring" }.Contains(result.Topology)) throw new ArgumentException("Unknown density/topology.");
            if (result.Operations.Except(new[] { "startup", "load", "io", "reload", "queries", "dimensions", "error", "validate", "contracts" }).Any()) throw new ArgumentException("Unknown operation group.");
            if (string.IsNullOrEmpty(result.Output)) throw new ArgumentException("--output is required for a durable NDJSON artifact.");
            return result;
        }
    }

    internal static class Program
    {
        private static StreamWriter? output;
        private static Config config = null!;
        private static long sink;
        private static bool allocationAvailable;

        [RuntimeInitializeOnLoadMethod(RuntimeInitializeLoadType.AfterSceneLoad)]
        private static void Start()
        {
            // 玩家启动后同步执行，避免编辑器与帧循环影响样本；引擎启动不计入 Load。
            Application.Quit(Run(Environment.GetCommandLineArgs()));
        }

        private static int Run(string[] args)
        {
            string? temporary = null;
            try
            {
                config = Config.Parse(args);
                var outputPath = Path.GetFullPath(config.Output!);
                Directory.CreateDirectory(Path.GetDirectoryName(outputPath)!);
                output = new StreamWriter(outputPath, false, new UTF8Encoding(false)) { AutoFlush = true };
                allocationAvailable = ProbeAllocationCounter();
                Emit("metadata", "unity_version", Application.unityVersion, "backend", Backend,
                    "runtime", Backend == "IL2CPP" ? "Unity IL2CPP AOT" : "Unity Mono",
                    "development", UnityEngine.Debug.isDebugBuild, "os", SystemInfo.operatingSystem,
                    "architecture", IntPtr.Size == 8 ? "x64" : "x86", "processors", Environment.ProcessorCount,
                    "stopwatch_frequency", Stopwatch.Frequency, "gc_max_generation", GC.MaxGeneration,
                    "allocation_counter_available", allocationAvailable, "incremental_gc", UnityEngine.Scripting.GarbageCollector.isIncremental,
                    "case", config.Case, "records", config.Records, "files", config.Files, "variants", config.Variants,
                    "density", config.Density, "topology", config.Topology, "templates", config.Templates,
                    "operations", config.Operations, "utc", DateTime.UtcNow.ToString("o"),
                    "protocol", "independent semantic preflight after optional startup; one warmup except startup; full GC before heavy samples; validation outside timing; OS cache retained; one fixture per process",
                    "memory", "current-thread operation allocation or null if unsupported; GC counts for supported generations; Unity managed heap used/reserved; no OS process working set; retained delta is after validation/full GC for heavy metrics");
                Ensure(!UnityEngine.Debug.isDebugBuild, "Formal measurements require a non-Development player.");
                var adapter = Adapters.Create(config);
                var sources = Fixture.Create(config);
                Emit("fixture", "files", sources.Length, "records", config.Records, "fields", config.Fields,
                    "utf8_bytes", sources.Sum(s => (long)Encoding.UTF8.GetByteCount(s.Text)),
                    "utf16_chars", sources.Sum(s => (long)s.Text.Length), "ring_group_size", 64,
                    "declared_dimension_variants", config.Case == "dimensions" ? config.Variants * 2 : 0);
                temporary = Path.Combine(Path.GetTempPath(), "coflow-unity-perf-" + Guid.NewGuid().ToString("N"));
                Directory.CreateDirectory(temporary);
                foreach (var source in sources) File.WriteAllText(Path.Combine(temporary, source.Path), source.Text, new UTF8Encoding(false));
                var groups = new HashSet<string>(config.Operations, StringComparer.Ordinal);
                Loaded? loaded = null;
                if (groups.Contains("startup"))
                {
                    loaded = Measure("startup_load_text", 1, () => adapter.Load(sources), x => RequireData(x), true, warmup: false).Data;
                    loaded = null; Collect();
                }
                ValidateFixture(adapter, sources);
                if (groups.Contains("contracts"))
                {
                    var thread = ConstructionContracts.Run();
                    Emit("contract_validation", "success", true, "cases_per_thread", ConstructionContracts.CasesPerThread,
                        "threads", "Unity main thread and new Thread without a requested stack size", "worker_managed_thread_id", thread,
                        "scope", "256/257-level references/templates; variant-only 300-level rejection; deep cycles; list/dictionary continuation identity; DFS order; mixed explicit references/templates/collections; pending ancestor cycles; depth limit before missing target");
                }
                Collect();
                if (groups.Contains("load")) loaded = Measure("load_text", config.Samples, () => adapter.Load(sources), x => RequireData(x), true).Data;
                if (groups.Contains("io"))
                {
                    loaded = null; Collect();
                    var paths = sources.Select(s => (s.Path, Full: Path.Combine(temporary, s.Path))).ToArray();
                    loaded = Measure("read_files_and_load", config.Samples,
                        () => adapter.Load(paths.Select(p => new CfdSource(p.Path, File.ReadAllText(p.Full)))), x => RequireData(x), true).Data;
                }
                if (groups.Contains("reload"))
                {
                    loaded = null; Collect();
                    var old = RequireData(adapter.Load(sources));
                    var first = old.Record("r0");
                    loaded = Measure("reload_with_old_result_alive", config.Samples, () => adapter.Load(sources), x =>
                    {
                        var fresh = RequireData(x);
                        Ensure(!ReferenceEquals(first, fresh.Record("r0")), "Reload returned old identity.");
                        old.Verify(config);
                    }, true).Data;
                    GC.KeepAlive(old);
                }
                if (groups.Contains("queries")) { loaded ??= RequireData(adapter.Load(sources)); Queries(loaded); }
                if (groups.Contains("dimensions"))
                {
                    Ensure(config.Case == "dimensions", "Dimension operations require dimensions fixture.");
                    Dimensions(adapter, sources);
                }
                if (groups.Contains("error")) Errors(adapter, sources);
                Emit("complete", "success", true, "sink", sink);
                GC.KeepAlive(loaded);
                return 0;
            }
            catch (Exception error) { Emit("failure", "error", error.ToString()); return 1; }
            finally
            {
                output?.Dispose(); output = null;
                if (temporary != null) Directory.Delete(temporary, true);
            }
        }

        private static string Backend
        {
            get
            {
#if ENABLE_IL2CPP
                return "IL2CPP";
#else
                return "Mono";
#endif
            }
        }

        private static Loaded RequireData(Outcome result)
        {
            Ensure(result.Data != null, string.Join("; ", result.Diagnostics));
            result.Data!.Verify(config); return result.Data;
        }

        [System.Runtime.CompilerServices.MethodImpl(System.Runtime.CompilerServices.MethodImplOptions.NoInlining)]
        private static void ValidateFixture(Adapter adapter, CfdSource[] sources)
        {
            var data = RequireData(adapter.Load(sources)); data.VerifySemantics(config);
            Emit("validation", "success", true, "scope", "all base fields, collections, templates and references; all variants on representative records; derived table identity; dimension references; selection order; unknown-variant fallback");
        }

        private static void Queries(Loaded data)
        {
            var ids = Enumerable.Range(0, config.Records).Select(i => "r" + i).ToArray();
            var expected = Enumerable.Range(0, config.QueryBatch).Sum(i => (long)(i % ids.Length));
            Measure("table_lookup_batch", config.QuerySamples, () =>
            {
                long sum = 0; for (var i = 0; i < config.QueryBatch; i++) sum += data.Value(data.Record(ids[i % ids.Length])); return sum;
            }, sum => { Ensure(sum == expected, "Lookup checksum mismatch."); sink = sum; }, false, config.QueryBatch);
            var row = data.Record("r1");
            Measure("direct_property_batch", config.QuerySamples, () =>
            {
                long sum = 0; for (var i = 0; i < config.QueryBatch; i++) sum += data.Value(row); return sum;
            }, sum => { Ensure(sum == config.QueryBatch, "Property checksum mismatch."); sink = sum; }, false, config.QueryBatch);
            Measure("table_enumerate", config.QuerySamples, data.SumValues,
                sum => { Ensure(sum == (long)config.Records * (config.Records - 1) / 2, "Enumeration checksum mismatch."); sink = sum; }, false, config.Records);
        }

        private static void Dimensions(Adapter adapter, CfdSource[] sources)
        {
            Loaded? fresh = null;
            // 首次视图每个样本重新加载独立结果；准备加载与校验均不计时。
            Action prepare = () => { fresh = null; Collect(); fresh = RequireData(adapter.Load(sources)); };
            Measure("dimension_first_record", config.Samples, () => fresh!.For(fresh.Record("r0"), "language", "v0"),
                view => fresh!.VerifyView(view, 0, true, false, config), true, 1, prepare);
            Measure("dimension_first_all", config.Samples, () =>
            {
                long sum = 0;
                for (var i = 0; i < config.Records; i++) sum += fresh!.Value(fresh.For(fresh.Record("r" + i), "language", "v0"));
                return sum;
            }, sum =>
            {
                Ensure(sum == (long)config.Records * (config.Records - 1) / 2, "Dimension checksum mismatch.");
                for (var i = 0; i < config.Records; i++) fresh!.VerifyView(fresh.For(fresh.Record("r" + i), "language", "v0"), i, true, false, config);
            }, true, config.Records, prepare);
            prepare();
            var baseRow = fresh!.Record("r0");
            var projected = fresh.For(baseRow, "language", "v0");
            fresh.VerifyView(projected, 0, true, false, config);
            Measure("dimension_cached_for_batch", config.QuerySamples, () =>
            {
                object result = projected; for (var i = 0; i < config.QueryBatch; i++) result = fresh.For(baseRow, "language", "v0"); return result;
            }, value => Ensure(ReferenceEquals(value, projected), "Cached view identity changed."), false, config.QueryBatch);
            Measure("dimension_combined_first_record", config.Samples,
                () => fresh!.For(fresh.For(fresh.Record("r0"), "language", "v0"), "balance", "v0"),
                view => fresh!.VerifyView(view, 0, true, true, config), true, 1, prepare);
            Measure("dimension_fallback_first_record", config.Samples,
                () => fresh!.For(fresh.Record("r0"), "language", "unknown"),
                view => fresh!.VerifyView(view, 0, false, false, config), true, 1, prepare);
            prepare();
            var fallbackRow = fresh!.Record("r0");
            var fallback = fresh.For(fallbackRow, "language", "unknown");
            Measure("dimension_fallback_cached_batch", config.QuerySamples, () =>
            {
                object result = fallback; for (var i = 0; i < config.QueryBatch; i++) result = fresh.For(fallbackRow, "language", "unknown"); return result;
            }, value => Ensure(ReferenceEquals(value, fallback), "Fallback identity changed."), false, config.QueryBatch);
            var record = fresh.Record("r0");
            Measure("dimension_variants_batch", config.QuerySamples, () =>
            {
                long sum = 0; for (var i = 0; i < config.QueryBatch; i++) sum += fresh.Variants(record, "language").Count; return sum;
            }, sum => Ensure(sum == (long)config.QueryBatch * config.Variants, "Variant count mismatch."), false, config.QueryBatch);
            Measure("dimension_has_variant_batch", config.QuerySamples, () =>
            {
                long sum = 0; for (var i = 0; i < config.QueryBatch; i++) if (fresh.HasVariant(record, "name", "v0")) sum++; return sum;
            }, sum => Ensure(sum == config.QueryBatch, "Explicit override missing."), false, config.QueryBatch);
            GC.KeepAlive(projected); GC.KeepAlive(fallback);
        }

        private static void Errors(Adapter adapter, CfdSource[] sources)
        {
            var invalid = sources.ToArray();
            var last = invalid.Length - 1;
            invalid[last] = new CfdSource(invalid[last].Path, invalid[last].Text + "\nbroken: MissingType {}\n");
            Measure("reject_unknown_type_at_end", config.Samples, () => adapter.Load(invalid),
                x => Ensure(x.Data == null && x.Diagnostics.Any(d => d.Code == "CFD-TYPE"), "Expected type diagnostic."), true);
            var syntax = sources.ToArray();
            syntax[last] = new CfdSource(syntax[last].Path, syntax[last].Text + "\nbroken: Row {");
            Measure("reject_syntax_at_end", config.Samples, () => adapter.Load(syntax),
                x => Ensure(x.Data == null && x.Diagnostics.Any(d => d.Code.StartsWith("CFD-SYNTAX", StringComparison.Ordinal)), "Expected syntax diagnostic."), true);
            if (config.Complex)
            {
                var reference = sources.ToArray(); var text = reference[0].Text;
                var start = text.IndexOf("next: ", StringComparison.Ordinal); var end = text.IndexOf(',', start);
                reference[0] = new CfdSource(reference[0].Path, text.Substring(0, start) + "next: Some(&Row::missing)" + text.Substring(end));
                Measure("reject_missing_reference", config.Samples, () => adapter.Load(reference),
                    x => Ensure(x.Data == null && x.Diagnostics.Any(d => d.Code == "CFD-REFERENCE"), "Expected reference diagnostic."), true);
            }
            Measure("load_after_invalid_input", config.Samples, () => adapter.Load(sources), x => RequireData(x), true);
        }

        private static T Measure<T>(string name, int samples, Func<T> operation, Action<T> validate,
            bool fullGc, int operationsPerSample = 1, Action? prepare = null, bool warmup = true)
        {
            if (warmup) { prepare?.Invoke(); var initial = operation(); validate(initial); initial = default!; }
            var elapsedSamples = new double[samples]; var details = new object[samples];
            T last = default!;
            for (var sample = 0; sample < samples; sample++)
            {
                last = default!; prepare?.Invoke(); if (fullGc) Collect();
                var beforeHeap = GC.GetTotalMemory(false);
                var beforeGc = Enumerable.Range(0, GC.MaxGeneration + 1).Select(GC.CollectionCount).ToArray();
                var beforeAlloc = Allocated();
                var started = Stopwatch.GetTimestamp();
                last = operation();
                var elapsed = (Stopwatch.GetTimestamp() - started) * 1000.0 / Stopwatch.Frequency;
                var allocated = Allocated() - beforeAlloc;
                var collections = Enumerable.Range(0, beforeGc.Length).Select(i => GC.CollectionCount(i) - beforeGc[i]).ToArray();
                validate(last);
                if (fullGc) Collect();
                var live = GC.GetTotalMemory(false);
                elapsedSamples[sample] = elapsed;
                details[sample] = Map("elapsed_ms", elapsed, "allocated_current_thread_bytes", allocationAvailable ? (object)allocated : null,
                    "collections", collections, "retained_delta_bytes", live - beforeHeap, "managed_live_bytes", live,
                    "unity_managed_used_bytes", Profiler.GetMonoUsedSizeLong(), "unity_managed_reserved_bytes", Profiler.GetMonoHeapSizeLong());
                GC.KeepAlive(last);
            }
            var ordered = elapsedSamples.OrderBy(x => x).ToArray();
            var median = samples % 2 == 0 ? (ordered[samples / 2 - 1] + ordered[samples / 2]) / 2 : ordered[samples / 2];
            Emit("measurement", "metric", name, "samples", samples, "samples_ms", elapsedSamples,
                "operations_per_sample", operationsPerSample, "gc_before_sample", fullGc, "warmups", warmup ? 1 : 0,
                "samples_detail", details, "p50_ms", median,
                "p95_ms", samples >= 20 ? (object)ordered[(int)Math.Ceiling(samples * .95) - 1] : null,
                "p99_ms", samples >= 100 ? (object)ordered[(int)Math.Ceiling(samples * .99) - 1] : null,
                "min_ms", ordered[0], "max_ms", ordered[samples - 1]);
            return last;
        }

        private static bool ProbeAllocationCounter()
        {
            try
            {
                var before = GC.GetAllocatedBytesForCurrentThread(); var probe = new byte[4096];
                var after = GC.GetAllocatedBytesForCurrentThread(); GC.KeepAlive(probe); return after > before;
            }
            catch (NotSupportedException) { return false; }
        }
        private static long Allocated() => allocationAvailable ? GC.GetAllocatedBytesForCurrentThread() : 0;
        private static void Collect() { GC.Collect(); GC.WaitForPendingFinalizers(); GC.Collect(); }
        internal static void Ensure(bool condition, string message) { if (!condition) throw new InvalidOperationException(message); }

        // 显式 JSON 写入支持 AOT，避免反射序列化和仅 .NET 8 可用的依赖。
        private static Dictionary<string, object?> Map(params object?[] pairs)
        {
            var value = new Dictionary<string, object?>();
            for (var i = 0; i < pairs.Length; i += 2) value.Add((string)pairs[i]!, pairs[i + 1]);
            return value;
        }
        private static void Emit(string kind, params object?[] pairs)
        {
            var value = Map(pairs); value.Add("kind", kind);
            var builder = new StringBuilder(); Json(builder, value);
            var line = builder.ToString(); output?.WriteLine(line); UnityEngine.Debug.Log(line);
        }
        private static void Json(StringBuilder builder, object? value)
        {
            if (value == null) { builder.Append("null"); return; }
            if (value is string text)
            {
                builder.Append('"');
                foreach (var c in text)
                {
                    switch (c)
                    {
                        case '"': builder.Append("\\\""); break;
                        case '\\': builder.Append("\\\\"); break;
                        case '\n': builder.Append("\\n"); break;
                        case '\r': builder.Append("\\r"); break;
                        case '\t': builder.Append("\\t"); break;
                        default: if (c < 32) builder.Append("\\u").Append(((int)c).ToString("x4")); else builder.Append(c); break;
                    }
                }
                builder.Append('"'); return;
            }
            if (value is bool boolean) { builder.Append(boolean ? "true" : "false"); return; }
            if (value is Dictionary<string, object?> map)
            {
                builder.Append('{'); var first = true;
                foreach (var pair in map) { if (!first) builder.Append(','); first = false; Json(builder, pair.Key); builder.Append(':'); Json(builder, pair.Value); }
                builder.Append('}'); return;
            }
            if (value is System.Collections.IEnumerable items)
            {
                builder.Append('['); var first = true;
                foreach (var item in items) { if (!first) builder.Append(','); first = false; Json(builder, item); }
                builder.Append(']'); return;
            }
            if (value is IFormattable number) { builder.Append(number.ToString(null, CultureInfo.InvariantCulture)); return; }
            throw new ArgumentException("Unsupported JSON value: " + value.GetType());
        }
    }
}
