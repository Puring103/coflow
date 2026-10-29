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
            // 业务对象保持基础值；维度视图通过独立上下文生成并保留同一视图内的引用身份。
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
        var members = new Dictionary<Type, List<RecordState>>();
        var routes = new Dictionary<Type, List<List<RecordState>>>();
        foreach (var type in types)
            if (type.Kind != CfdTypeKind.Data && !members.ContainsKey(type.ValueType))
                members.Add(type.ValueType, new List<RecordState>());
        foreach (var record in records)
        {
            var actual = record.Value.GetType();
            if (!routes.TryGetValue(actual, out var tables))
            {
                tables = new List<List<RecordState>>();
                // CLR 可赋值关系按实际类型计算一次，父表仍收录相同的派生对象身份。
                foreach (var member in members)
                    if (member.Key.IsAssignableFrom(actual)) tables.Add(member.Value);
                routes.Add(actual, tables);
            }
            foreach (var table in tables) table.Add(record);
        }
        // 注册类别决定可访问入口，data 不进入记录表；重复获取返回同一只读表。
        foreach (var type in types)
        {
            if (type.Kind == CfdTypeKind.Table) _tables.Add(type.ValueType, type.BuildTable(members[type.ValueType]));
            if (type.Kind == CfdTypeKind.Singleton) _singletons.Add(type.ValueType, type.BuildSingleton(members[type.ValueType]));
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
    internal bool Available => _built || _building;
    internal void Resume() { _building = false; }
    internal void Build()
    {
        if (_built || _building) return;
        _building = true;
        try { Reader.Validate(); Type.FillRecord(Value, Reader); _built = true; _building = false; }
        catch (ConstructionSuspension suspension) { suspension.Records.Add(this); throw; }
        catch { _building = false; throw; }
    }
}

