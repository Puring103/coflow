using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Linq;

namespace Coflow.Loader
{
/// <summary>Generated records retain the reader that supplies independent dimension views.</summary>
public interface ICfdRecord
{
    [EditorBrowsable(EditorBrowsableState.Never)]
    CfdReader Reader { get; set; }
}

public static class CfdRecordDimensions
{
    /// <summary>Returns an independent view with the selected dimension variant.</summary>
    public static T For<T>(this T record, string dimension, string variant) where T : class, ICfdRecord
        => (T)record.Reader.For(dimension, variant);

    /// <summary>Whether this field explicitly defines the variant, including None or the base value.</summary>
    public static bool HasVariant(this ICfdRecord record, string field, string variant)
        => record.Reader.HasVariant(field, variant);

    /// <summary>All variants declared for the dimension in the loaded project.</summary>
    public static IReadOnlyList<string> Variants(this ICfdRecord record, string dimension)
        => record.Reader.Variants(dimension);
}

// 上下文不可变；缓存标识同时包含维度和变体，避免同名变体跨维度串值。
internal sealed class DimensionSelection
{
    internal static DimensionSelection Empty { get; } = new DimensionSelection(new SortedDictionary<string, string>(StringComparer.Ordinal));
    private readonly SortedDictionary<string, string> _values;
    private DimensionSelection(SortedDictionary<string, string> values)
    {
        _values = values;
        Key = string.Concat(values.Select(pair => pair.Key.Length + ":" + pair.Key + pair.Value.Length + ":" + pair.Value));
    }
    internal string Key { get; }
    internal bool TryGet(string dimension, out string variant) => _values.TryGetValue(dimension, out variant!);
    internal DimensionSelection With(string dimension, string variant)
    {
        if (dimension == null || variant == null) throw new ArgumentNullException();
        var values = new SortedDictionary<string, string>(_values, StringComparer.Ordinal) { [dimension] = variant };
        return new DimensionSelection(values);
    }
}
}
