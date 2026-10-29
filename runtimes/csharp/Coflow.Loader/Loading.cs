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
    public static LoadResult<T> Load<T>(IEnumerable<CfdSource> sources, IReadOnlyList<CfdType> types, Func<CfdData, T> construct, IReadOnlyDictionary<string, CfdValueNode>? constants = null) where T : class
    {
        if (sources == null || types == null || construct == null) throw new ArgumentNullException();
        try
        {
            var context = new LoadContext(CfdParser.ParseAll(sources), types, constants);
            context.Build();
            // 业务对象保持基础值；维度视图通过独立上下文生成并保留同一视图内的引用身份。
            return new LoadResult<T>(construct(new CfdData(types, context)), System.Array.Empty<CfdDiagnostic>());
        }
        catch (CfdLoadException error) { return new LoadResult<T>(null, error.Diagnostics); }
    }
}

public sealed class CfdData
{
    private readonly Dictionary<Type, object> _tables = new Dictionary<Type, object>();
    private readonly Dictionary<Type, object> _singletons = new Dictionary<Type, object>();
    internal CfdData(IReadOnlyList<CfdType> types, LoadContext context)
    {
        // 注册类别决定可访问入口，data 不进入记录表；重复获取返回同一只读表。
        foreach (var type in types)
        {
            if (type.Kind == CfdTypeKind.Table) _tables.Add(type.ValueType, type.BuildTable(context.RecordsFor(type)));
            if (type.Kind == CfdTypeKind.Singleton) _singletons.Add(type.ValueType, type.BuildSingleton(context.RecordsFor(type)));
        }
    }
    public Table<T> Table<T>() where T : class => _tables.TryGetValue(typeof(T), out var table)
        ? (Table<T>)table : throw new InvalidOperationException($"`{typeof(T).Name}` is not a table in this schema.");
    public T Singleton<T>() where T : class => _singletons.TryGetValue(typeof(T), out var value)
        ? (T)value : throw new InvalidOperationException($"`{typeof(T).Name}` is not a singleton in this schema.");
}

internal sealed class RecordState
{
    internal RecordState(CfdType type, CfdRecordNode node, string path) { Type = type; Node = node; Path = path; Value = type.CreateRecord(node.Id); }
    internal CfdType Type { get; }
    internal CfdRecordNode Node { get; }
    internal string Path { get; }
    internal object Value { get; }
    internal CfdReader Reader { get; set; } = null!;
    private bool _building;
    private bool _built;
    internal bool IsBuilt => _built;
    internal void Build()
    {
        if (_built || _building) return;
        _building = true;
        try { Reader.Validate(); Type.FillRecord(Value, Reader); _built = true; }
        finally { _building = false; }
    }
}

