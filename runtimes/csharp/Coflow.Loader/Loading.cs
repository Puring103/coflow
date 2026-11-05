using System;
using System.Collections.Generic;
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
    public static LoadResult<T> Load<T>(IEnumerable<CfdSource> sources, IReadOnlyList<CfdType> types, Func<CfdData, T> construct, IReadOnlyDictionary<string, CfdValueNode>? constants = null, LoadOptions? options = null) where T : class
    {
        if (sources == null || types == null || construct == null) throw new ArgumentNullException();
        try
        {
            var context = new LoadContext(CfdParser.ParseAll(sources), types, constants, options);
            context.Build();
            // 全部上下文构建成功后才交付；最终数据不保留构建状态。
            return new LoadResult<T>(construct(new CfdData(types, context)), System.Array.Empty<CfdDiagnostic>());
        }
        catch (CfdLoadException error) { return new LoadResult<T>(null, error.Diagnostics); }
    }
}

/// <summary>A fully built data set for one context.</summary>
public sealed class CfdSnapshot
{
    private readonly Dictionary<Type, object> _tables = new Dictionary<Type, object>();
    private readonly Dictionary<Type, object> _singletons = new Dictionary<Type, object>();
    internal CfdSnapshot(IReadOnlyList<CfdType> types, LoadContext context, BuildSession session)
    {
        foreach (var type in types)
        {
            if (type.Kind == CfdTypeKind.Table) _tables.Add(type.ValueType, type.BuildTable(context.RecordsFor(type), session));
            if (type.Kind == CfdTypeKind.Singleton) _singletons.Add(type.ValueType, session.Reader(context.RecordsFor(type)[0].Data).Value);
        }
    }
    public Table<T> Table<T>() where T : class => _tables.TryGetValue(typeof(T), out var table)
        ? (Table<T>)table : throw new InvalidOperationException($"`{typeof(T).Name}` is not a table in this schema.");
    public T Singleton<T>() where T : class => _singletons.TryGetValue(typeof(T), out var value)
        ? (T)value : throw new InvalidOperationException($"`{typeof(T).Name}` is not a singleton in this schema.");
}

/// <summary>Completed tables, singletons and dimension results.</summary>
public sealed class CfdData
{
    private readonly Dictionary<Type, object> _tables = new Dictionary<Type, object>();
    private readonly Dictionary<Type, object> _singletons = new Dictionary<Type, object>();
    private readonly Dimension<CfdSnapshot> _snapshots;
    internal CfdData(IReadOnlyList<CfdType> types, LoadContext context)
    {
        _snapshots = new Dimension<CfdSnapshot>(context.Catalog, context.Sessions.Select(session => new CfdSnapshot(types, context, session)).ToArray());
        foreach (var type in types)
        {
            if (type.Kind == CfdTypeKind.Table) _tables.Add(type.ValueType, type.BuildDimensions(context.RecordsFor(type), context));
            if (type.Kind == CfdTypeKind.Singleton) _singletons.Add(type.ValueType, type.BuildDimension(context.RecordsFor(type)[0], context));
        }
    }
    public CfdSnapshot Base => _snapshots.Base;
    public CfdSnapshot For(string dimension, string variant) => _snapshots.For(dimension, variant);
    public CfdSnapshot For(DimensionSelection selection) => _snapshots.For(selection);
    public CfdSnapshot For(params (string Dimension, string Variant)[] selection) => _snapshots.For(selection);
    public IReadOnlyList<string> Variants(string dimension) => _snapshots.Variants(dimension);
    public Table<Dimension<T>> Table<T>() where T : class => _tables.TryGetValue(typeof(T), out var table)
        ? (Table<Dimension<T>>)table : throw new InvalidOperationException($"`{typeof(T).Name}` is not a table in this schema.");
    public Dimension<T> Singleton<T>() where T : class => _singletons.TryGetValue(typeof(T), out var value)
        ? (Dimension<T>)value : throw new InvalidOperationException($"`{typeof(T).Name}` is not a singleton in this schema.");
}