internal sealed partial class LoadContext
{
    private readonly Dictionary<string, CfdType> _types;
    private readonly Dictionary<string, IReadOnlyList<string>> _variantNames = new Dictionary<string, IReadOnlyList<string>>(StringComparer.Ordinal);
    private readonly Dictionary<(DimensionSelection Selection, string Dimension, string Variant), DimensionSelection> _selectionSteps = new Dictionary<(DimensionSelection, string, string), DimensionSelection>();
    private readonly Dictionary<string, DimensionSelection> _selections = new Dictionary<string, DimensionSelection>(StringComparer.Ordinal);
    private readonly Dictionary<string, Dictionary<RecordState, RecordState>> _views = new Dictionary<string, Dictionary<RecordState, RecordState>>(StringComparer.Ordinal);
    internal RecordState InView(RecordState record, DimensionSelection selection)
    {
        if (selection.Key.Length == 0) return record;
        if (!_views.TryGetValue(selection.Key, out var records)) _views.Add(selection.Key, records = new Dictionary<RecordState, RecordState>());
        if (!records.TryGetValue(record, out var projected))
        {
            // 先登记身份，再填充字段；视图中的循环引用仍指向同一个对象。
            projected = new RecordState(record.Type, record.Node, record.Path);
            projected.Reader = record.Reader.InView(selection);
            records.Add(record, projected);
        }
        return projected;
    }
    internal object Select(RecordState record, DimensionSelection selection)
    {
        lock (_views)
        {
            return SelectLocked(record, selection);
        }
    }
    internal object Select(RecordState record, DimensionSelection selection, string dimension, string variant)
    {
        lock (_views)
        {
            // For 已在取得记录身份前验证维度；转换与构造共用当前锁，避免重复查找和锁重入。
            return SelectLocked(record, SelectDimensions(selection, dimension, variant));
        }
    }
    private object SelectLocked(RecordState record, DimensionSelection selection)
    {
        try
        {
            var projected = InView(record, selection);
            if (!projected.Available) BuildSelection(projected);
            return projected.Value;
        }
        catch
        {
            // 组合视图失败时移除构造缓存，避免循环引用中已填充的邻居暴露半成品。
            _views.Remove(selection.Key);
            throw;
        }
    }
    private DimensionSelection SelectDimensions(DimensionSelection selection, string dimension, string variant)
    {
        if (dimension == null || variant == null) throw new ArgumentNullException();
        var step = (selection, dimension, variant);
        if (_selectionSteps.TryGetValue(step, out var result)) return result;
        result = selection.With(dimension, variant);
        // 选择转换按加载上下文共享。任意组合顺序得到同一规范选择，命中时不克隆字典或键。
        if (_selections.TryGetValue(result.Key, out var existing)) result = existing;
        else _selections.Add(result.Key, result);
        _selectionSteps.Add(step, result);
        return result;
    }
    internal IReadOnlyList<string> DimensionVariants(string dimension)
        => _variantNames.TryGetValue(dimension, out var variants) ? variants
        : throw new ArgumentException($"Unknown dimension `{dimension}`.", nameof(dimension));
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
                var identity = (Root(type), node.Id);
                if (_recordsByIdentity.ContainsKey(identity))
                    throw Error("CFD-DUPLICATE", $"duplicate record `{node.Id}`", document.Path, node.Span);
                var record = new RecordState(type, node, document.Path);
                Records.Add(record);
                // 引用按继承树与 id 建立临时索引，加载过程不逐条扫描全部记录。
                _recordsByIdentity.Add(identity, record);
                if (!_recordsById.TryGetValue(node.Id, out var matching))
                    _recordsById.Add(node.Id, matching = new List<RecordState>());
                matching.Add(record);
            }
        foreach (var type in types.Where(type => type.Kind == CfdTypeKind.Singleton))
        {
            var records = Records.Where(record => Assignable(record.Type, type.Name)).ToArray();
            if (records.Length != 1)
                throw Error("CFD-SINGLETON", $"singleton `{type.Name}` requires exactly one record", "", default);
            // 单例身份来自声明短名，和 Rust 数据模型使用同一条 id 规则。
            var id = type.Name.Substring(type.Name.LastIndexOf('.') + 1);
            if (records[0].Node.Id != id)
                throw Error("CFD-SINGLETON", $"singleton `{type.Name}` requires record id `{id}`", records[0].Path, records[0].Node.Span);
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
                    var record = Find(target.DeclaredType, target.Id, document.Path, target.Span);
                    if (!overrides.TryGetValue(record, out var fields)) overrides.Add(record, fields = new Dictionary<string, Dictionary<string, CfdValueNode>>(StringComparer.Ordinal));
                    foreach (var field in target.Fields)
                    {
                        var descriptor = record.Type.Field(field.Name);
                        if (descriptor == null || descriptor.Type.Dimension != block.Dimension)
                            throw Error("CFD-DIMENSION", $"field `{field.Name}` is not bound to dimension `{block.Dimension}`", document.Path, field.Span);
                        if (!fields.TryGetValue(field.Name, out var values)) fields.Add(field.Name, values = new Dictionary<string, CfdValueNode>(StringComparer.Ordinal));
                        if (values.ContainsKey(block.Variant)) throw Error("CFD-DUPLICATE-OVERLAY", $"duplicate override `{block.Dimension}[{block.Variant}]::{target.Id}.{field.Name}`", document.Path, field.Span);
                        values.Add(block.Variant, field.Value);
                        Track(field.Value, document.Path);
                    }
                }
            }
        foreach (var dimension in Variants)
            _variantNames.Add(dimension.Key, System.Array.AsReadOnly(dimension.Value.ToArray()));
        foreach (var record in Records)
        {
            foreach (var field in record.Node.Fields) Track(field.Value, record.Path);
            overrides.TryGetValue(record, out var fields);
            record.Reader = new CfdReader(this, record.Type, record.Node.Fields, record.Path, record.Node.Span, record.Node.Id, fields, record: record);
        }
    }
    private void Track(CfdValueNode node, string path)
    {
        node.SourcePath = path;
        if (node is CfdSomeValue some) Track(some.Value, path);
        if (node is CfdObjectValue obj) foreach (var field in obj.Fields) Track(field.Value, path);
        if (node is CfdArrayValue array) foreach (var value in array.Items) Track(value, path);
        if (node is CfdDictionaryValue dictionary) foreach (var entry in dictionary.Entries) { Track(entry.Key, path); Track(entry.Value, path); }
    }
    internal string Source(CfdValueNode node, string fallback) => node.SourcePath ?? fallback;
    private string Root(CfdType type) => type.Parent == null ? type.Name : Root(_types[type.Parent]);
    internal bool Assignable(CfdType actual, string expected) => actual.Name == expected || actual.Parent != null && Assignable(_types[actual.Parent], expected);
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
        foreach (var record in Records) Select(record, DimensionSelection.Empty);
        // 每个已声明变体都在加载时验证；读取视图不会把无效覆盖隐藏到业务访问之后。
        foreach (var dimension in Variants)
            foreach (var variant in dimension.Value)
            {
                var selection = SelectDimensions(DimensionSelection.Empty, dimension.Key, variant);
                try
                {
                    foreach (var record in Records) Select(record, selection);
                }
                finally
                {
                    // 校验用对象尚未交付业务方，及时释放，避免常驻内存随记录数乘变体数增长。
                    // 首次 For 再按需建立可复用视图；业务已取得的视图身份不会被逐出。
                    _views.Remove(selection.Key);
                }
            }
    }
    internal T ReadReference<T>(string expected, CfdValueNode node, CfdReader owner) where T : class
    {
        if (!(node is CfdReferenceValue reference)) throw owner.Error(node, "CFD-TYPE", "expected record reference");
        var record = Find(reference.TypeName ?? expected, reference.Id, Source(node, owner.Path), node.Span);
        if (!Assignable(record.Type, expected) || !(RequireRecord(record, owner.Selection) is T value)) throw owner.Error(node, "CFD-TYPE", "reference type mismatch");
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
        return new CfdReader(this, type, fields, Source(node, owner.Path), node.Span, selection: owner.Selection);
    }
    internal static CfdLoadException Error(string code, string message, string path, CfdSpan span) => new CfdLoadException(new[] { new CfdDiagnostic(code, message, path, span) });
}

