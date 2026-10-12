using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.Linq;
using System.Text;

namespace Coflow.Loader
{
public sealed class LoadResult<T> where T : class
{
    private readonly T? _data;
    internal LoadResult(T? data, IReadOnlyList<CfdDiagnostic> diagnostics) { _data = data; Diagnostics = diagnostics; }
    public bool Success => _data != null;
    public T Data => _data ?? throw new InvalidOperationException("Loading failed; inspect Diagnostics.");
    public IReadOnlyList<CfdDiagnostic> Diagnostics { get; }
}

public static class CfdLoader
{
    public static LoadResult<T> Load<T>(IEnumerable<CfdSource> sources, IReadOnlyList<CfdType> types, Func<CfdData, T> construct, IReadOnlyDictionary<string, CfdValueNode>? constants = null) where T : class
    {
        if (sources == null || types == null || construct == null) throw new ArgumentNullException();
        try
        {
            var context = new LoadContext(CfdParser.ParseAll(sources), types, constants);
            context.Build();
            // 对外数据只含生成对象；构建上下文和解析节点不进入结果。
            return new LoadResult<T>(construct(new CfdData(types, context.Records)), System.Array.Empty<CfdDiagnostic>());
        }
        catch (CfdLoadException error) { return new LoadResult<T>(null, error.Diagnostics); }
    }
}

public sealed class CfdData
{
    private readonly Dictionary<Type, object> _tables = new Dictionary<Type, object>();
    private readonly Dictionary<Type, object> _singletons = new Dictionary<Type, object>();
    internal CfdData(IReadOnlyList<CfdType> types, IReadOnlyList<RecordState> records)
    {
        // 注册类别决定可访问入口，data 不进入记录表；重复获取返回同一只读表。
        foreach (var type in types)
        {
            if (type.Kind == CfdTypeKind.Table) _tables.Add(type.ValueType, type.BuildTable(records));
            if (type.Kind == CfdTypeKind.Singleton) _singletons.Add(type.ValueType, type.BuildSingleton(records));
        }
    }
    public Table<T> Table<T>() where T : class => _tables.TryGetValue(typeof(T), out var table)
        ? (Table<T>)table : throw new InvalidOperationException($"`{typeof(T).Name}` is not a table in this schema.");
    public T Singleton<T>() where T : class => _singletons.TryGetValue(typeof(T), out var value)
        ? (T)value : throw new InvalidOperationException($"`{typeof(T).Name}` is not a singleton in this schema.");
}

internal sealed class RecordState
{
    internal RecordState(CfdType type, CfdRecordNode node, string path) { Type = type; Node = node; Path = path; Value = type.CreateRecord(node.Key); }
    internal CfdType Type { get; }
    internal CfdRecordNode Node { get; }
    internal string Path { get; }
    internal object Value { get; }
    internal CfdReader Reader { get; set; } = null!;
}

internal sealed class LoadContext
{
    private readonly Dictionary<string, CfdType> _types;
    private readonly Dictionary<(string Root, string Key), RecordState> _recordsByIdentity = new Dictionary<(string Root, string Key), RecordState>();
    private readonly Dictionary<string, List<RecordState>> _recordsByKey = new Dictionary<string, List<RecordState>>(StringComparer.Ordinal);
    private readonly IReadOnlyDictionary<string, CfdValueNode> _constants;
    internal CfdValueNode ResolveConstant(CfdValueNode node) => node is CfdScalarValue scalar && _constants.TryGetValue(scalar.Value, out var value) ? value : node;
    private int _depth;
    internal void EnterRead(string path, CfdSpan span)
    {
        if (++_depth > 256) { _depth--; throw Error("CFD-LIMIT", "construction nesting exceeds 256 levels", path, span); }
    }
    internal void ExitRead() { _depth--; }
    private readonly Dictionary<CfdValueNode, string> _sources = new Dictionary<CfdValueNode, string>();
    internal List<RecordState> Records { get; } = new List<RecordState>();
    internal Dictionary<string, SortedSet<string>> Variants { get; } = new Dictionary<string, SortedSet<string>>(StringComparer.Ordinal);
    internal LoadContext(IReadOnlyList<CfdDocument> documents, IReadOnlyList<CfdType> types, IReadOnlyDictionary<string, CfdValueNode>? constants)
    {
        _constants = constants ?? new Dictionary<string, CfdValueNode>(StringComparer.Ordinal);
        _types = types.ToDictionary(type => type.Name, StringComparer.Ordinal);
        foreach (var type in types)
            foreach (var field in type.Fields)
                if (field.Type.Dimension is string dimension && !Variants.ContainsKey(dimension)) Variants.Add(dimension, new SortedSet<string>(StringComparer.Ordinal));
        // 全部记录先建立身份，基础文件与覆盖文件顺序不影响解析结果。
        foreach (var document in documents)
            foreach (var node in document.Records)
            {
                if (!_types.TryGetValue(node.DeclaredType, out var type) || type.Kind == CfdTypeKind.Data || type.IsAbstract)
                    throw Error("CFD-TYPE", $"invalid record type `{node.DeclaredType}`", document.Path, node.Span);
                // 分组声明限定实际类型，未知分组与无关类型不能绕过继承校验。
                if (node.GroupType != null && (!_types.TryGetValue(node.GroupType, out var group) || group.Kind != CfdTypeKind.Table || !Assignable(type, node.GroupType)))
                    throw Error("CFD-TYPE", $"record type `{type.Name}` is not assignable to group `{node.GroupType}`", document.Path, node.Span);
                var identity = (Root(type), node.Key);
                if (_recordsByIdentity.ContainsKey(identity))
                    throw Error("CFD-DUPLICATE", $"duplicate record `{node.Key}`", document.Path, node.Span);
                var record = new RecordState(type, node, document.Path);
                Records.Add(record);
                // 引用按继承树与 key 建立临时索引，加载过程不逐条扫描全部记录。
                _recordsByIdentity.Add(identity, record);
                if (!_recordsByKey.TryGetValue(node.Key, out var matching))
                    _recordsByKey.Add(node.Key, matching = new List<RecordState>());
                matching.Add(record);
            }
        foreach (var type in types.Where(type => type.Kind == CfdTypeKind.Singleton))
        {
            var records = Records.Where(record => Assignable(record.Type, type.Name)).ToArray();
            if (records.Length != 1)
                throw Error("CFD-SINGLETON", $"singleton `{type.Name}` requires exactly one record", "", default);
            // 单例身份来自声明短名，和 Rust 数据模型使用同一条 key 规则。
            var key = type.Name.Substring(type.Name.LastIndexOf('.') + 1);
            if (records[0].Node.Key != key)
                throw Error("CFD-SINGLETON", $"singleton `{type.Name}` requires record key `{key}`", records[0].Path, records[0].Node.Span);
        }
        var overrides = new Dictionary<RecordState, Dictionary<string, Dictionary<string, CfdValueNode>>>();
        foreach (var document in documents)
            foreach (var block in document.Dimensions)
            {
                if (!Variants.TryGetValue(block.Dimension, out var variants))
                    throw Error("CFD-DIMENSION", $"unknown dimension `{block.Dimension}`", document.Path, block.Span);
                variants.Add(block.Variant);
                foreach (var target in block.Records)
                {
                    var record = Find(target.DeclaredType, target.Key, document.Path, target.Span);
                    if (!overrides.TryGetValue(record, out var fields)) overrides.Add(record, fields = new Dictionary<string, Dictionary<string, CfdValueNode>>(StringComparer.Ordinal));
                    foreach (var field in target.Fields)
                    {
                        var descriptor = record.Type.Fields.FirstOrDefault(candidate => candidate.Name == field.Name);
                        if (descriptor == null || descriptor.Type.Dimension != block.Dimension)
                            throw Error("CFD-DIMENSION", $"field `{field.Name}` is not bound to dimension `{block.Dimension}`", document.Path, field.Span);
                        if (!fields.TryGetValue(field.Name, out var values)) fields.Add(field.Name, values = new Dictionary<string, CfdValueNode>(StringComparer.Ordinal));
                        if (values.ContainsKey(block.Variant)) throw Error("CFD-DUPLICATE-OVERLAY", $"duplicate override `{block.Dimension}[{block.Variant}]::{target.Key}.{field.Name}`", document.Path, field.Span);
                        values.Add(block.Variant, field.Value);
                        Track(field.Value, document.Path);
                    }
                }
            }
        foreach (var record in Records)
        {
            foreach (var field in record.Node.Fields) Track(field.Value, record.Path);
            overrides.TryGetValue(record, out var fields);
            record.Reader = new CfdReader(this, record.Type, record.Node.Fields, record.Path, record.Node.Span, record.Node.Key, fields);
        }
    }
    private void Track(CfdValueNode node, string path)
    {
        _sources[node] = path;
        if (node is CfdSomeValue some) Track(some.Value, path);
        if (node is CfdObjectValue obj) foreach (var field in obj.Fields) Track(field.Value, path);
        if (node is CfdArrayValue array) foreach (var value in array.Items) Track(value, path);
        if (node is CfdDictionaryValue dictionary) foreach (var entry in dictionary.Entries) { Track(entry.Key, path); Track(entry.Value, path); }
    }
    internal string Source(CfdValueNode node, string fallback) => _sources.TryGetValue(node, out var path) ? path : fallback;
    private string Root(CfdType type) => type.Parent == null ? type.Name : Root(_types[type.Parent]);
    internal bool Assignable(CfdType actual, string expected) => actual.Name == expected || actual.Parent != null && Assignable(_types[actual.Parent], expected);
    internal RecordState Find(string? type, string key, string path, CfdSpan span)
    {
        if (type != null)
        {
            if (_types.TryGetValue(type, out var expected)
                && _recordsByIdentity.TryGetValue((Root(expected), key), out var record)
                && Assignable(record.Type, type)) return record;
        }
        else if (_recordsByKey.TryGetValue(key, out var matching) && matching.Count == 1)
            return matching[0];
        throw Error("CFD-REFERENCE", $"record `{type}::{key}` is missing or ambiguous", path, span);
    }
    internal void Build()
    {
        foreach (var record in Records) { record.Reader.Validate(); record.Type.FillRecord(record.Value, record.Reader); }
    }
    internal T ReadReference<T>(string expected, CfdValueNode node, CfdReader owner) where T : class
    {
        if (!(node is CfdReferenceValue reference)) throw owner.Error(node, "CFD-TYPE", "expected record reference");
        var record = Find(reference.TypeName ?? expected, reference.Key, Source(node, owner.Path), node.Span);
        if (!Assignable(record.Type, expected) || !(record.Value is T value)) throw owner.Error(node, "CFD-TYPE", "reference type mismatch");
        return value;
    }
    internal T ReadObject<T>(string expected, CfdValueNode node, CfdReader owner)
    {
        var reader = owner.ObjectReader(expected, node);
        reader.Validate();
        if (reader.Type.Construct(reader) is T result) return result;
        throw owner.Error(node, "CFD-TYPE", "generated object type mismatch");
    }
    internal CfdReader CreateObjectReader(string expected, CfdValueNode node, CfdReader owner)
    {
        string? actual;
        IReadOnlyList<CfdFieldNode> fields;
        if (node is CfdObjectValue obj) { actual = obj.DeclaredType; fields = obj.Fields; }
        else if (node is CfdDictionaryValue dictionary)
        {
            actual = null;
            fields = dictionary.Entries.Select(entry => new CfdFieldNode(entry.Key is CfdScalarValue key ? key.Value : entry.Key is CfdStringValue text ? text.Value : throw owner.Error(entry.Key, "CFD-TYPE", "expected field name"), entry.Value, entry.Span)).ToArray();
        }
        else throw owner.Error(node, "CFD-TYPE", "expected inline data object");
        if (!_types.TryGetValue(actual ?? expected, out var type) || type.Kind != CfdTypeKind.Data || type.IsAbstract || !Assignable(type, expected)) throw owner.Error(node, "CFD-TYPE", "inline object type mismatch");
        return new CfdReader(this, type, fields, Source(node, owner.Path), node.Span);
    }
    internal static CfdLoadException Error(string code, string message, string path, CfdSpan span) => new CfdLoadException(new[] { new CfdDiagnostic(code, message, path, span) });
}

public sealed class CfdReader
{
    private readonly LoadContext _context;
    private readonly CfdType _type;
    private readonly CfdSpan _span;
    private readonly string? _key;
    private readonly IReadOnlyDictionary<string, CfdValueNode> _nodes;
    private readonly IReadOnlyDictionary<string, Dictionary<string, CfdValueNode>> _overrides;
    private readonly Dictionary<string, object?> _values = new Dictionary<string, object?>(StringComparer.Ordinal);
    private readonly HashSet<string> _resolving = new HashSet<string>(StringComparer.Ordinal);
    private readonly Dictionary<CfdValueNode, CfdReader> _objects = new Dictionary<CfdValueNode, CfdReader>();
    internal CfdType Type => _type;
    internal CfdReader ObjectReader(string expected, CfdValueNode node)
    {
        if (!_objects.TryGetValue(node, out var reader)) _objects.Add(node, reader = _context.CreateObjectReader(expected, node, this));
        return reader;
    }
    internal void Validate() { foreach (var field in _type.Fields) Read(field.Name); }
    internal string Path { get; }
    internal CfdReader(LoadContext context, CfdType type, IReadOnlyList<CfdFieldNode> nodes, string path, CfdSpan span, string? key = null, IReadOnlyDictionary<string, Dictionary<string, CfdValueNode>>? overrides = null)
    {
        _context = context; _type = type; _span = span; _key = key; Path = path;
        // 未标类型的内联字典也要给出来源诊断，重复字段不能泄漏 Dictionary 异常。
        var fieldsByName = new Dictionary<string, CfdValueNode>(StringComparer.Ordinal);
        foreach (var field in nodes)
        {
            if (fieldsByName.ContainsKey(field.Name)) throw Error(field.Value, "CFD-DUPLICATE-FIELD", $"duplicate field `{field.Name}`");
            fieldsByName.Add(field.Name, field.Value);
        }
        _nodes = fieldsByName;
        _overrides = overrides ?? new Dictionary<string, Dictionary<string, CfdValueNode>>();
        foreach (var field in nodes)
            if (!type.Fields.Any(descriptor => descriptor.Name == field.Name)) throw Error(field.Value, "CFD-FIELD", $"unknown field `{field.Name}`");
    }
    public T Get<T>(string field) => (T)Read(field)!;
    private object? Read(string field)
    {
        if (field == "id" && _key != null) return _key;
        if (_values.TryGetValue(field, out var value)) return value;
        if (!_resolving.Add(field)) throw LoadContext.Error("CFD-INTERPOLATION-CYCLE", $"cyclic field dependency `{field}`", Path, _span);
        _context.EnterRead(Path, _span);
        try
        {
            var descriptor = _type.Fields.FirstOrDefault(candidate => candidate.Name == field) ?? throw LoadContext.Error("CFD-FIELD", $"unknown field `{field}`", Path, _span);
            if (!_nodes.TryGetValue(field, out var node)) node = descriptor.DefaultValue;
            if (node == null)
            {
                if (!descriptor.Type.AllowsMissing) throw LoadContext.Error("CFD-MISSING", $"missing required field `{field}`", Path, _span);
                node = CfdDefaults.None();
            }
            if (descriptor.Type.Dimension is string dimension)
            {
                _overrides.TryGetValue(field, out var overrides);
                value = descriptor.Type.ReadDimension(_context, node, this, dimension, overrides ?? new Dictionary<string, CfdValueNode>());
            }
            else value = descriptor.Type.Read(_context, node, this);
            _values.Add(field, value);
            return value;
        }
        finally { _resolving.Remove(field); _context.ExitRead(); }
    }
    internal CfdLoadException Error(CfdValueNode node, string code, string message) => LoadContext.Error(code, message, _context.Source(node, Path), node.Span);
    internal string Format(CfdFormattedStringValue formatted)
    {
        var output = new StringBuilder();
        foreach (var segment in formatted.Segments)
        {
            if (segment is CfdFormatText text) { output.Append(text.Text); continue; }
            var reference = (CfdFormatReference)segment;
            var owner = reference.Key == null ? this : _context.Find(reference.TypeName, reference.Key, _context.Source(formatted, Path), formatted.Span).Reader;
            var path = reference.Key == null ? reference.Path.Skip(1).ToArray() : reference.Path.ToArray();
            // 是否可插值由静态字段描述决定，不靠装箱值的形状放宽可选类型。
            var value = owner.ReadPath(path, formatted);
            output.Append(value ?? throw Error(formatted, "CFD-INTERPOLATION", "interpolation requires a string, number, bool, enum or fstring field"));
        }
        return output.ToString();
    }
    private string? ReadPath(IReadOnlyList<string> path, CfdValueNode template)
    {
        if (path.Count == 0) throw Error(template, "CFD-INTERPOLATION", "empty field path");
        var descriptor = _type.Fields.FirstOrDefault(field => field.Name == path[0]);
        if (path.Count == 1)
        {
            var value = Read(path[0]);
            if (path[0] == "id" && _key != null) return _key;
            return descriptor?.Type.FormatScalar(value);
        }
        if (descriptor?.Type.AllowsMissing == true)
            throw Error(template, "CFD-INTERPOLATION", "field path cannot traverse an optional value");
        // 路径依赖沿构建材料解析，避免使用反射访问尚未填充的业务属性。
        if (!_nodes.TryGetValue(path[0], out var node)) node = _type.Fields.FirstOrDefault(field => field.Name == path[0])?.DefaultValue;
        if (node != null) node = _context.ResolveConstant(node);
        var tail = path.Skip(1).ToArray();
        if (node is CfdReferenceValue reference) return _context.Find(reference.TypeName ?? descriptor?.Type.NamedType, reference.Key, _context.Source(node, Path), node.Span).Reader.ReadPath(tail, template);
        if (node != null && descriptor?.Type.NamedType is string expected) return ObjectReader(expected, node).ReadPath(tail, template);
        throw Error(template, "CFD-INTERPOLATION", "field path must traverse data or a record reference");
    }
}
}
