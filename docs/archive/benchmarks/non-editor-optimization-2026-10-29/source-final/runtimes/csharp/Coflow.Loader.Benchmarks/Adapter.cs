using Coflow.Loader;

namespace Coflow.Benchmarks;

internal sealed record Outcome(Loaded? Data, IReadOnlyList<CfdDiagnostic> Diagnostics);

internal abstract class Adapter
{
    internal abstract Outcome Load(IEnumerable<CfdSource> sources);
}

internal abstract class Loaded
{
    internal abstract object Record(string id);
    internal abstract long Value(object row);
    internal abstract long SumValues();
    internal abstract void Verify(Config config);
    internal abstract void VerifySemantics(Config config);
    internal abstract void VerifyView(object row, int index, bool language, bool balance, Config config);
    internal abstract object For(object row, string dimension, string variant);
    internal abstract IReadOnlyList<string> Variants(object row, string dimension);
    internal abstract bool HasVariant(object row, string field, string variant);
}

internal sealed class Adapter<TData, TRow>(Func<IEnumerable<CfdSource>, LoadResult<TData>> load,
    Func<TData, Table<TRow>> table, Func<TRow, long> value,
    Action<TRow, int, Config, Table<TRow>> verify,
    Action<TRow, int, int?, int?, Config>? verifyView = null,
    Action<TData, Config>? verifyTables = null,
    Func<TRow, TRow?>? reference = null) : Adapter
    where TData : class where TRow : class, ICfdRecord
{
    internal override Outcome Load(IEnumerable<CfdSource> sources)
    {
        // 委托转发真实生成的 CoflowData.Load；固定的测量包装分配计入加载成本。
        var result = load(sources);
        return new Outcome(result.Success ? new TypedLoaded(result.Data, table(result.Data), value, verify, verifyView, verifyTables, reference) : null, result.Diagnostics);
    }

    private sealed class TypedLoaded(TData data, Table<TRow> rows, Func<TRow, long> value,
        Action<TRow, int, Config, Table<TRow>> verify,
        Action<TRow, int, int?, int?, Config>? verifyView,
        Action<TData, Config>? verifyTables, Func<TRow, TRow?>? reference) : Loaded
    {
        internal override object Record(string id) => rows[id];
        internal override long Value(object row) => value((TRow)row);
        internal override long SumValues() { long sum = 0; foreach (var row in rows.Values) sum += value(row); return sum; }
        internal override void Verify(Config config)
        {
            Program.Ensure(rows.Count == config.Records, "Record count mismatch.");
            for (var i = 0; i < config.Records; i++) verify(rows["r" + i], i, config, rows);
            GC.KeepAlive(data);
        }
        internal override void VerifyView(object row, int index, bool language, bool balance, Config config)
            => (verifyView ?? throw new InvalidOperationException("No dimension view verifier"))((TRow)row, index, language ? 0 : null, balance ? 0 : null, config);
        internal override void VerifySemantics(Config config)
        {
            Verify(config);
            verifyTables?.Invoke(data, config);
            if (config.Case != "dimensions") return;
            // 此方法仅在独立验证实例运行；额外业务视图不进入正式保留堆与计时样本。
            var indices = new[] { 0, 1, 99, 100, config.Records - 1 }.Where(i => i < config.Records).Distinct().ToArray();
            var variantNames = Enumerable.Range(0, config.Variants).Select(i => "v" + i).Order(StringComparer.Ordinal).ToArray();
            foreach (var index in indices)
            {
                var row = rows["r" + index];
                Program.Ensure(row.Variants("language").SequenceEqual(variantNames), "Language variants mismatch.");
                Program.Ensure(row.Variants("balance").SequenceEqual(variantNames), "Balance variants mismatch.");
                for (var variant = 0; variant < config.Variants; variant++)
                {
                    var name = "v" + variant;
                    Program.Ensure(row.HasVariant("name", name) == Fixture.Overridden(config, index), "Language override presence mismatch.");
                    Program.Ensure(row.HasVariant("value", name) == Fixture.Overridden(config, index), "Balance override presence mismatch.");
                    CheckSelection(row.For("language", name), index, variant, null, config);
                    CheckSelection(row.For("balance", name), index, null, variant, config);
                    var combined = row.For("language", name).For("balance", name);
                    CheckSelection(combined, index, variant, variant, config);
                    Program.Ensure(ReferenceEquals(combined, row.For("balance", name).For("language", name)), "Selection order changed view identity.");
                }
                verifyView!(row.For("language", "unknown"), index, null, null, config);
            }
            Verify(config);
        }
        private void CheckSelection(TRow view, int index, int? language, int? balance, Config config)
        {
            verifyView!(view, index, language, balance, config);
            if (reference == null) return;
            var next = Fixture.Next(config, index);
            if (next < 0) { Program.Ensure(reference(view) == null, "View added a reference."); return; }
            var expected = rows["r" + next];
            if (language.HasValue) expected = expected.For("language", "v" + language.Value);
            if (balance.HasValue) expected = expected.For("balance", "v" + balance.Value);
            Program.Ensure(ReferenceEquals(reference(view), expected), "View reference identity mismatch.");
        }
        internal override object For(object row, string dimension, string variant) => ((TRow)row).For(dimension, variant);
        internal override IReadOnlyList<string> Variants(object row, string dimension) => ((TRow)row).Variants(dimension);
        internal override bool HasVariant(object row, string field, string variant) => ((TRow)row).HasVariant(field, variant);
    }
}
