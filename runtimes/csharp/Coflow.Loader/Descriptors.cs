using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.Globalization;
using System.Linq;

namespace Coflow.Loader
{
// 描述负责绑定、创建和填充；最终业务对象只保存完整字段值。
public abstract class CfdValueType
{
    internal abstract BoundValue Bind(LoadContext context, CfdValueNode node, BindingScope owner);
    internal virtual string? FormatScalar(object? value) => null;
    internal virtual bool IsOptional => false;
    internal virtual string? NamedType => null;
}

public sealed class CfdValueType<T> : CfdValueType
{
    private readonly Func<LoadContext, CfdValueNode, BindingScope, BoundValue<T>> _bind;
    private readonly Func<T, string?>? _format;
    private readonly bool _optional;
    private readonly string? _namedType;
    internal CfdValueType(Func<LoadContext, CfdValueNode, BindingScope, T> read, Func<T, string?>? formatScalar = null)
        : this((context, node, owner) => new BoundValue<T>(read(context, node, owner)), false, null, formatScalar) { }
    internal CfdValueType(Func<LoadContext, CfdValueNode, BindingScope, BoundValue<T>> bind, bool optional, string? namedType, Func<T, string?>? formatScalar = null)
    { _bind = bind; _optional = optional; _namedType = namedType; _format = formatScalar; }
    internal BoundValue<T> BindValue(LoadContext context, CfdValueNode node, BindingScope owner)
    {
        var value = _bind(context, context.ResolveConstant(node), owner);
        value.Path = owner.Path;
        value.Span = node.Span;
        return value;
    }
    internal override BoundValue Bind(LoadContext context, CfdValueNode node, BindingScope owner) => BindValue(context, node, owner);
    internal override string? FormatScalar(object? value) => _format == null ? null : _format((T)value!);
    internal override bool IsOptional => _optional;
    internal override string? NamedType => _namedType;
}

public abstract class CfdField
{
    protected CfdField(string name, CfdValueType type, CfdValueNode? defaultValue, string? dimension)
    { Name = name; Type = type; DefaultValue = defaultValue; Dimension = dimension; }
    public string Name { get; }
    internal CfdValueType Type { get; }
    internal CfdValueNode? DefaultValue { get; }
    internal string? Dimension { get; }
}
public sealed class CfdField<T> : CfdField
{
    public CfdField(string name, CfdValueType<T> type, CfdValueNode? defaultValue = null, string? dimension = null)
        : base(name, type, defaultValue, dimension) { }
}
public enum CfdTypeKind { Table, Data, Singleton }

public abstract class CfdType
{
    private readonly Dictionary<string, int> _ordinals = new Dictionary<string, int>(StringComparer.Ordinal);
    protected CfdType(string name, CfdTypeKind kind, Type valueType, IReadOnlyList<CfdField> fields, string? parent, bool abstractType)
    {
        Name = name; Kind = kind; ValueType = valueType; Parent = parent; IsAbstract = abstractType;
        Fields = new CfdField[fields.Count];
        for (var i = 0; i < Fields.Length; i++) { Fields[i] = fields[i]; _ordinals.Add(Fields[i].Name, i); }
    }
    public string Name { get; }
    internal CfdField[] Fields { get; }
    internal int FindOrdinal(string name) => _ordinals.TryGetValue(name, out var ordinal) ? ordinal : -1;
    internal CfdTypeKind Kind { get; }
    internal Type ValueType { get; }
    internal bool IsAbstract { get; }
    internal string? Parent { get; }
    internal abstract object Create(string id);
    internal abstract object Fill(object value, CfdReader reader);
    internal abstract object BuildTable(IReadOnlyList<RecordState> records, BuildSession session);
    internal abstract object BuildDimensions(IReadOnlyList<RecordState> records, LoadContext context);
    internal abstract object BuildDimension(RecordState record, LoadContext context);
}
public sealed class CfdType<T> : CfdType
{
    private readonly Func<string, T>? _create;
    private readonly Func<T, CfdReader, T>? _fill;
    public CfdType(string name, CfdTypeKind kind, IReadOnlyList<CfdField> fields, Func<string, T>? create, Func<T, CfdReader, T>? fill, string? parent = null, bool abstractType = false)
        : base(name, kind, typeof(T), fields, parent, abstractType) { _create = create; _fill = fill; }
    internal override object Create(string id) => _create!(id)!;
    internal override object Fill(object value, CfdReader reader) => _fill!((T)value, reader)!;
    internal override object BuildTable(IReadOnlyList<RecordState> records, BuildSession session)
        => new Table<T>(records.ToDictionary(record => record.Id, record => (T)session.Reader(record.Data).Value, StringComparer.Ordinal));
    internal override object BuildDimensions(IReadOnlyList<RecordState> records, LoadContext context)
        => new Table<Dimension<T>>(records.ToDictionary(record => record.Id, record => (Dimension<T>)BuildDimension(record, context), StringComparer.Ordinal));
    internal override object BuildDimension(RecordState record, LoadContext context)
    {
        var overrides = new Dictionary<string, HashSet<string>>(StringComparer.Ordinal);
        for (var i = 0; i < record.Data.Type.Fields.Length; i++)
        {
            var field = record.Data.Type.Fields[i];
            if (field.Dimension == null) continue;
            var variants = record.Data.Overrides != null && record.Data.Overrides.TryGetValue(i, out var values)
                ? new HashSet<string>(values.Keys, StringComparer.Ordinal) : new HashSet<string>(StringComparer.Ordinal);
            overrides.Add(field.Name, variants);
        }
        return new Dimension<T>(context.Catalog, context.Sessions.Select(session => (T)session.Reader(record.Data).Value).ToArray(), overrides);
    }
}

public static class CfdDefaults
{
    private static readonly CfdSpan Span = new CfdSpan(0, 0, 0, 0);
    public static CfdValueNode Int(long value) => new CfdScalarValue(value.ToString(CultureInfo.InvariantCulture), Span, 0);
    public static CfdValueNode Long(long value) => new CfdScalarValue(value.ToString(CultureInfo.InvariantCulture), Span, 1);
    public static CfdValueNode Double(double value) => new CfdScalarValue(value.ToString("R", CultureInfo.InvariantCulture), Span, 3);
    public static CfdValueNode Float(double value) => new CfdScalarValue(value.ToString("R", CultureInfo.InvariantCulture), Span, 2);
    public static CfdValueNode Bool(bool value) => new CfdScalarValue(value ? "true" : "false", Span);
    public static CfdValueNode String(string value) => new CfdStringValue(value, Span);
    public static CfdValueNode None() => new CfdNoneValue(Span);
    public static CfdValueNode Some(CfdValueNode value) => new CfdSomeValue(value, Span);
    public static CfdValueNode Scalar(string value) => new CfdScalarValue(value, Span);
    public static CfdValueNode Enum(string type, long value) => new CfdEnumDefaultValue(type, value, Span);
    public static CfdValueNode Reference(string type, string id) => new CfdReferenceValue(type, id, Span);
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
        if (node is CfdScalarValue scalar && (scalar.NumericRank == null || scalar.NumericRank <= 0) && int.TryParse(scalar.Value, NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture, out var value)) return value;
        throw owner.Error(node, "CFD-TYPE", "expected a 32-bit integer");
    }, formatScalar: value => value.ToString(CultureInfo.InvariantCulture));
    public static CfdValueType<float> Float { get; } = new CfdValueType<float>((context, node, owner) =>
    {
        // 先按 binary64 读取再舍入，与 Rust 的字面量及模型边界保持一致。
        if (node is CfdScalarValue scalar && (scalar.NumericRank == null || scalar.NumericRank <= 2) && double.TryParse(scalar.Value, NumberStyles.Float, CultureInfo.InvariantCulture, out var parsed))
        {
            var value = (float)parsed;
            if (!float.IsInfinity(value) && !float.IsNaN(value)) return value;
        }
        throw owner.Error(node, "CFD-TYPE", "expected a finite binary32 number");
    }, formatScalar: value => FormatFloat(value));
    public static CfdValueType<long> Long { get; } = new CfdValueType<long>((context, node, owner) =>
    {
        if (node is CfdScalarValue scalar && (scalar.NumericRank == null || scalar.NumericRank <= 1) && long.TryParse(scalar.Value, NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture, out var value)) return value;
        throw owner.Error(node, "CFD-TYPE", "expected a 64-bit integer");
    }, formatScalar: value => value.ToString(CultureInfo.InvariantCulture));
    public static CfdValueType<double> Double { get; } = new CfdValueType<double>((context, node, owner) =>
    {
        if (node is CfdScalarValue scalar && double.TryParse(scalar.Value, NumberStyles.Float, CultureInfo.InvariantCulture, out var value) && !double.IsInfinity(value) && !double.IsNaN(value)) return value;
        throw owner.Error(node, "CFD-TYPE", "expected a finite binary64 number");
    }, formatScalar: FormatFloat);
    // Rust 的基础插值使用十进制最短文本；展开指数表示，保证跨语言文本一致。
    private static string FormatFloat(double value)
    {
        if (value == 0) return BitConverter.DoubleToInt64Bits(value) < 0 ? "-0" : "0";
        var text = value.ToString("R", CultureInfo.InvariantCulture);
        var exponentIndex = text.IndexOf('E');
        if (exponentIndex < 0) return text;
        var negative = text[0] == '-';
        var mantissa = text.Substring(negative ? 1 : 0, exponentIndex - (negative ? 1 : 0));
        var decimalIndex = mantissa.IndexOf('.');
        var digits = mantissa.Replace(".", "");
        var position = (decimalIndex < 0 ? mantissa.Length : decimalIndex)
            + int.Parse(text.Substring(exponentIndex + 1), CultureInfo.InvariantCulture);
        var expanded = position <= 0 ? "0." + new string('0', -position) + digits
            : position >= digits.Length ? digits + new string('0', position - digits.Length)
            : digits.Insert(position, ".");
        return negative ? "-" + expanded : expanded;
    }

    public static CfdValueType<bool> Bool { get; } = new CfdValueType<bool>((context, node, owner) => node is CfdScalarValue scalar && scalar.Value == "true" ? true : node is CfdScalarValue other && other.Value == "false" ? false : throw owner.Error(node, "CFD-TYPE", "expected bool"), formatScalar: value => value ? "true" : "false");
    public static CfdValueType<string> String { get; } = new CfdValueType<string>((context, node, owner) =>
    {
        if (node is CfdStringValue value) return new BoundValue<string>(value.Value);
        if (node is CfdFormattedStringValue template) return context.BindTemplate(template, owner);
        throw owner.Error(node, "CFD-TYPE", "expected string");
    }, false, null, value => value);
    public static CfdValueType<Option<T>> Optional<T>(CfdValueType<T> type) => new CfdValueType<Option<T>>((context, node, owner) =>
    {
        if (node is CfdNoneValue) return new BoundValue<Option<T>>(Option<T>.None);
        var inner = type.BindValue(context, node is CfdSomeValue some ? some.Value : node, owner);
        return inner.IsFixed ? new BoundValue<Option<T>>(Option<T>.Some(inner.Fixed))
            : new BoundValue<Option<T>>(reader => Option<T>.Some(inner.Get(reader)));
    }, true, type.NamedType);
    public static CfdValueType<IReadOnlyList<T>> List<T>(CfdValueType<T> type) => new CfdValueType<IReadOnlyList<T>>((context, node, owner) =>
    {
        if (!(node is CfdArrayValue array)) throw owner.Error(node, "CFD-TYPE", "expected array");
        var items = array.Items.Select(item => type.BindValue(context, item, owner)).ToArray();
        if (items.All(item => item.IsFixed)) return new BoundValue<IReadOnlyList<T>>(System.Array.AsReadOnly(items.Select(item => item.Fixed).ToArray()));
        return new BoundValue<IReadOnlyList<T>>(reader => System.Array.AsReadOnly(items.Select(item => item.Get(reader)).ToArray()));
    }, false, null);
    public static CfdValueType<IReadOnlyDictionary<K, V>> Dictionary<K, V>(CfdValueType<K> keyType, CfdValueType<V> valueType) where K : notnull
        => new CfdValueType<IReadOnlyDictionary<K, V>>((context, node, owner) =>
    {
        if (!(node is CfdDictionaryValue dictionary)) throw owner.Error(node, "CFD-TYPE", "expected dictionary");
        var keys = dictionary.Entries.Select(entry => keyType.BindValue(context, entry.Key, owner)).ToArray();
        var values = dictionary.Entries.Select(entry => valueType.BindValue(context, entry.Value, owner)).ToArray();
        var spans = dictionary.Entries.Select(entry => entry.Span).ToArray();
        Dictionary<K, int> Index(CfdReader reader)
        {
            var index = new Dictionary<K, int>(keys.Length);
            for (var i = 0; i < keys.Length; i++)
            {
                if (!index.TryAdd(keys[i].Get(reader), i)) throw LoadContext.Error("CFD-DUPLICATE", "duplicate dictionary key", owner.Path, spans[i]);
            }
            return index;
        }
        if (keys.All(key => key.IsFixed))
        {
            var index = Index(null!);
            if (values.All(value => value.IsFixed))
                return new BoundValue<IReadOnlyDictionary<K, V>>(new ReadOnlyDictionary<K, V>(index.ToDictionary(pair => pair.Key, pair => values[pair.Value].Fixed)));
            return new BoundValue<IReadOnlyDictionary<K, V>>(reader => new ReadOnlyDictionary<K, V>(index.ToDictionary(pair => pair.Key, pair => values[pair.Value].Get(reader))));
        }
        return new BoundValue<IReadOnlyDictionary<K, V>>(reader => new ReadOnlyDictionary<K, V>(Index(reader).ToDictionary(pair => pair.Key, pair => values[pair.Value].Get(reader))));
    }, false, null);
    public static CfdValueType<T> Object<T>(string type) => new CfdValueType<T>((context, node, owner) => context.BindObject<T>(type, node, owner), false, type);
    public static CfdValueType<T> Reference<T>(string type) where T : class => new CfdValueType<T>((context, node, owner) => context.BindReference<T>(type, node, owner), false, type);
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
            if (fromBits != null && long.TryParse(text, NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture, out var bits))
            {
                long mask = 0;
                foreach (var variant in variants.Values) mask |= Convert.ToInt64(variant, CultureInfo.InvariantCulture);
                if (bits >= 0 && (bits & ~mask) == 0) return fromBits(bits);
            }
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
}
}