internal sealed class LoadContext
{
    private readonly Dictionary<string, CfdType> _types;
    private readonly Dictionary<DimensionSelection, Dictionary<RecordState, RecordState>> _views = new Dictionary<DimensionSelection, Dictionary<RecordState, RecordState>>();
    private Dictionary<(DimensionSelection Selection, RecordState Record), RecordState>? _pendingViews;
    private readonly Dictionary<string, DimensionSelection> _selections = new Dictionary<string, DimensionSelection>(StringComparer.Ordinal);
    private readonly Dictionary<(DimensionSelection Selection, string Dimension, string Variant), DimensionSelection> _selectionTransitions = new Dictionary<(DimensionSelection, string, string), DimensionSelection>();
    private Dictionary<RecordState, CfdReader>? _validationReaders;
    private DimensionSelection? _validationSelection;
    private DimensionSelection SelectionWith(DimensionSelection selection, string dimension, string variant)
    {
        if (dimension == null || variant == null) throw new ArgumentNullException();
        if (selection.TryGet(dimension, out var current) && current == variant) return selection;
        var transition = (selection, dimension, variant);
        if (_selectionTransitions.TryGetValue(transition, out var cached)) return cached;
        var candidate = selection.With(dimension, variant);
        if (!_selections.TryGetValue(candidate.Key, out cached)) _selections.Add(candidate.Key, cached = candidate);
        _selectionTransitions.Add(transition, cached);
        return cached;
    }
    internal object For(RecordState record, DimensionSelection selection, string dimension, string variant)
    {
        lock (_views)
        {
            DimensionVariants(dimension);
            return Select(record, SelectionWith(selection, dimension, variant));
        }
    }
    internal RecordState InView(RecordState record, DimensionSelection selection)
    {
        if (selection.Key.Length == 0) return record;
        if (!_views.TryGetValue(selection, out var records)) _views.Add(selection, records = new Dictionary<RecordState, RecordState>());
        if (!records.TryGetValue(record, out var projected))
        {
            // 先登记身份，再填充字段；视图中的循环引用仍指向同一个对象。
            projected = new RecordState(record.Type, record.Node, record.Path);
            projected.Reader = record.Reader.InView(selection);
            records.Add(record, projected);
        }
        // 模板曾经访问但尚未完整构造的投影，也必须纳入失败回滚。
        if (!projected.IsBuilt) _pendingViews?.TryAdd((selection, record), projected);
        return projected;
    }
    internal CfdReader ReaderInView(RecordState record, DimensionSelection selection)
    {
        if (_validationReaders == null) return InView(record, selection).Reader;
        if (!_validationReaders.TryGetValue(record, out var reader))
            _validationReaders.Add(record, reader = record.Reader.InView(_validationSelection!, validationOnly: true));
        return reader;
    }
    internal object Select(RecordState record, DimensionSelection selection)
    {
        lock (_views)
        {
            if (selection.Key.Length == 0) { record.Build(); return record.Value; }
            if (_views.TryGetValue(selection, out var records) && records.TryGetValue(record, out var existing) && existing.IsBuilt) return existing.Value;
            var ownsTransaction = _pendingViews == null;
            if (ownsTransaction) _pendingViews = new Dictionary<(DimensionSelection, RecordState), RecordState>();
            try
            {
                var projected = InView(record, selection);
                projected.Build();
                return projected.Value;
            }
            catch
            {
                // 只回滚本次未交付的对象图；其他记录已经交付的同组合视图保留身份。
                if (ownsTransaction)
                    foreach (var pending in _pendingViews!)
                        if (_views.TryGetValue(pending.Key.Selection, out var changed))
                        {
                            changed.Remove(pending.Key.Record);
                            if (changed.Count == 0) _views.Remove(pending.Key.Selection);
                        }
                throw;
            }
            finally { if (ownsTransaction) _pendingViews = null; }
        }
    }
    internal IReadOnlyList<string> DimensionVariants(string dimension)
        => _variantNames.TryGetValue(dimension, out var variants) ? variants
        : throw new ArgumentException($"Unknown dimension `{dimension}`.", nameof(dimension));
    private readonly Dictionary<string, IReadOnlyList<string>> _variantNames = new Dictionary<string, IReadOnlyList<string>>(StringComparer.Ordinal);
    private readonly Dictionary<(string Root, string Id), RecordState> _recordsByIdentity = new Dictionary<(string Root, string Id), RecordState>();
    private readonly Dictionary<string, List<RecordState>> _recordsById = new Dictionary<string, List<RecordState>>(StringComparer.Ordinal);
    private readonly IReadOnlyDictionary<string, CfdValueNode> _constants;
    internal CfdValueNode ResolveConstant(CfdValueNode node) => node is CfdScalarValue scalar && _constants.TryGetValue(scalar.Value, out var value) ? value : node;
    private int _depth;
    internal void EnterRead(string path, CfdSpan span)
    {
        if (++_depth > 256) { _depth--; throw Error("CFD-LIMIT", "construction nesting exceeds 256 levels", path, span); }
    }
    internal void ExitRead() { _depth--; }
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
    internal LoadContext(IReadOnlyList<CfdDocument> documents, IReadOnlyList<CfdType> types, IReadOnlyDictionary<string, CfdValueNode>? constants)
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
                var identity = (Root(type), node.Id);
                if (_recordsByIdentity.ContainsKey(identity))
                    throw Error("CFD-DUPLICATE", $"duplicate record `{node.Id}`", document.Path, node.Span);
                var record = new RecordState(type, node, document.Path);
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
            if (records[0].Node.Id != id)
                throw Error("CFD-SINGLETON", $"singleton `{type.Name}` requires record id `{id}`", records[0].Path, records[0].Node.Span);
        }
        var overrides = new Dictionary<RecordState, Dictionary<int, Dictionary<string, CfdFieldValue>>>();
        foreach (var document in documents)
            foreach (var block in document.Dimensions)
            {
                if (!Variants.TryGetValue(block.Dimension, out var variants))
                    throw Error("CFD-DIMENSION", $"unknown dimension `{block.Dimension}`", document.Path, block.Span);
                variants.Add(block.Variant);
                foreach (var target in block.Records)
                {
                    var record = Find(target.DeclaredType, target.Id, document.Path, target.Span);
                    if (!overrides.TryGetValue(record, out var fields)) overrides.Add(record, fields = new Dictionary<int, Dictionary<string, CfdFieldValue>>());
                    foreach (var field in target.Fields)
                    {
                        var ordinal = record.Type.FindOrdinal(field.Name);
                        if (ordinal < 0 || record.Type.Fields[ordinal].Type.Dimension != block.Dimension)
                            throw Error("CFD-DIMENSION", $"field `{field.Name}` is not bound to dimension `{block.Dimension}`", document.Path, field.Span);
                        if (!fields.TryGetValue(ordinal, out var values)) fields.Add(ordinal, values = new Dictionary<string, CfdFieldValue>(StringComparer.Ordinal));
                        if (values.ContainsKey(block.Variant)) throw Error("CFD-DUPLICATE-OVERLAY", $"duplicate override `{block.Dimension}[{block.Variant}]::{target.Id}.{field.Name}`", document.Path, field.Span);
                        values.Add(block.Variant, new CfdFieldValue(field.Value, document.Path));
                    }
                }
            }
        foreach (var record in Records)
        {
            overrides.TryGetValue(record, out var fields);
            record.Reader = new CfdReader(this, record.Type, record.Node.Fields, record.Path, record.Node.Span, record.Node.Id, fields, record: record);
        }
        // 已发现变体在加载结束后固定，所有记录复用一次排序后的只读列表。
        foreach (var dimension in Variants)
            _variantNames.Add(dimension.Key, System.Array.AsReadOnly(dimension.Value.ToArray()));
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
    internal void Build()
    {
        foreach (var record in Records) record.Build();
        // 每个已声明变体都在加载时验证；读取视图不会把无效覆盖隐藏到业务访问之后。
        foreach (var dimension in Variants)
            foreach (var variant in dimension.Value)
            {
                var selection = SelectionWith(DimensionSelection.Empty, dimension.Key, variant);
                try
                {
                    // 验证保持完整字段与依赖遍历，只省去业务对象、集合和 Option 的物化。
                    _validationSelection = selection;
                    _validationReaders = new Dictionary<RecordState, CfdReader>(Records.Count);
                    foreach (var record in Records) ReaderInView(record, selection).ValidateRecord();
                }
                finally
                {
                    _validationReaders = null;
                    _validationSelection = null;
                }
            }
    }
    private RecordState Reference<T>(string expected, CfdValueNode node, CfdReader owner) where T : class
    {
        if (!(node is CfdReferenceValue reference)) throw owner.Error(node, "CFD-TYPE", "expected record reference");
        var record = Find(reference.TypeName ?? expected, reference.Id, owner.ValuePath, node.Span);
        if (!Assignable(record.Type, expected) || !(record.Value is T)) throw owner.Error(node, "CFD-TYPE", "reference type mismatch");
        return record;
    }
    internal T ReadReference<T>(string expected, CfdValueNode node, CfdReader owner) where T : class
    {
        var record = Reference<T>(expected, node, owner);
        if (!owner.ValidationOnly) return (T)Select(record, owner.Selection);
        ReaderInView(record, owner.Selection).ValidateRecord();
        return null!;
    }
    internal T ReadObject<T>(string expected, CfdValueNode node, CfdReader owner)
    {
        var reader = owner.ObjectReader(expected, node);
        reader.Validate();
        if (owner.ValidationOnly)
        {
            if (!typeof(T).IsAssignableFrom(reader.Type.ValueType)) throw owner.Error(node, "CFD-TYPE", "generated object type mismatch");
            return default!;
        }
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
        return new CfdReader(this, type, fields, owner.ValuePath, node.Span, selection: owner.Selection, validationOnly: owner.ValidationOnly);
    }
    internal static CfdLoadException Error(string code, string message, string path, CfdSpan span) => new CfdLoadException(new[] { new CfdDiagnostic(code, message, path, span) });
}