public sealed partial class CfdReader
{
    private readonly LoadContext _context;
    private readonly CfdType _type;
    private readonly CfdSpan _span;
    private readonly string? _id;
    private readonly RecordState? _record;
    private readonly CfdReader? _baseReader;
    internal DimensionSelection Selection { get; }
    internal CfdReader InView(DimensionSelection selection)
        => new CfdReader(this, selection);
    internal object For(string dimension, string variant)
    {
        _context.DimensionVariants(dimension);
        return _context.Select(_record ?? throw new InvalidOperationException("Only records have dimension views."), Selection, dimension, variant);
    }
    internal IReadOnlyList<string> Variants(string dimension) => _context.DimensionVariants(dimension);
    internal bool HasVariant(string field, string variant)
    {
        var descriptor = field == null ? null : _type.Field(field);
        descriptor = descriptor
            ?? throw new ArgumentException($"Unknown field `{field}`.", nameof(field));
        if (descriptor.Type.Dimension == null) throw new ArgumentException($"Field `{field}` is not bound to a dimension.", nameof(field));
        var slot = _type.FieldSlot(descriptor.Name);
        return _overrides.Length != 0 && _overrides[slot] is Dictionary<string, CfdValueNode> variants && variants.ContainsKey(variant);
    }
    private readonly CfdValueNode?[] _nodes;
    private readonly Dictionary<string, CfdValueNode>?[] _overrides;
    private readonly object?[] _values;
    // 0 未求值，1 正在求值，2 已求值，3 已求值且不依赖维度上下文。
    private readonly byte[] _states;
    private Dictionary<CfdValueNode, CfdReader>? _objects;
    internal CfdType Type => _type;
    internal CfdReader ObjectReader(string expected, CfdValueNode node)
    {
        if (_objects == null) _objects = new Dictionary<CfdValueNode, CfdReader>();
        if (!_objects.TryGetValue(node, out var reader)) _objects.Add(node, reader = _context.CreateObjectReader(expected, node, this));
        return reader;
    }
    internal void Validate() { for (var slot = 0; slot < _type.Fields.Count; slot++) Read(slot); }
    internal string Path { get; }
    internal CfdReader(LoadContext context, CfdType type, IReadOnlyList<CfdFieldNode> nodes, string path, CfdSpan span, string? id = null, IReadOnlyDictionary<string, Dictionary<string, CfdValueNode>>? overrides = null, DimensionSelection? selection = null, RecordState? record = null)
    {
        _context = context; _type = type; _span = span; _id = id; Path = path; Selection = selection ?? DimensionSelection.Empty; _record = record;
        _nodes = new CfdValueNode?[type.Fields.Count];
        _values = new object?[type.Fields.Count];
        _states = new byte[type.Fields.Count];
        HashSet<string>? unknown = null;
        CfdFieldNode? firstUnknown = null;
        // 用 schema 槽位直接登记来源；先完成重复字段检测，保留原来重复优先于未知字段的诊断顺序。
        foreach (var field in nodes)
        {
            var slot = type.FieldSlot(field.Name);
            if (slot < 0)
            {
                if (unknown == null) unknown = new HashSet<string>(StringComparer.Ordinal);
                if (!unknown.Add(field.Name)) throw Error(field.Value, "CFD-DUPLICATE-FIELD", $"duplicate field `{field.Name}`");
                if (firstUnknown == null) firstUnknown = field;
            }
            else
            {
                if (_nodes[slot] != null) throw Error(field.Value, "CFD-DUPLICATE-FIELD", $"duplicate field `{field.Name}`");
                _nodes[slot] = field.Value;
            }
        }
        if (firstUnknown != null) throw Error(firstUnknown.Value, "CFD-FIELD", $"unknown field `{firstUnknown.Name}`");
        _overrides = overrides == null || overrides.Count == 0
            ? System.Array.Empty<Dictionary<string, CfdValueNode>?>()
            : new Dictionary<string, CfdValueNode>?[type.Fields.Count];
        if (overrides != null)
            foreach (var field in overrides) _overrides[type.FieldSlot(field.Key)] = field.Value;
    }
    private CfdReader(CfdReader source, DimensionSelection selection)
    {
        _context = source._context; _type = source._type; _span = source._span; _id = source._id;
        _record = source._record; Path = source.Path; Selection = selection;
        _baseReader = source._baseReader ?? source;
        // 来源节点及覆盖在加载后不可变；视图只分配自己的求值状态和业务对象。
        _nodes = source._nodes; _overrides = source._overrides;
        _values = new object?[_type.Fields.Count]; _states = new byte[_type.Fields.Count];
    }
    public T Get<T>(string field) => (T)Read(field)!;
    private object? Read(string field)
    {
        if (field == "id" && _id != null) return _id;
        var slot = _type.FieldSlot(field);
        if (slot >= 0) return Read(slot);
        _context.EnterRead(Path, _span);
        try { throw LoadContext.Error("CFD-FIELD", $"unknown field `{field}`", Path, _span); }
        finally { _context.ExitRead(); }
    }
    internal object? Read(int slot)
    {
        if (_states[slot] >= 2) return _values[slot];
        var descriptor = _type.Fields[slot];
        if (_states[slot] == 1) throw LoadContext.Error("CFD-INTERPOLATION-CYCLE", $"cyclic field dependency `{descriptor.Name}`", Path, _span);
        _context.CheckConstructionStack(this, slot);
        _context.EnterRead(Path, _span);
        _states[slot] = 1;
        var activeSlot = _activeSlot;
        _activeSlot = slot;
        var suspended = false;
        try
        {
            var node = SourceNode(slot)
                ?? throw LoadContext.Error("CFD-MISSING", $"missing required field `{descriptor.Name}`", Path, _span);
            var reusable = _baseReader != null && _baseReader._states[slot] == 3
                && ReferenceEquals(node, _baseReader.SourceNode(slot));
            // 只共享没有模板、引用或内联对象的不可变结果；依赖当前视图的值仍完整求值。
            var value = reusable ? _baseReader!._values[slot] : descriptor.Type.Read(_context, node, this);
            _values[slot] = value;
            _states[slot] = (byte)(reusable || ContextIndependent(node) ? 3 : 2);
            return value;
        }
        catch (ConstructionSuspension suspension)
        {
            suspended = true;
            suspension.Fields.Add((this, slot));
            throw;
        }
        finally
        {
            if (!suspended && _states[slot] == 1) _states[slot] = 0;
            _activeSlot = activeSlot;
            _context.ExitRead();
        }
    }
    private bool ContextIndependent(CfdValueNode node)
    {
        // 常量描述可包含多层别名，解析层数取决于值类型包装；保守地在当前上下文求值。
        if (!ReferenceEquals(node, _context.ResolveConstant(node))) return false;
        // 字典语法也能表示内联对象，甚至位于数组/Option 内；省略字段的默认模板不可共享。
        if (node is CfdFormattedStringValue || node is CfdReferenceValue || node is CfdObjectValue || node is CfdDictionaryValue) return false;
        if (node is CfdSomeValue some) return ContextIndependent(some.Value);
        if (node is CfdArrayValue array)
        {
            foreach (var item in array.Items) if (!ContextIndependent(item)) return false;
        }
        return true;
    }
    private CfdValueNode? SourceNode(int slot)
    {
        var descriptor = _type.Fields[slot];
        // 缺少覆盖时重新求值基础表达式，不能复用已经按基础上下文渲染的字符串。
        if (descriptor.Type.Dimension is string dimension && Selection.TryGet(dimension, out var variant)
            && _overrides.Length != 0 && _overrides[slot] is Dictionary<string, CfdValueNode> values && values.TryGetValue(variant, out var value)) return value;
        return _nodes[slot] ?? descriptor.DefaultValue;
    }
    internal CfdLoadException Error(CfdValueNode node, string code, string message) => LoadContext.Error(code, message, _context.Source(node, Path), node.Span);
    internal string Format(CfdFormattedStringValue formatted)
    {
        var output = new StringBuilder();
        foreach (var segment in formatted.Segments)
        {
            if (segment is CfdFormatText text) { output.Append(text.Text); continue; }
            var reference = (CfdFormatReference)segment;
            var owner = reference.Id == null ? this : _context.InView(_context.Find(reference.TypeName, reference.Id, _context.Source(formatted, Path), formatted.Span), Selection).Reader;
            // 是否可插值由静态字段描述决定，不靠装箱值的形状放宽可选类型。
            var value = owner.ReadPath(reference.Path, reference.Id == null ? 1 : 0, formatted);
            output.Append(value ?? throw Error(formatted, "CFD-INTERPOLATION", "interpolation requires a string, number, bool or enum field"));
        }
        return output.ToString();
    }
    private string? ReadPath(IReadOnlyList<string> path, int offset, CfdValueNode template)
    {
        if (path.Count == offset) throw Error(template, "CFD-INTERPOLATION", "empty field path");
        var name = path[offset];
        var slot = _type.FieldSlot(name);
        var descriptor = slot < 0 ? null : _type.Fields[slot];
        if (path.Count == offset + 1)
        {
            var value = Read(name);
            if (name == "id" && _id != null) return _id;
            return descriptor?.Type.FormatScalar(value);
        }
        if (descriptor?.Type.IsOptional == true)
            throw Error(template, "CFD-INTERPOLATION", "field path cannot traverse an optional value");
        // 路径依赖沿构建材料解析，避免使用反射访问尚未填充的业务属性。
        var node = descriptor == null ? null : SourceNode(slot);
        if (node != null) node = _context.ResolveConstant(node);
        if (node is CfdReferenceValue reference) return _context.InView(_context.Find(reference.TypeName ?? descriptor?.Type.NamedType, reference.Id, _context.Source(node, Path), node.Span), Selection).Reader.ReadPath(path, offset + 1, template);
        if (node != null && descriptor?.Type.NamedType is string expected) return ObjectReader(expected, node).ReadPath(path, offset + 1, template);
        throw Error(template, "CFD-INTERPOLATION", "field path must traverse data or a record reference");
    }
}
}
