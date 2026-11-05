using System;
using System.Collections.Generic;
using System.Linq;
using System.Text;

namespace Coflow.Loader
{
/// <summary>An immutable, order-independent selection to build or query.</summary>
public sealed class DimensionSelection
{
    public static DimensionSelection Empty { get; } = new DimensionSelection();
    internal readonly (string Dimension, string Variant)[] Values;
    internal string Key { get; }
    public DimensionSelection(params (string Dimension, string Variant)[] values)
    {
        if (values == null) throw new ArgumentNullException(nameof(values));
        Values = values.OrderBy(value => value.Dimension, StringComparer.Ordinal).ToArray();
        var key = new StringBuilder();
        for (var i = 0; i < Values.Length; i++)
        {
            var pair = Values[i];
            if (pair.Dimension == null || pair.Variant == null) throw new ArgumentException("Dimension and variant must not be null.", nameof(values));
            if (i > 0 && Values[i - 1].Dimension == pair.Dimension) throw new ArgumentException($"Duplicate dimension `{pair.Dimension}`.", nameof(values));
            key.Append(pair.Dimension.Length).Append(':').Append(pair.Dimension).Append(pair.Variant.Length).Append(':').Append(pair.Variant);
        }
        Key = key.ToString();
    }
    internal bool TryGet(string dimension, out string variant)
    {
        foreach (var pair in Values) if (pair.Dimension == dimension) { variant = pair.Variant; return true; }
        variant = null!; return false;
    }
}

public sealed class LoadOptions
{
    /// <summary>Additional multi-dimension combinations to fully construct during loading.</summary>
    public IReadOnlyList<DimensionSelection> Combinations { get; set; } = Array.Empty<DimensionSelection>();
}

// 目录只保存已完成结果的位置和变体名称，不持有构建器、读取器或求值委托。
internal sealed class DimensionCatalog
{
    private readonly Dictionary<string, IReadOnlyList<string>> _variants = new Dictionary<string, IReadOnlyList<string>>(StringComparer.Ordinal);
    private readonly Dictionary<(string, string), int> _single = new Dictionary<(string, string), int>();
    private readonly Dictionary<string, int> _indices = new Dictionary<string, int>(StringComparer.Ordinal);
    internal IReadOnlyList<DimensionSelection> Selections { get; }
    internal DimensionCatalog(Dictionary<string, SortedSet<string>> variants, LoadOptions? options)
    {
        var selections = new List<DimensionSelection> { DimensionSelection.Empty };
        _indices.Add("", 0);
        foreach (var dimension in variants.OrderBy(pair => pair.Key, StringComparer.Ordinal))
        {
            _variants.Add(dimension.Key, Array.AsReadOnly(dimension.Value.ToArray()));
            foreach (var variant in dimension.Value)
            {
                var selection = new DimensionSelection((dimension.Key, variant));
                _single.Add((dimension.Key, variant), selections.Count);
                _indices.Add(selection.Key, selections.Count);
                selections.Add(selection);
            }
        }
        if (options != null)
            foreach (var selection in options.Combinations ?? throw new ArgumentException("Combinations must not be null.", nameof(options)))
            {
                if (selection == null) throw new ArgumentException("A combination must not be null.", nameof(options));
                Validate(selection);
                if (_indices.ContainsKey(selection.Key)) continue;
                _indices.Add(selection.Key, selections.Count);
                selections.Add(selection);
            }
        Selections = selections.AsReadOnly();
    }
    internal IReadOnlyList<string> Variants(string dimension) => _variants.TryGetValue(dimension, out var values)
        ? values : throw new ArgumentException($"Unknown dimension `{dimension}`.", nameof(dimension));
    private void Validate(DimensionSelection selection)
    {
        foreach (var pair in selection.Values) Index(pair.Dimension, pair.Variant);
    }
    internal int Index(string dimension, string variant)
    {
        var names = Variants(dimension);
        if (variant == null) throw new ArgumentNullException(nameof(variant));
        return _single.TryGetValue((dimension, variant), out var index) ? index
            : throw new ArgumentException($"Unknown variant `{variant}` for dimension `{dimension}`. Loaded variants: [{string.Join(", ", names)}].", nameof(variant));
    }
    internal int Index(DimensionSelection selection)
    {
        if (selection == null) throw new ArgumentNullException(nameof(selection));
        Validate(selection);
        return _indices.TryGetValue(selection.Key, out var index) ? index
            : throw new ArgumentException("This dimension combination was not built; include it in LoadOptions.Combinations.", nameof(selection));
    }
    internal bool TryIndex(string dimension, string variant, out int index) => _single.TryGetValue((dimension, variant), out index);
    internal bool TryIndex(DimensionSelection selection, out int index)
    {
        if (selection == null) throw new ArgumentNullException(nameof(selection));
        return _indices.TryGetValue(selection.Key, out index);
    }
}

/// <summary>Fully constructed values. Selection never loads, evaluates or creates business data.</summary>
public sealed class Dimension<T>
{
    private readonly DimensionCatalog _catalog;
    private readonly T[] _values;
    private readonly Dictionary<string, HashSet<string>>? _overrides;
    internal Dimension(DimensionCatalog catalog, T[] values, Dictionary<string, HashSet<string>>? overrides = null)
    { _catalog = catalog; _values = values; _overrides = overrides; }
    public T Base => _values[0];
    public T For(string dimension, string variant) => _values[_catalog.Index(dimension, variant)];
    public T For(DimensionSelection selection) => _values[_catalog.Index(selection)];
    public T For(params (string Dimension, string Variant)[] selection) => For(new DimensionSelection(selection));
    public bool TryFor(DimensionSelection selection, out T value)
    {
        if (_catalog.TryIndex(selection, out var index)) { value = _values[index]; return true; }
        value = default!; return false;
    }
    public bool TryFor(string dimension, string variant, out T value)
    {
        if (dimension == null || variant == null) throw new ArgumentNullException();
        if (_catalog.TryIndex(dimension, variant, out var index)) { value = _values[index]; return true; }
        value = default!; return false;
    }
    public IReadOnlyList<string> Variants(string dimension) => _catalog.Variants(dimension);
    public bool HasVariant(string field, string variant)
        => _overrides != null && _overrides.TryGetValue(field, out var values) ? values.Contains(variant)
            : throw new ArgumentException($"Field `{field}` is not bound to a dimension.", nameof(field));
}
}
