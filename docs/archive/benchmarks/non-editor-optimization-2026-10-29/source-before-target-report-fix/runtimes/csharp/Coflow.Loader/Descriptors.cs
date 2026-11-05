using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.Globalization;
using System.Linq;
using System.Runtime.CompilerServices;

namespace Coflow.Loader
{
// 描述仅包含类型与构造协议，解析算法由加载器统一提供。
public abstract class CfdValueType
{
    internal abstract object? Read(LoadContext context, CfdValueNode node, CfdReader owner);
    internal virtual string? FormatScalar(object? value) => null;
    internal virtual bool IsOptional => false;
    internal virtual string? Dimension => null;
    internal virtual string? NamedType => null;
    internal virtual bool HasReferenceDependency => false;
    internal virtual RecordState? ReferenceDependency(LoadContext context, CfdValueNode node, CfdReader owner) => null;
}

public sealed class CfdValueType<T> : CfdValueType
{
    private readonly Func<LoadContext, CfdValueNode, CfdReader, T> _read;
    private readonly bool _optional;
    private readonly Func<T, string?>? _formatScalar;
    private readonly string? _namedType;
    private readonly string? _dimension;
    private readonly int _constantReads;
    private readonly Func<LoadContext, CfdValueNode, CfdReader, RecordState?>? _referenceDependency;
    internal CfdValueType(Func<LoadContext, CfdValueNode, CfdReader, T> read, bool optional = false, string? namedType = null, string? dimension = null, Func<T, string?>? formatScalar = null, int constantReads = 1, Func<LoadContext, CfdValueNode, CfdReader, RecordState?>? referenceDependency = null)
    { _read = read; _optional = optional; _namedType = namedType; _dimension = dimension; _formatScalar = formatScalar; _constantReads = constantReads; _referenceDependency = referenceDependency; }
    internal override string? FormatScalar(object? value) => _formatScalar == null ? null : _formatScalar((T)value!);
    internal override string? Dimension => _dimension;
    [MethodImpl(MethodImplOptions.AggressiveInlining)]
    internal override object? Read(LoadContext context, CfdValueNode node, CfdReader owner)
    {
        for (var index = 0; index < _constantReads; index++) node = context.ResolveConstant(node);
        return _read(context, node, owner);
    }
    // 维度仅添加元数据，不增加委托递归层；常量解析次数保持原包装链的语义。
    internal CfdValueType<T> WithDimension(string dimension)
        => new CfdValueType<T>(_read, _optional, _namedType, dimension, _formatScalar, checked(_constantReads + 1), _referenceDependency);
    internal override string? NamedType => _namedType;
    internal override bool IsOptional => _optional;
    internal override bool HasReferenceDependency => _referenceDependency != null;
    internal override RecordState? ReferenceDependency(LoadContext context, CfdValueNode node, CfdReader owner)
    {
        // 依赖探测与实际读取逐包装解析相同次数的常量，不能提前穿透额外别名。
        for (var index = 0; index < _constantReads; index++) node = context.ResolveConstant(node);
        return _referenceDependency!(context, node, owner);
    }
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

public enum CfdTypeKind { Table, Data, Singleton }

public abstract class CfdType
{
    private readonly Dictionary<string, int> _fieldSlots;
    protected CfdType(string name, CfdTypeKind kind, Type valueType, IReadOnlyList<CfdField> fields, string? parent, bool abstractType)
    {
        Name = name; Kind = kind; ValueType = valueType; Parent = parent; IsAbstract = abstractType;
        // 字段顺序与名称索引属于 schema，一次建立后由所有记录和维度上下文共享。
        Fields = System.Array.AsReadOnly(fields.ToArray());
        HasReferenceDependencies = Fields.Any(field => field.Type.HasReferenceDependency);
        _fieldSlots = new Dictionary<string, int>(Fields.Count, StringComparer.Ordinal);
        for (var index = 0; index < Fields.Count; index++)
            if (!_fieldSlots.ContainsKey(Fields[index].Name)) _fieldSlots.Add(Fields[index].Name, index);
    }
    public string Name { get; }
    internal IReadOnlyList<CfdField> Fields { get; }
    internal bool HasReferenceDependencies { get; }
    internal int FieldSlot(string name) => _fieldSlots.TryGetValue(name, out var slot) ? slot : -1;
    internal CfdField? Field(string name) => _fieldSlots.TryGetValue(name, out var slot) ? Fields[slot] : null;
    internal CfdTypeKind Kind { get; }
    internal Type ValueType { get; }
    internal abstract object BuildTable(IReadOnlyList<RecordState> records);
    internal abstract object BuildSingleton(IReadOnlyList<RecordState> records);
    internal bool IsAbstract { get; }
    internal string? Parent { get; }
    internal abstract object CreateRecord(string id);
    internal abstract void FillRecord(object value, CfdReader reader);
    internal abstract object Construct(CfdReader reader);
}

public sealed class CfdType<T> : CfdType
{
    private readonly Func<string, T>? _create;
    private readonly Action<T, CfdReader>? _fill;
    private readonly Func<CfdReader, T>? _construct;
    // 声明类别明确选择记录身份或内联构造，注册时生成强类型表与单例。
    public CfdType(string name, CfdTypeKind kind, IReadOnlyList<CfdField> fields, Func<string, T>? create, Action<T, CfdReader>? fill, Func<CfdReader, T>? construct, string? parent = null, bool abstractType = false)
        : base(name, kind, typeof(T), fields, parent, abstractType) { _create = create; _fill = fill; _construct = construct; }
    internal override object CreateRecord(string id) => _create!(id)!;
    internal override void FillRecord(object value, CfdReader reader)
    {
        if (value is ICfdRecord record) record.Reader = reader;
        _fill!((T)value, reader);
    }
    internal override object Construct(CfdReader reader) => _construct!(reader)!;
    internal override object BuildTable(IReadOnlyList<RecordState> records)
    {
        // 保持原来的字典增长容量，避免精确预分配提高装载率而拖慢长期查询。
        var values = new Dictionary<string, T>(StringComparer.Ordinal);
        foreach (var record in records) if (record.Value is T value) values.Add(record.Id, value);
        return new Table<T>(values);
    }
    internal override object BuildSingleton(IReadOnlyList<RecordState> records)
        => records.Single(record => record.Value is T).Value;
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
    public static CfdValueType<string> String { get; } = new CfdValueType<string>((context, node, owner) => node is CfdStringValue value ? value.Value : node is CfdFormattedStringValue template ? owner.Format(template) : throw owner.Error(node, "CFD-TYPE", "expected string"), formatScalar: value => value);
    public static CfdValueType<Option<T>> Optional<T>(CfdValueType<T> type) => new CfdValueType<Option<T>>((context, node, owner) =>
    {
        if (node is CfdNoneValue) return Option<T>.None;
        // 裸值与 Some(value) 均只物化一层，内部 None 留给下一层描述读取。
        var inner = node is CfdSomeValue some ? some.Value : node;
        return Option<T>.Some((T)type.Read(context, inner, owner)!);
    }, true, type.NamedType, referenceDependency: type.HasReferenceDependency
        ? (context, node, owner) => node is CfdNoneValue ? null
            : type.ReferenceDependency(context, node is CfdSomeValue some ? some.Value : node, owner)
        : (Func<LoadContext, CfdValueNode, CfdReader, RecordState?>?)null);
    public static CfdValueType<IReadOnlyList<T>> List<T>(CfdValueType<T> type)
        => new CfdValueType<IReadOnlyList<T>>((context, node, owner) => owner.ReadList(type, node));
    public static CfdValueType<IReadOnlyDictionary<K, V>> Dictionary<K, V>(CfdValueType<K> keyType, CfdValueType<V> valueType) where K : notnull
        => new CfdValueType<IReadOnlyDictionary<K, V>>((context, node, owner) => owner.ReadDictionary(keyType, valueType, node));
    public static CfdValueType<T> Object<T>(string type) => new CfdValueType<T>((context, node, owner) => context.ReadObject<T>(type, node, owner), namedType: type);
    public static CfdValueType<T> Reference<T>(string type) where T : class
        => new CfdValueType<T>((context, node, owner) => context.ReadReference<T>(type, node, owner), namedType: type,
            referenceDependency: (context, node, owner) => context.ReferenceDependency(type, node, owner));
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
    public static CfdValueType<T> Dimension<T>(string dimension, CfdValueType<T> type)
        => type.WithDimension(dimension);
}
}
