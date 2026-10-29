using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Text;

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
    internal static DimensionSelection Empty { get; } = new DimensionSelection(Array.Empty<KeyValuePair<string, string>>());
    private readonly KeyValuePair<string, string>[] _values;
    private DimensionSelection(KeyValuePair<string, string>[] values)
    {
        _values = values;
        var key = new StringBuilder();
        foreach (var pair in values) key.Append(pair.Key.Length).Append(':').Append(pair.Key).Append(pair.Value.Length).Append(':').Append(pair.Value);
        Key = key.ToString();
    }
    internal string Key { get; }
    private int Find(string dimension)
    {
        var low = 0;
        var high = _values.Length - 1;
        while (low <= high)
        {
            var middle = low + ((high - low) / 2);
            var comparison = StringComparer.Ordinal.Compare(_values[middle].Key, dimension);
            if (comparison == 0) return middle;
            if (comparison < 0) low = middle + 1;
            else high = middle - 1;
        }
        return ~low;
    }
    internal bool TryGet(string dimension, out string variant)
    {
        var index = Find(dimension);
        variant = index < 0 ? null! : _values[index].Value;
        return index >= 0;
    }
    internal DimensionSelection With(string dimension, string variant)
    {
        if (dimension == null || variant == null) throw new ArgumentNullException();
        var index = Find(dimension);
        if (index >= 0 && _values[index].Value == variant) return this;
        // 维度组合通常很小，紧凑有序数组避免每次选择分配树节点，并保留顺序无关的键。
        var values = new KeyValuePair<string, string>[_values.Length + (index < 0 ? 1 : 0)];
        if (index >= 0) Array.Copy(_values, values, _values.Length);
        else
        {
            index = ~index;
            Array.Copy(_values, 0, values, 0, index);
            Array.Copy(_values, index, values, index + 1, _values.Length - index);
        }
        values[index] = new KeyValuePair<string, string>(dimension, variant);
        return new DimensionSelection(values);
    }
}
}