internal sealed class RecordState
{
    internal RecordState(CfdType type, string id, string path, CfdSpan span)
    { Data = new ObjectData(type, path, span, id); }
    internal ObjectData Data { get; }
    internal string Id => Data.Id!;
    internal CfdType Type => Data.Type;
    internal string Path => Data.Path;
}

internal sealed class ObjectData
{
    internal ObjectData(CfdType type, string path, CfdSpan span, string? id = null)
    { Type = type; Path = path; Span = span; Id = id; Fields = new BoundValue[type.Fields.Length]; }
    internal CfdType Type { get; }
    internal string Path { get; }
    internal CfdSpan Span { get; }
    internal string? Id { get; }
    internal BoundValue[] Fields { get; }
    internal Dictionary<int, Dictionary<string, BoundValue>>? Overrides { get; set; }
    internal bool NeedsEvaluation { get; set; }
    internal bool IsFixed => Overrides == null && !NeedsEvaluation;
}
internal readonly struct SourceField
{
    internal SourceField(CfdValueNode node, string path) { Node = node; Path = path; }
    internal CfdValueNode Node { get; }
    internal string Path { get; }
}
internal sealed class LoadContext
{
    private readonly Dictionary<string, CfdType> _types;
    private readonly Dictionary<(string Root, string Id), RecordState> _recordsByIdentity = new Dictionary<(string Root, string Id), RecordState>();
    private readonly Dictionary<string, List<RecordState>> _recordsById = new Dictionary<string, List<RecordState>>(StringComparer.Ordinal);
    private readonly IReadOnlyDictionary<string, CfdValueNode> _constants;
    internal DimensionCatalog Catalog { get; private set; } = null!;
    internal List<BuildSession> Sessions { get; } = new List<BuildSession>();
    internal BuildSession Base => Sessions[0];
    internal CfdValueNode ResolveConstant(CfdValueNode node) => node is CfdScalarValue scalar && _constants.TryGetValue(scalar.Value, out var value) ? value : node;
    private sealed class TypePlan
    {
        internal TypePlan(CfdType[] lineage) { Lineage = lineage; Root = lineage[lineage.Length - 1].Name; Ancestors = new HashSet<CfdType>(lineage); }
        internal CfdType[] Lineage { get; }
        internal string Root { get; }
        internal HashSet<CfdType> Ancestors { get; }
    }
    private readonly Dictionary<CfdType, TypePlan> _typePlans = new Dictionary<CfdType, TypePlan>();
    private readonly Dictionary<CfdType, List<RecordState>> _recordsByType = new Dictionary<CfdType, List<RecordState>>();
    internal IReadOnlyList<RecordState> RecordsFor(CfdType type) => _recordsByType[type];
    internal List<RecordState> Records { get; } = new List<RecordState>();
    internal Dictionary<string, SortedSet<string>> Variants { get; } = new Dictionary<string, SortedSet<string>>(StringComparer.Ordinal);
    internal LoadContext(IReadOnlyList<CfdDocument> documents, IReadOnlyList<CfdType> types, IReadOnlyDictionary<string, CfdValueNode>? constants, LoadOptions? options = null)
    {
        _constants = constants ?? new Dictionary<string, CfdValueNode>(StringComparer.Ordinal);
        _types = types.ToDictionary(type => type.Name, StringComparer.Ordinal);
        // 继承关系与表成员在本次加载中只编译一次，父表直接共享派生记录身份。
        foreach (var type in types)
        {
            var lineage = new List<CfdType>();
            for (var ancestor = type; ancestor != null; ancestor = ancestor.Parent == null ? null : _types[ancestor.Parent]) lineage.Add(ancestor);
            _typePlans.Add(type, new TypePlan(lineage.ToArray()));
            if (type.Kind != CfdTypeKind.Data) _recordsByType.Add(type, new List<RecordState>());
        }
        foreach (var type in types)
            foreach (var field in type.Fields)
                if (field.Dimension is string dimension && !Variants.ContainsKey(dimension)) Variants.Add(dimension, new SortedSet<string>(StringComparer.Ordinal));
        // 全部记录先建立身份，基础文件与覆盖文件顺序不影响解析结果。
        foreach (var document in documents)
            foreach (var node in document.Records)
            {
                if (!_types.TryGetValue(node.DeclaredType, out var type) || type.Kind == CfdTypeKind.Data || type.IsAbstract)
                    throw Error("CFD-TYPE", $"invalid record type `{node.DeclaredType}`", document.Path, node.Span);
                // 分组声明限定实际类型，未知分组与无关类型不能绕过继承校验。
                if (node.GroupType != null && (!_types.TryGetValue(node.GroupType, out var group) || group.Kind != CfdTypeKind.Table || !Assignable(type, node.GroupType)))
                    throw Error("CFD-TYPE", $"record type `{type.Name}` is not assignable to group `{node.GroupType}`", document.Path, node.Span);
                var identity = (Root(type), node.Id);
                if (_recordsByIdentity.ContainsKey(identity))
                    throw Error("CFD-DUPLICATE", $"duplicate record `{node.Id}`", document.Path, node.Span);
                var record = new RecordState(type, node.Id, document.Path, node.Span);
                Records.Add(record);
                foreach (var ancestor in _typePlans[type].Lineage)
                    if (ancestor.Kind != CfdTypeKind.Data) _recordsByType[ancestor].Add(record);
                // 引用按继承树与 id 建立临时索引，加载过程不逐条扫描全部记录。
                _recordsByIdentity.Add(identity, record);
                if (!_recordsById.TryGetValue(node.Id, out var matching))
                    _recordsById.Add(node.Id, matching = new List<RecordState>());
                matching.Add(record);
            }
        foreach (var type in types.Where(type => type.Kind == CfdTypeKind.Singleton))
        {
            var records = _recordsByType[type];
            if (records.Count != 1)
                throw Error("CFD-SINGLETON", $"singleton `{type.Name}` requires exactly one record", "", default);
            // 单例身份来自声明短名，和 Rust 数据模型使用同一条 id 规则。
            var id = type.Name.Substring(type.Name.LastIndexOf('.') + 1);
            if (records[0].Id != id)
                throw Error("CFD-SINGLETON", $"singleton `{type.Name}` requires record id `{id}`", records[0].Path, records[0].Data.Span);
        }
        var overrides = new Dictionary<RecordState, Dictionary<int, Dictionary<string, SourceField>>>();
        foreach (var document in documents)
            foreach (var block in document.Dimensions)
            {
                if (!Variants.TryGetValue(block.Dimension, out var variants))
                    throw Error("CFD-DIMENSION", $"unknown dimension `{block.Dimension}`", document.Path, block.Span);
                variants.Add(block.Variant);
                foreach (var target in block.Records)
                {
                    var record = Find(target.DeclaredType, target.Id, document.Path, target.Span);
                    if (!overrides.TryGetValue(record, out var fields)) overrides.Add(record, fields = new Dictionary<int, Dictionary<string, SourceField>>());
                    foreach (var field in target.Fields)
                    {
                        var ordinal = record.Type.FindOrdinal(field.Name);
                        if (ordinal < 0 || record.Type.Fields[ordinal].Dimension != block.Dimension)
                            throw Error("CFD-DIMENSION", $"field `{field.Name}` is not bound to dimension `{block.Dimension}`", document.Path, field.Span);
                        if (!fields.TryGetValue(ordinal, out var values)) fields.Add(ordinal, values = new Dictionary<string, SourceField>(StringComparer.Ordinal));
                        if (values.ContainsKey(block.Variant)) throw Error("CFD-DUPLICATE-OVERLAY", $"duplicate override `{block.Dimension}[{block.Variant}]::{target.Id}.{field.Name}`", document.Path, field.Span);
                        values.Add(block.Variant, new SourceField(field.Value, document.Path));
                    }
                }
            }
        Catalog = new DimensionCatalog(Variants, options);
        foreach (var selection in Catalog.Selections) Sessions.Add(new BuildSession(selection));
        // 身份索引先完整建立，再绑定字段；普通引用不会递归构造业务对象。
        foreach (var document in documents)
            foreach (var node in document.Records)
            {
                var record = Find(node.DeclaredType, node.Id, document.Path, node.Span);
                overrides.TryGetValue(record, out var fields);
                BindFields(record.Data, node.Fields, fields);
            }

    }
    private string Root(CfdType type) => _typePlans[type].Root;
    internal bool Assignable(CfdType actual, string expected) => _types.TryGetValue(expected, out var type) && _typePlans[actual].Ancestors.Contains(type);
    internal RecordState Find(string? type, string id, string path, CfdSpan span)
    {
        if (type != null)
        {
            if (_types.TryGetValue(type, out var expected)
                && _recordsByIdentity.TryGetValue((Root(expected), id), out var record)
                && Assignable(record.Type, type)) return record;
        }
        else if (_recordsById.TryGetValue(id, out var matching) && matching.Count == 1)
            return matching[0];
        throw Error("CFD-REFERENCE", $"record `{type}::{id}` is missing or ambiguous", path, span);
    }
    private int _bindingDepth;
    private void BindFields(ObjectData data, IReadOnlyList<CfdFieldNode> fields, Dictionary<int, Dictionary<string, SourceField>>? overrides = null)
    {
        if (++_bindingDepth > 256) { _bindingDepth--; throw Error("CFD-LIMIT", "construction nesting exceeds 256 levels", data.Path, data.Span); }
        try
        {
            var nodes = new CfdValueNode?[data.Fields.Length];
            foreach (var field in fields)
            {
                var ordinal = data.Type.FindOrdinal(field.Name);
                if (ordinal < 0) throw Error("CFD-FIELD", $"unknown field `{field.Name}`", data.Path, field.Span);
                if (nodes[ordinal] != null) throw Error("CFD-DUPLICATE-FIELD", $"duplicate field `{field.Name}`", data.Path, field.Span);
                nodes[ordinal] = field.Value;
            }
            for (var i = 0; i < nodes.Length; i++)
            {
                var descriptor = data.Type.Fields[i];
                var node = nodes[i] ?? descriptor.DefaultValue ?? throw Error("CFD-MISSING", $"missing required field `{descriptor.Name}`", data.Path, data.Span);
                data.Fields[i] = descriptor.Type.Bind(this, node, new BindingScope(data.Type, data.Path));
            }
            data.NeedsEvaluation = data.Fields.Any(field => !field.IsFixed);
            if (overrides == null) return;
            data.Overrides = new Dictionary<int, Dictionary<string, BoundValue>>();
            foreach (var field in overrides)
            {
                var values = new Dictionary<string, BoundValue>(StringComparer.Ordinal);
                foreach (var variant in field.Value)
                {
                    var source = variant.Value;
                    values.Add(variant.Key, data.Type.Fields[field.Key].Type.Bind(this, source.Node, new BindingScope(data.Type, source.Path)));
                }
                data.Overrides.Add(field.Key, values);
                data.NeedsEvaluation |= values.Values.Any(value => !value.IsFixed);
            }
        }
        finally { _bindingDepth--; }
    }
    internal void Build()
    {
        // 每个请求上下文完整构建；失败时整个加载结果均不发布。
        foreach (var session in Sessions)
            foreach (var record in Records) _ = session.Reader(record.Data).Value;
    }
    internal BoundValue<T> BindReference<T>(string expected, CfdValueNode node, BindingScope owner) where T : class
    {
        if (!(node is CfdReferenceValue reference)) throw owner.Error(node, "CFD-TYPE", "expected record reference");
        var record = Find(reference.TypeName ?? expected, reference.Id, owner.Path, node.Span);
        if (!Assignable(record.Type, expected) || !typeof(T).IsAssignableFrom(record.Type.ValueType)) throw owner.Error(node, "CFD-TYPE", "reference type mismatch");
        return new BoundValue<T>(reader => (T)reader.Session.Reader(record.Data).Value, reader => reader.Session.Reader(record.Data));
    }
    internal BoundValue<T> BindObject<T>(string expected, CfdValueNode node, BindingScope owner)
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
        if (!_types.TryGetValue(actual ?? expected, out var type) || type.Kind != CfdTypeKind.Data || type.IsAbstract || !Assignable(type, expected)
            || !typeof(T).IsAssignableFrom(type.ValueType)) throw owner.Error(node, "CFD-TYPE", "inline object type mismatch");
        var data = new ObjectData(type, owner.Path, node.Span);
        BindFields(data, fields);
        if (data.IsFixed)
        {
            var fixedReader = Base.Reader(data);
            return new BoundValue<T>((T)fixedReader.Value, _ => fixedReader);
        }
        return new BoundValue<T>(reader => (T)reader.Session.Reader(data).Value, reader => reader.Session.Reader(data));
    }
    internal BoundValue<string> BindTemplate(CfdFormattedStringValue template, BindingScope owner)
    {
        var parts = new List<Func<CfdReader, string>>();
        foreach (var segment in template.Segments)
        {
            if (segment is CfdFormatText text) { parts.Add(_ => text.Text); continue; }
            var reference = (CfdFormatReference)segment;
            var record = reference.Id == null ? null : Find(reference.TypeName, reference.Id, owner.Path, template.Span);
            var type = record?.Type ?? owner.Type;
            var start = reference.Id == null ? 1 : 0;
            var ordinals = new List<int>();
            var id = false;
            CfdValueType? terminal = null;
            for (var i = start; i < reference.Path.Count; i++)
            {
                var name = reference.Path[i];
                if (i == reference.Path.Count - 1 && name == "id" && type.Kind != CfdTypeKind.Data) { id = true; break; }
                var ordinal = type.FindOrdinal(name);
                if (ordinal < 0) throw owner.Error(template, "CFD-FIELD", $"unknown field `{name}`");
                var field = type.Fields[ordinal];
                ordinals.Add(ordinal);
                terminal = field.Type;
                if (i == reference.Path.Count - 1) break;
                if (field.Type.IsOptional) throw owner.Error(template, "CFD-INTERPOLATION", "field path cannot traverse an optional value");
                if (field.Type.NamedType == null || !_types.TryGetValue(field.Type.NamedType, out type!))
                    throw owner.Error(template, "CFD-INTERPOLATION", "field path must traverse data or a record reference");
            }
            if (ordinals.Count == 0 && !id) throw owner.Error(template, "CFD-INTERPOLATION", "empty field path");
            var path = ordinals.ToArray();
            var terminalType = terminal;
            var pathText = owner.Path;
            var span = template.Span;
            parts.Add(reader =>
            {
                var current = record == null ? reader : reader.Session.Reader(record.Data);
                var count = id ? path.Length : path.Length - 1;
                for (var i = 0; i < count; i++)
                    current = current.Source(path[i]).Traverse(current)
                        ?? throw Error("CFD-INTERPOLATION", "field path must traverse data or a record reference", pathText, span);
                if (id) return current.Id;
                return terminalType!.FormatScalar(current.Read(path[path.Length - 1]))
                    ?? throw Error("CFD-INTERPOLATION", "interpolation requires a string, number, bool or enum field", pathText, span);
            });
        }
        return new BoundValue<string>(reader =>
        {
            var output = new StringBuilder();
            foreach (var part in parts) output.Append(part(reader));
            return output.ToString();
        });
    }
    internal static CfdLoadException Error(string code, string message, string path, CfdSpan span) => new CfdLoadException(new[] { new CfdDiagnostic(code, message, path, span) });
}