// 来源只绑定在字段边界；嵌套值沿读取上下文继承，避免为每个 AST 节点建全局字典。
internal readonly struct CfdFieldValue
{
    internal CfdFieldValue(CfdValueNode node, string path) { Node = node; Path = path; }
    internal CfdValueNode Node { get; }
    internal string Path { get; }
}

public sealed class CfdReader
{
    private readonly LoadContext _context;
    private readonly CfdType _type;
    private readonly CfdSpan _span;
    private readonly string? _id;
    private readonly RecordState? _record;
    internal DimensionSelection Selection { get; }
    internal bool ValidationOnly { get; }
    internal CfdReader InView(DimensionSelection selection, bool validationOnly = false)
        => new CfdReader(this, selection, validationOnly);
    internal object For(string dimension, string variant)
        => _context.For(_record ?? throw new InvalidOperationException("Only records have dimension views."), Selection, dimension, variant);
    internal IReadOnlyList<string> Variants(string dimension) => _context.DimensionVariants(dimension);
    internal bool HasVariant(string field, string variant)
    {
        var ordinal = _type.FindOrdinal(field);
        if (ordinal < 0) throw new ArgumentException($"Unknown field `{field}`.", nameof(field));
        if (_type.Fields[ordinal].Type.Dimension == null) throw new ArgumentException($"Field `{field}` is not bound to a dimension.", nameof(field));
        return _overrides != null && _overrides.TryGetValue(ordinal, out var variants) && variants.ContainsKey(variant);
    }
    private readonly CfdValueNode?[] _nodes;
    private readonly IReadOnlyDictionary<int, Dictionary<string, CfdFieldValue>>? _overrides;
    private readonly object?[] _values;
    // 0 = 尚未求值，1 = 当前依赖栈中，2 = 已完成；不再为字段名分配哈希缓存。
    private readonly byte[] _states;
    private Dictionary<(CfdValueNode Node, string Path), CfdReader>? _objects;
    private bool _validating;
    private bool _validated;
    private string _valuePath;
    internal string ValuePath => _valuePath;
    internal CfdType Type => _type;
    internal CfdReader ObjectReader(string expected, CfdValueNode node)
    {
        var readers = _objects ??= new Dictionary<(CfdValueNode, string), CfdReader>();
        var key = (node, ValuePath);
        if (!readers.TryGetValue(key, out var reader)) readers.Add(key, reader = _context.CreateObjectReader(expected, node, this));
        return reader;
    }
    internal void Validate() { for (var ordinal = 0; ordinal < _values.Length; ordinal++) Read(ordinal); }
    internal void ValidateRecord()
    {
        // 记录引用可成环；字段模板依赖环仍由各 slot 的求值状态精确报告。
        if (_validated || _validating) return;
        _validating = true;
        try { Validate(); _validated = true; }
        finally { _validating = false; }
    }
    internal string Path { get; }
    private CfdReader(CfdReader source, DimensionSelection selection, bool validationOnly)
    {
        // 源节点和覆盖索引已在基础读取器中校验；视图只分配独立求值缓存。
        _context = source._context; _type = source._type; _span = source._span; _id = source._id;
        Path = source.Path; _valuePath = Path; _record = source._record; Selection = selection; ValidationOnly = validationOnly;
        _nodes = source._nodes; _overrides = source._overrides;
        _values = new object?[_nodes.Length]; _states = new byte[_nodes.Length];
    }
    internal CfdReader(LoadContext context, CfdType type, IReadOnlyList<CfdFieldNode> nodes, string path, CfdSpan span, string? id = null, IReadOnlyDictionary<int, Dictionary<string, CfdFieldValue>>? overrides = null, DimensionSelection? selection = null, RecordState? record = null, bool validationOnly = false)
    {
        _context = context; _type = type; _span = span; _id = id; Path = path; _valuePath = path;
        Selection = selection ?? DimensionSelection.Empty; _record = record; ValidationOnly = validationOnly;
        _nodes = new CfdValueNode?[type.Fields.Length]; _values = new object?[_nodes.Length]; _states = new byte[_nodes.Length];
        _overrides = overrides;
        CfdFieldNode? unknown = null;
        HashSet<string>? unknownNames = null;
        foreach (var field in nodes)
        {
            var ordinal = type.FindOrdinal(field.Name);
            if (ordinal < 0)
            {
                unknown ??= field;
                unknownNames ??= new HashSet<string>(StringComparer.Ordinal);
                if (!unknownNames.Add(field.Name)) throw Error(field.Value, "CFD-DUPLICATE-FIELD", $"duplicate field `{field.Name}`");
            }
            else
            {
                if (_nodes[ordinal] != null) throw Error(field.Value, "CFD-DUPLICATE-FIELD", $"duplicate field `{field.Name}`");
                _nodes[ordinal] = field.Value;
            }
        }
        if (unknown != null) throw Error(unknown.Value, "CFD-FIELD", $"unknown field `{unknown.Name}`");
    }
    public T Get<T>(string field) => (T)Read(field)!;
    /// <summary>Reads a field by its zero-based position in the generated type descriptor.</summary>
    public T Get<T>(int ordinal) => (T)Read(ordinal)!;
    private object? Read(string field)
    {
        if (field == "id" && _id != null) return _id;
        var ordinal = _type.FindOrdinal(field);
        if (ordinal < 0) throw LoadContext.Error("CFD-FIELD", $"unknown field `{field}`", Path, _span);
        return Read(ordinal);
    }
    private object? Read(int ordinal)
    {
        if ((uint)ordinal >= (uint)_values.Length) throw new ArgumentOutOfRangeException(nameof(ordinal));
        if (_states[ordinal] == 2) return _values[ordinal];
        var descriptor = _type.Fields[ordinal];
        if (_states[ordinal] == 1) throw LoadContext.Error("CFD-INTERPOLATION-CYCLE", $"cyclic field dependency `{descriptor.Name}`", Path, _span);
        _context.EnterRead(Path, _span);
        _states[ordinal] = 1;
        var previousPath = _valuePath;
        try
        {
            var source = SourceNode(ordinal);
            var node = source.Node ?? throw LoadContext.Error("CFD-MISSING", $"missing required field `{descriptor.Name}`", Path, _span);
            _valuePath = source.Path;
            var value = descriptor.Type.Read(_context, node, this);
            _values[ordinal] = value;
            _states[ordinal] = 2;
            return value;
        }
        finally
        {
            if (_states[ordinal] == 1) _states[ordinal] = 0;
            _valuePath = previousPath;
            _context.ExitRead();
        }
    }
    private (CfdValueNode? Node, string Path) SourceNode(int ordinal)
    {
        var descriptor = _type.Fields[ordinal];
        // 缺少覆盖时重新求值基础表达式，不能复用已经按基础上下文渲染的字符串。
        if (descriptor.Type.Dimension is string dimension && Selection.TryGet(dimension, out var variant)
            && _overrides != null && _overrides.TryGetValue(ordinal, out var values) && values.TryGetValue(variant, out var value)) return (value.Node, value.Path);
        return (_nodes[ordinal] ?? descriptor.DefaultValue, Path);
    }
    internal CfdLoadException Error(CfdValueNode node, string code, string message) => LoadContext.Error(code, message, ValuePath, node.Span);
    internal string Format(CfdFormattedStringValue formatted)
    {
        var output = new StringBuilder();
        foreach (var segment in formatted.Segments)
        {
            if (segment is CfdFormatText text) { output.Append(text.Text); continue; }
            var reference = (CfdFormatReference)segment;
            var owner = reference.Id == null ? this : _context.ReaderInView(_context.Find(reference.TypeName, reference.Id, ValuePath, formatted.Span), Selection);
            // 路径使用同一只读数组与偏移量，跨记录或内联数据时不再复制尾部数组。
            var value = owner.ReadPath(reference.Path, reference.Id == null ? 1 : 0, formatted, ValuePath);
            output.Append(value ?? throw Error(formatted, "CFD-INTERPOLATION", "interpolation requires a string, number, bool or enum field"));
        }
        return output.ToString();
    }
    private string? ReadPath(IReadOnlyList<string> path, int offset, CfdValueNode template, string templatePath)
    {
        if (offset == path.Count) throw LoadContext.Error("CFD-INTERPOLATION", "empty field path", templatePath, template.Span);
        var name = path[offset];
        var ordinal = _type.FindOrdinal(name);
        var descriptor = ordinal < 0 ? null : _type.Fields[ordinal];
        if (offset + 1 == path.Count)
        {
            var value = Read(name);
            if (name == "id" && _id != null) return _id;
            return descriptor?.Type.FormatScalar(value);
        }
        if (descriptor?.Type.IsOptional == true)
            throw LoadContext.Error("CFD-INTERPOLATION", "field path cannot traverse an optional value", templatePath, template.Span);
        // 路径依赖沿构建材料解析，避免使用反射访问尚未填充的业务属性。
        var source = ordinal < 0 ? (Node: (CfdValueNode?)null, Path) : SourceNode(ordinal);
        var node = source.Node == null ? null : _context.ResolveConstant(source.Node);
        if (node is CfdReferenceValue reference)
            return _context.ReaderInView(_context.Find(reference.TypeName ?? descriptor?.Type.NamedType, reference.Id, source.Path, node.Span), Selection).ReadPath(path, offset + 1, template, templatePath);
        if (node != null && descriptor?.Type.NamedType is string expected)
        {
            var previousPath = _valuePath;
            _valuePath = source.Path;
            try { return ObjectReader(expected, node).ReadPath(path, offset + 1, template, templatePath); }
            finally { _valuePath = previousPath; }
        }
        throw LoadContext.Error("CFD-INTERPOLATION", "field path must traverse data or a record reference", templatePath, template.Span);
    }
}
}
