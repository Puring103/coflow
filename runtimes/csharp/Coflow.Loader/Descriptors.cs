using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.Globalization;
using System.Linq;

namespace Coflow.Loader
{
// 描述仅包含类型与构造协议，解析算法由加载器统一提供。
public abstract class CfdValueType
{
    internal abstract object? Read(LoadContext context, CfdValueNode node, CfdReader owner);
    internal virtual string? FormatScalar(object? value) => null;
    internal virtual bool AllowsMissing => false;
    internal virtual string? Dimension => null;
    internal virtual string? NamedType => null;
    internal virtual object? ReadDimension(LoadContext context, CfdValueNode node, CfdReader owner, string dimension, IReadOnlyDictionary<string, CfdValueNode> overrides) => Read(context, node, owner);
}

public sealed class CfdValueType<T> : CfdValueType
{
    private readonly Func<LoadContext, CfdValueNode, CfdReader, T> _read;
    private readonly bool _optional;
    private readonly Func<T, string?>? _formatScalar;
    private readonly string? _namedType;
    private readonly string? _dimension;
    private readonly Func<LoadContext, CfdValueNode, CfdReader, IReadOnlyDictionary<string, CfdValueNode>, T>? _readDimension;
    internal CfdValueType(Func<LoadContext, CfdValueNode, CfdReader, T> read, bool optional = false, string? namedType = null, string? dimension = null, Func<LoadContext, CfdValueNode, CfdReader, IReadOnlyDictionary<string, CfdValueNode>, T>? readDimension = null, Func<T, string?>? formatScalar = null)
    { _read = read; _optional = optional; _namedType = namedType; _dimension = dimension; _readDimension = readDimension; _formatScalar = formatScalar; }
    internal override string? FormatScalar(object? value) => _formatScalar == null ? null : _formatScalar((T)value!);
    internal override string? Dimension => _dimension;
    internal override object? ReadDimension(LoadContext context, CfdValueNode node, CfdReader owner, string dimension, IReadOnlyDictionary<string, CfdValueNode> overrides) => _readDimension == null ? _read(context, context.ResolveConstant(node), owner) : _readDimension(context, context.ResolveConstant(node), owner, overrides);
    internal override object? Read(LoadContext context, CfdValueNode node, CfdReader owner) => _read(context, context.ResolveConstant(node), owner);
    internal override string? NamedType => _namedType;
    internal override bool AllowsMissing => _optional;
}

public abstract class CfdField
{
    protected CfdField(string name, CfdValueType type, CfdValueNode? defaultValue)
    { Name = name; Type = type; DefaultValue = defaultValue; }
    public string Name { get; }
    internal CfdValueType Type { get; }
    internal CfdValueNode? DefaultValue { get; }
}

public sealed class CfdField<T> : CfdField
{
    public CfdField(string name, CfdValueType<T> type, CfdValueNode? defaultValue = null) : base(name, type, defaultValue) { }
}

public abstract class CfdType
{
    protected CfdType(string name, IReadOnlyList<CfdField> fields, bool record, bool singleton, string? parent, bool abstractType)
    { Name = name; Fields = fields; IsRecord = record; IsSingleton = singleton; Parent = parent; IsAbstract = abstractType; }
    public string Name { get; }
    internal IReadOnlyList<CfdField> Fields { get; }
    internal bool IsRecord { get; }
    internal bool IsSingleton { get; }
    internal bool IsAbstract { get; }
    internal string? Parent { get; }
    internal abstract object CreateRecord(string key);
    internal abstract void FillRecord(object value, CfdReader reader);
    internal abstract object Construct(CfdReader reader);
}

public sealed class CfdType<T> : CfdType
{
    private readonly Func<string, T>? _create;
    private readonly Action<T, CfdReader>? _fill;
    private readonly Func<CfdReader, T>? _construct;
    // 记录先创建身份，再填充字段，允许直接对象引用成环。
    public CfdType(string name, IReadOnlyList<CfdField> fields, Func<string, T>? create, Action<T, CfdReader>? fill, bool singleton = false, string? parent = null, bool abstractType = false)
        : base(name, fields, true, singleton, parent, abstractType) { _create = create; _fill = fill; }
    // struct 与内联 class 均通过完整字段值一次性构造。
    public CfdType(string name, IReadOnlyList<CfdField> fields, Func<CfdReader, T>? construct, string? parent = null, bool abstractType = false)
        : base(name, fields, false, false, parent, abstractType) { _construct = construct; }
    internal override object CreateRecord(string key) => _create!(key)!;
    internal override void FillRecord(object value, CfdReader reader) => _fill!((T)value, reader);
    internal override object Construct(CfdReader reader) => _construct!(reader)!;
}

public static class CfdDefaults
{
    private static readonly CfdSpan Span = new CfdSpan(0, 0, 0, 0);
    public static CfdValueNode Int(int value) => new CfdScalarValue(value.ToString(CultureInfo.InvariantCulture), Span);
    public static CfdValueNode Float(float value) => new CfdScalarValue(value.ToString("R", CultureInfo.InvariantCulture), Span);
    public static CfdValueNode Bool(bool value) => new CfdScalarValue(value ? "true" : "false", Span);
    public static CfdValueNode String(string value) => new CfdStringValue(value, Span);
    public static CfdValueNode None() => new CfdNoneValue(Span);
    public static CfdValueNode Scalar(string value) => new CfdScalarValue(value, Span);
    public static CfdValueNode Enum(string type, long value) => new CfdEnumDefaultValue(type, value, Span);
    public static CfdValueNode Reference(string type, string key) => new CfdReferenceValue(type, key, Span);
    public static CfdValueNode Array(params CfdValueNode[] values) => new CfdArrayValue(values, Span);
    public static CfdValueNode Object(string type, params CfdDefaultField[] fields) => new CfdObjectValue(type, fields.Select(field => new CfdFieldNode(field.Name, field.Value, Span)).ToArray(), Span);
    public static CfdValueNode Dictionary(params CfdDefaultEntry[] entries) => new CfdDictionaryValue(entries.Select(entry => new CfdEntryNode(entry.Key, entry.Value, Span)).ToArray(), Span);
    public static CfdValueNode Template(string source) => CfdParser.ParseValue(new CfdSource("<default>", source));
}
public sealed class CfdDefaultField
{
    public CfdDefaultField(string name, CfdValueNode value) { Name = name; Value = value; }
    internal string Name { get; }
    internal CfdValueNode Value { get; }
}
public sealed class CfdDefaultEntry
{
    public CfdDefaultEntry(CfdValueNode key, CfdValueNode value) { Key = key; Value = value; }
    internal CfdValueNode Key { get; }
    internal CfdValueNode Value { get; }
}

public static class CfdValues
{
    public static CfdValueType<int> Int { get; } = new CfdValueType<int>((context, node, owner) =>
    {
        if (node is CfdScalarValue scalar && int.TryParse(scalar.Value, NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture, out var value)) return value;
        throw owner.Error(node, "CFD-TYPE", "expected a 32-bit integer");
    }, formatScalar: value => value.ToString(CultureInfo.InvariantCulture));
    public static CfdValueType<float> Float { get; } = new CfdValueType<float>((context, node, owner) =>
    {
        if (node is CfdScalarValue scalar)
        {
            if (scalar.Value == "inf") return float.PositiveInfinity;
            if (scalar.Value == "-inf") return float.NegativeInfinity;
            if (float.TryParse(scalar.Value, NumberStyles.Float, CultureInfo.InvariantCulture, out var value) && !float.IsInfinity(value) && !float.IsNaN(value)) return value;
        }
        throw owner.Error(node, "CFD-TYPE", "expected a binary32 number");
    }, formatScalar: value => float.IsPositiveInfinity(value) ? "inf" : float.IsNegativeInfinity(value) ? "-inf" : value.ToString("R", CultureInfo.InvariantCulture));
    public static CfdValueType<bool> Bool { get; } = new CfdValueType<bool>((context, node, owner) => node is CfdScalarValue scalar && scalar.Value == "true" ? true : node is CfdScalarValue other && other.Value == "false" ? false : throw owner.Error(node, "CFD-TYPE", "expected bool"), formatScalar: value => value ? "true" : "false");
    public static CfdValueType<string> String { get; } = new CfdValueType<string>((context, node, owner) => node is CfdStringValue value ? value.Value : throw owner.Error(node, "CFD-TYPE", "expected ordinary string"), formatScalar: value => value);
    public static CfdValueType<string> FormattedString { get; } = new CfdValueType<string>((context, node, owner) => node is CfdFormattedStringValue value ? owner.Format(value) : throw owner.Error(node, "CFD-TYPE", "expected fstring"), formatScalar: value => value);
    public static CfdValueType<T?> Optional<T>(CfdValueType<T> type) where T : struct => new CfdValueType<T?>((context, node, owner) => node is CfdNoneValue ? (T?)null : (T)type.Read(context, node, owner)!, true, type.NamedType);
    public static CfdValueType<T?> OptionalReference<T>(CfdValueType<T> type) where T : class => new CfdValueType<T?>((context, node, owner) => node is CfdNoneValue ? null : (T)type.Read(context, node, owner)!, true, type.NamedType);
    public static CfdValueType<IReadOnlyList<T>> List<T>(CfdValueType<T> type) => new CfdValueType<IReadOnlyList<T>>((context, node, owner) => node is CfdArrayValue array ? System.Array.AsReadOnly(array.Items.Select(value => (T)type.Read(context, value, owner)!).ToArray()) : throw owner.Error(node, "CFD-TYPE", "expected array"));
    public static CfdValueType<IReadOnlyDictionary<K, V>> Dictionary<K, V>(CfdValueType<K> keyType, CfdValueType<V> valueType) where K : notnull => new CfdValueType<IReadOnlyDictionary<K, V>>((context, node, owner) =>
    {
        if (!(node is CfdDictionaryValue dictionary)) throw owner.Error(node, "CFD-TYPE", "expected dictionary");
        var values = new Dictionary<K, V>();
        foreach (var entry in dictionary.Entries)
        {
            var key = (K)keyType.Read(context, entry.Key, owner)!;
            if (values.ContainsKey(key)) throw owner.Error(entry.Key, "CFD-DUPLICATE", "duplicate dictionary key");
            values.Add(key, (V)valueType.Read(context, entry.Value, owner)!);
        }
        return new ReadOnlyDictionary<K, V>(values);
    });
    public static CfdValueType<T> Object<T>(string type) => new CfdValueType<T>((context, node, owner) => context.ReadObject<T>(type, node, owner), namedType: type);
    public static CfdValueType<T> Reference<T>(string type) where T : class => new CfdValueType<T>((context, node, owner) => context.ReadReference<T>(type, node, owner), namedType: type);
    public static CfdValueType<T> Enum<T>(string name, IReadOnlyDictionary<string, T> variants, Func<long, T>? fromBits = null) where T : struct => new CfdValueType<T>((context, node, owner) =>
    {
        if (node is CfdEnumDefaultValue numeric)
        {
            if (numeric.Type != name) throw owner.Error(node, "CFD-TYPE", "enum default type mismatch");
            foreach (var variant in variants.Values)
                if (Convert.ToInt64(variant, CultureInfo.InvariantCulture) == numeric.Value) return variant;
            if (fromBits != null)
            {
                long mask = 0;
                foreach (var variant in variants.Values) mask |= Convert.ToInt64(variant, CultureInfo.InvariantCulture);
                if ((numeric.Value & ~mask) == 0) return fromBits(numeric.Value);
            }
            throw owner.Error(node, "CFD-ENUM", "invalid enum default value");
        }
        T Parse(string text)
        {
            var separator = text.LastIndexOf("::", StringComparison.Ordinal);
            if (separator >= 0)
            {
                if (text.Substring(0, separator) != name) throw owner.Error(node, "CFD-TYPE", "enum type mismatch");
                text = text.Substring(separator + 2);
            }
            if (variants.TryGetValue(text, out var result)) return result;
            throw owner.Error(node, "CFD-ENUM", $"unknown enum variant `{text}`");
        }
        if (node is CfdScalarValue scalar) return Parse(scalar.Value);
        // 枚举运算是数据字面量的一部分，不依赖表达式执行器。
        long Bits(CfdBitExpression expression) => expression.Kind switch
        {
            CfdBitExpressionKind.Value value => Convert.ToInt64(Parse(value.Text), CultureInfo.InvariantCulture),
            CfdBitExpressionKind.Binary binary => binary.Operator switch
            {
                CfdBitOperator.Or => Bits(binary.Left) | Bits(binary.Right),
                CfdBitOperator.Xor => Bits(binary.Left) ^ Bits(binary.Right),
                _ => Bits(binary.Left) & Bits(binary.Right),
            },
            _ => throw owner.Error(node, "CFD-ENUM", "invalid enum expression"),
        };
        if (fromBits != null && node is CfdBitExpressionValue bit) return fromBits(Bits(bit.Expression));
        throw owner.Error(node, "CFD-TYPE", "expected enum value");
    }, formatScalar: value =>
    {
        // 复用生成的枚举成员描述，命名值输出成员名，组合值输出数字位模式。
        foreach (var variant in variants)
            if (EqualityComparer<T>.Default.Equals(variant.Value, value)) return variant.Key;
        return Convert.ToInt64(value, CultureInfo.InvariantCulture).ToString(CultureInfo.InvariantCulture);
    });
    public static CfdValueType<DimensionValue<T>> Dimension<T>(string dimension, CfdValueType<T> type)
    {
        DimensionValue<T> Read(LoadContext context, CfdValueNode node, CfdReader owner, IReadOnlyDictionary<string, CfdValueNode> overrides)
        {
            var defaultValue = (T)type.Read(context, node, owner)!;
            var variants = new SortedDictionary<string, T>(StringComparer.Ordinal);
            // 所有已发现变体都有有效值，显式 None 不触发基础值回退。
            foreach (var variant in context.Variants[dimension])
                variants.Add(variant, overrides.TryGetValue(variant, out var value) ? (T)type.Read(context, value, owner)! : defaultValue);
            return new DimensionValue<T>(defaultValue, new ReadOnlyDictionary<string, T>(variants));
        }
        return new CfdValueType<DimensionValue<T>>((context, node, owner) => Read(context, node, owner, new Dictionary<string, CfdValueNode>()), type.AllowsMissing, type.NamedType, dimension, Read, value => type.FormatScalar(value.Default));
    }
}
}