// 会话仅在加载期间存在。记录先登记身份再填充，循环引用不递归创建新身份。
internal sealed class BuildSession
{
    internal readonly DimensionSelection Selection;
    private readonly Dictionary<ObjectData, CfdReader> _readers = new Dictionary<ObjectData, CfdReader>();
    private readonly HashSet<(CfdReader, BoundValue)> _evaluating = new HashSet<(CfdReader, BoundValue)>();
    internal BuildSession(DimensionSelection selection) { Selection = selection; }
    internal CfdReader Reader(ObjectData data)
    {
        if (!_readers.TryGetValue(data, out var reader)) _readers.Add(data, reader = new CfdReader(this, data));
        return reader;
    }
    internal object? Evaluate(CfdReader reader, BoundValue value)
    {
        var key = (reader, value);
        if (!_evaluating.Add(key)) throw LoadContext.Error("CFD-INTERPOLATION-CYCLE", "cyclic value dependency", value.Path, value.Span);
        try
        {
            if (_evaluating.Count > 256) throw LoadContext.Error("CFD-LIMIT", "construction nesting exceeds 256 levels", value.Path, value.Span);
            return value.Evaluate(reader);
        }
        finally { _evaluating.Remove(key); }
    }
}

public sealed class CfdReader
{
    internal readonly BuildSession Session;
    internal readonly ObjectData Data;
    private readonly BoundValue[] _fields;
    private readonly Dictionary<int, BoundValue>? _selected;
    private Dictionary<BoundValue, object?>? _results;
    private object? _value;
    private bool _building;
    private bool _built;
    internal CfdReader(BuildSession session, ObjectData data)
    {
        Session = session; Data = data; _fields = data.Fields;
        // 构建读取器只定位实际命中的覆盖；其余字段使用已绑定的基础值。
        if (data.Overrides == null || session.Selection == DimensionSelection.Empty) return;
        foreach (var field in data.Overrides)
            if (session.Selection.TryGet(data.Type.Fields[field.Key].Dimension!, out var variant)
                && field.Value.TryGetValue(variant, out var value))
                (_selected ??= new Dictionary<int, BoundValue>()).Add(field.Key, value);
    }
    public string Id => Data.Id ?? throw new InvalidOperationException("Inline data has no record identity.");
    internal object Value
    {
        get
        {
            if (_built || _building) return _value!;
            _value = Data.Type.Create(Data.Id ?? "");
            _building = true;
            try
            {
                for (var i = 0; i < _fields.Length; i++) Read(i);
                _value = Data.Type.Fill(_value, this);
                _built = true;
                return _value;
            }
            finally { _building = false; }
        }
    }
    public T Get<T>(string field)
    {
        if (field == "id" && Data.Id != null) return (T)(object)Data.Id;
        var ordinal = Data.Type.FindOrdinal(field);
        if (ordinal < 0) throw LoadContext.Error("CFD-FIELD", $"unknown field `{field}`", Data.Path, Data.Span);
        return Get<T>(ordinal);
    }
    public T Get<T>(int ordinal)
    {
        if ((uint)ordinal >= (uint)_fields.Length) throw new ArgumentOutOfRangeException(nameof(ordinal));
        var source = _selected != null && _selected.TryGetValue(ordinal, out var selected) ? selected : _fields[ordinal];
        // 构建回调按字段序号读取已绑定的值，完成后不再保留读取器。
        var value = (BoundValue<T>)source;
        return value.IsFixed ? value.Fixed : value.Get(this);
    }
    internal BoundValue Source(int ordinal)
    {
        if ((uint)ordinal >= (uint)_fields.Length) throw new ArgumentOutOfRangeException(nameof(ordinal));
        return _selected != null && _selected.TryGetValue(ordinal, out var value) ? value : _fields[ordinal];
    }
    internal object? Read(int ordinal) => Source(ordinal).Read(this);
    internal object? ReadDynamic(BoundValue source)
    {
        if (_results != null && _results.TryGetValue(source, out var cached)) return cached;
        var value = Session.Evaluate(this, source);
        (_results ??= new Dictionary<BoundValue, object?>()).Add(source, value);
        return value;
    }
}
}
