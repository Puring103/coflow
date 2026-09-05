namespace CoflowRuntime.Generated;

using System.ComponentModel;
using System.Globalization;

/// <summary>
/// Per-load identity and reference state. Generated bindings register one
/// explicit reader for each schema type; no constructor discovery or reflection
/// is needed at runtime.
/// </summary>
[EditorBrowsable(EditorBrowsableState.Never)]
public sealed class CfdLoadContext
{
    private readonly Dictionary<(string DeclaredType, string Key), object> _cache = new();
    private readonly HashSet<(string DeclaredType, string Key)> _resolving = new();
    private readonly Stack<(string DeclaredType, string Key)> _currentRecords = new();
    private readonly IReadOnlyDictionary<CfdRecordNode, CfdNameResolver> _recordNames;
    private readonly IReadOnlyDictionary<CfdRecordNode, string> _recordPaths;
    private readonly Stack<CfdNameResolver> _currentNames = new();
    private readonly HashSet<CfdFormattedStringValue> _formatting = new();
    private readonly System.Runtime.CompilerServices.ConditionalWeakTable<
        CfdRecordNode, Dictionary<CfdValueNode, (string FieldName, string ValuePath)>> _valueLocations = new();
    private readonly List<CoflowFunctionEntry> _functions = new();
    private readonly bool _functionsCompiled;
    private readonly Dictionary<CoflowConstant, object> _constantCache = new();
    private readonly HashSet<CoflowConstant> _resolvingConstants = new();

    internal CfdLoadContext(
        IReadOnlyList<CfdDocument> documents,
        IEnumerable<ICfdTypeBinding>? bindings = null,
        IEnumerable<ICoflowEnumMetadata>? enums = null,
        IEnumerable<CoflowConstant>? constants = null,
        bool functionsCompiled = false)
    {
        if (documents is null) throw new ArgumentNullException(nameof(documents));
        _functionsCompiled = functionsCompiled;
        var bindingMap = new Dictionary<string, ICfdTypeBinding>(StringComparer.Ordinal);
        foreach (var binding in bindings ?? Array.Empty<ICfdTypeBinding>())
        {
            if (!bindingMap.TryAdd(binding.DeclaredType, binding))
            {
                throw new CfdLoadException(new[]
                {
                    new CfdDiagnostic(
                        "CFD-BINDING-DUPLICATE",
                        $"multiple bindings are registered for `{binding.DeclaredType}`",
                        string.Empty),
                });
            }
        }
        Bindings = bindingMap;
        var constantMap = (constants ?? Array.Empty<CoflowConstant>())
            .ToDictionary(item => item.DeclaredName, StringComparer.Ordinal);
        var bound = CfdNameBinder.Bind(
            documents,
            bindingMap.Keys.Concat((enums ?? Array.Empty<ICoflowEnumMetadata>())
                .Select(item => item.DeclaredType))
                .Concat(constantMap.Keys),
            constantMap);
        Documents = bound.Documents;
        _recordNames = bound.RecordNames;
        _recordPaths = bound.RecordPaths;
        Records = new CfdRecordCatalog(Documents, bindingMap);
    }

    internal IReadOnlyList<CfdDocument> Documents { get; }
    public IReadOnlyDictionary<string, ICfdTypeBinding> Bindings { get; }
    public List<CfdDiagnostic> Diagnostics { get; } = new();
    internal IReadOnlyList<CoflowFunctionEntry> Functions => _functions;
    internal CfdRecordCatalog Records { get; }

    internal void RegisterRecord(string declaredType, string key, object record)
    {
        if (!_cache.TryAdd((declaredType, key), record))
            throw Error("CFD-SYNTAX-DUPLICATE-RECORD", $"record `{declaredType}::{key}` is registered more than once");
    }

    public CoflowHostState Host() => new(_functionsCompiled);

    internal object ResolveConstant(CoflowConstant constant)
    {
        if (_constantCache.TryGetValue(constant, out var cached)) return cached;
        if (!_resolvingConstants.Add(constant))
            throw Error("COFLOW-CONSTANT-CYCLE", $"constant `{constant.DeclaredName}` has a runtime dependency cycle");
        try
        {
            var value = constant.Resolve(this);
            _constantCache.Add(constant, value);
            return value;
        }
        finally
        {
            _resolvingConstants.Remove(constant);
        }
    }

    public CoflowFunctionEntry Function(
        CfdValueNode? node,
        string fieldName,
        Type resultType,
        params Type[] parameterTypes)
    {
        if (node is not null && node is not CfdFunctionValue)
            throw new CfdLoadException(new[] { new CfdDiagnostic(
                "CFD-VALUE-FUNCTION",
                "expected a CFD function body",
                CurrentPath,
                node.Span) });
        return CreateFunctionEntry(node as CfdFunctionValue, fieldName, fieldName,
            resultType, parameterTypes, requiresCfdBody: false);
    }

    public CoflowFunctionEntry RequiredFunction(
        CfdValueNode? node,
        string fieldName,
        Type resultType,
        params Type[] parameterTypes)
    {
        if (node is not null && node is not CfdFunctionValue)
            throw new CfdLoadException(new[] { new CfdDiagnostic(
                "CFD-VALUE-FUNCTION",
                "expected a CFD function body",
                CurrentPath,
                node.Span) });
        return CreateFunctionEntry(node as CfdFunctionValue, fieldName, fieldName,
            resultType, parameterTypes, requiresCfdBody: true);
    }

    public CfdValueNode DefaultValue(string source)
    {
        if (source is null) throw new ArgumentNullException(nameof(source));
        return CfdParser.ParseValue(new CfdSource(CurrentPath, source));
    }

    public CoflowFunctionEntry DefaultFunction(
        string source,
        string fieldName,
        Type resultType,
        params Type[] parameterTypes)
    {
        var node = DefaultValue(source);
        if (node is not CfdFunctionValue function)
            throw new InvalidOperationException("A generated function default must contain a function value.");
        return CreateFunctionEntry(function, fieldName, fieldName,
            resultType, parameterTypes, requiresCfdBody: true);
    }

    public TDelegate FunctionValue<TDelegate>(
        CfdValueNode node,
        Type resultType,
        Type[] parameterTypes,
        Func<CoflowFunctionEntry, TDelegate> factory)
        where TDelegate : Delegate
    {
        if (node is not CfdFunctionValue source)
            throw new CfdLoadException(new[] { new CfdDiagnostic(
                "CFD-VALUE-FUNCTION",
                "expected a CFD function body",
                CurrentPath,
                node.Span) });
        if (resultType is null) throw new ArgumentNullException(nameof(resultType));
        if (parameterTypes is null) throw new ArgumentNullException(nameof(parameterTypes));
        if (factory is null) throw new ArgumentNullException(nameof(factory));
        var location = LocateFunctionValue(node) ?? throw new InvalidOperationException(
            "A persistent function value must belong to the current CFD record.");
        var entry = CreateFunctionEntry(
            source,
            location.FieldName,
            location.ValuePath,
            resultType,
            parameterTypes,
            requiresCfdBody: true);
        return factory(entry);
    }

    [System.ComponentModel.EditorBrowsable(System.ComponentModel.EditorBrowsableState.Never)]
    public TDelegate FunctionValueAot<TDelegate>(
        CfdValueNode node,
        Type resultType,
        Type[] parameterTypes,
        Func<CoflowCallable, TDelegate> factory)
        where TDelegate : Delegate
    {
        if (factory is null) throw new ArgumentNullException(nameof(factory));
        CoflowDelegateAdapter.Register(factory);
        return FunctionValue(node, resultType, parameterTypes,
            entry => factory(new CoflowCallable(entry)));
    }

    private CoflowFunctionEntry CreateFunctionEntry(
        CfdFunctionValue? source,
        string fieldName,
        string valuePath,
        Type resultType,
        IReadOnlyList<Type> parameterTypes,
        bool requiresCfdBody)
    {
        var declaredType = CurrentRecordType ?? throw new InvalidOperationException(
            "A persistent Coflow function entry must be created while reading a record.");
        var recordKey = CurrentRecordKey ?? string.Empty;
        var entry = new CoflowFunctionEntry(
            new CoflowFunctionIdentity(declaredType, recordKey, fieldName, valuePath),
            new CoflowFunctionSignature(resultType, parameterTypes),
            source,
            _currentNames.Peek(),
            CurrentPath,
            source?.Span ?? CurrentSpan,
            requiresCfdBody);
        _functions.Add(entry);
        return entry;
    }

    private (string FieldName, string ValuePath)? LocateFunctionValue(CfdValueNode target)
    {
        var record = CurrentRecordType is { } type && CurrentRecordKey is { } key
            ? FindRecord(type, key)
            : null;
        if (record is null) return null;
        var locations = _valueLocations.GetValue(record, static value => IndexValueLocations(value));
        return locations.TryGetValue(target, out var location) ? location : null;
    }

    private static Dictionary<CfdValueNode, (string FieldName, string ValuePath)> IndexValueLocations(
        CfdRecordNode record)
    {
        var result = new Dictionary<CfdValueNode, (string FieldName, string ValuePath)>();
        foreach (var field in record.Fields) AddValueLocation(result, field.Value, field.Name, field.Name);
        return result;
    }

    private static void AddValueLocation(
        Dictionary<CfdValueNode, (string FieldName, string ValuePath)> locations,
        CfdValueNode current,
        string fieldName,
        string path)
    {
        locations.Add(current, (fieldName, path));
        switch (current)
        {
            case CfdSomeValue some:
                AddValueLocation(locations, some.Value, fieldName, $"{path}.Some");
                break;
            case CfdOkValue ok:
                AddValueLocation(locations, ok.Value, fieldName, $"{path}.Ok");
                break;
            case CfdErrValue error:
                AddValueLocation(locations, error.Value, fieldName, $"{path}.Err");
                break;
            case CfdArrayValue array:
                for (var index = 0; index < array.Items.Count; index++)
                    AddValueLocation(locations, array.Items[index], fieldName, $"{path}[{index}]");
                break;
            case CfdDictionaryValue dictionary:
                for (var index = 0; index < dictionary.Entries.Count; index++)
                    AddValueLocation(locations, dictionary.Entries[index].Value, fieldName, $"{path}[{index}]");
                break;
            case CfdObjectValue objectValue:
                foreach (var field in objectValue.Fields)
                    AddValueLocation(locations, field.Value, fieldName, $"{path}.{field.Name}");
                break;
        }
    }

    public IDisposable EnterRecord(string declaredType, string key)
    {
        _currentRecords.Push((declaredType, key));
        var record = FindRecord(declaredType, key);
        if (record is null && key.Length != 0)
            throw new InvalidOperationException($"Cannot enter unknown CFD record `{declaredType}::{key}`.");
        _currentNames.Push(record is null ? CfdNameResolver.Root : _recordNames[record]);
        return new RecordScope(_currentRecords, _currentNames);
    }

    internal string? CurrentRecordType => _currentRecords.Count == 0 ? null : _currentRecords.Peek().DeclaredType;
    internal string? CurrentRecordKey => _currentRecords.Count == 0 ? null : _currentRecords.Peek().Key;

    private sealed class RecordScope : IDisposable
    {
        private readonly Stack<(string DeclaredType, string Key)> _records;
        private readonly Stack<CfdNameResolver> _names;
        private bool _disposed;

        public RecordScope(
            Stack<(string DeclaredType, string Key)> records,
            Stack<CfdNameResolver> names)
        {
            _records = records;
            _names = names;
        }

        public void Dispose()
        {
            if (_disposed) return;
            _disposed = true;
            if (_records.Count != 0) _records.Pop();
            if (_names.Count != 0) _names.Pop();
        }
    }

    public T Resolve<T>(string key) => Resolve<T>(typeof(T).Name, key);

    public CfdRecordNode? FindRecord(string declaredType, string key) =>
        Records.Find(declaredType, key);

    public T Resolve<T>(string declaredType, string key)
    {
        if (_cache.TryGetValue((declaredType, key), out var cached))
        {
            return cached is T cachedTyped
                ? cachedTyped
                : throw Error("CFD-REF-TYPE", $"CFD reference `{key}` is not a `{typeof(T).Name}`");
        }

        if (!_resolving.Add((declaredType, key)))
            throw Error("CFD-REF-CYCLE", $"CFD reference cycle detected at `{declaredType}.{key}`");
        try
        {
            var node = FindAssignableRecord(declaredType, key);
            if (node is null)
                throw Error("CFD-REF-MISSING", $"CFD reference `{key}` could not be resolved as `{declaredType}`");

            if (_cache.TryGetValue((node.DeclaredType, key), out var actualCached))
            {
                if (actualCached is not T actualTyped)
                    throw Error("CFD-REF-TYPE", $"CFD reference `{key}` is not a `{typeof(T).Name}`");
                _cache[(declaredType, key)] = actualTyped;
                return actualTyped;
            }

            var binding = Bindings.TryGetValue(node.DeclaredType, out var selected)
                ? selected
                : throw Error("CFD-REF-UNKNOWN-TYPE", $"CFD reference `{key}` uses unknown type `{node.DeclaredType}`");
            var value = WithRecordPath(node, () => binding.Read(node, this));
            if (value is not T typed)
                throw Error("CFD-REF-TYPE", $"CFD reference `{key}` produced an incompatible `{typeof(T).Name}` value");
            _cache[(node.DeclaredType, key)] = typed;
            _cache[(declaredType, key)] = typed;
            return typed;
        }
        finally
        {
            _resolving.Remove((declaredType, key));
        }
    }

    private CfdRecordNode? FindAssignableRecord(string declaredType, string key)
        => Records.FindAssignable(declaredType, key);

    public T WithRecordPath<T>(CfdRecordNode record, Func<T> read)
    {
        try
        {
            return read();
        }
        catch (CfdLoadException error)
        {
            var path = _recordPaths.TryGetValue(record, out var recordPath) ? recordPath : string.Empty;
            if (path.Length == 0 || error.Diagnostics.All(diagnostic => diagnostic.Path.Length != 0))
                throw;
            throw new CfdLoadException(error.Diagnostics
                .Select(diagnostic => diagnostic.Path.Length == 0
                    ? new CfdDiagnostic(diagnostic.Code, diagnostic.Message, path, diagnostic.Span)
                    : diagnostic)
                .ToList());
        }
    }

    public void WithRecordPath(CfdRecordNode record, Action read) =>
        WithRecordPath(record, () =>
        {
            read();
            return true;
        });

    private string CurrentPath
    {
        get
        {
            if (_currentRecords.Count == 0) return string.Empty;
            var current = _currentRecords.Peek();
            var record = FindRecord(current.DeclaredType, current.Key);
            return record is not null && _recordPaths.TryGetValue(record, out var path) ? path : string.Empty;
        }
    }

    private CfdSpan? CurrentSpan
    {
        get
        {
            if (_currentRecords.Count == 0) return null;
            var current = _currentRecords.Peek();
            return FindRecord(current.DeclaredType, current.Key)?.Span;
        }
    }

    internal bool IsAssignableType(string actualType, string expectedType) =>
        string.Equals(actualType, expectedType, StringComparison.Ordinal) ||
        Bindings.TryGetValue(actualType, out var binding) &&
        binding.AssignableTypes.Contains(expectedType, StringComparer.Ordinal);

    private CfdLoadException Error(string code, string message) =>
        new(new[] { new CfdDiagnostic(code, message, CurrentPath) });

    internal string RenderFormatted(CfdFormattedStringValue value)
    {
        if (!_formatting.Add(value))
            throw Error("CFD-VALUE-FORMAT", "formatted string reference cycle detected");
        var rendered = new System.Text.StringBuilder();
        try
        {
            foreach (var segment in value.Segments)
            {
                switch (segment)
                {
                    case CfdFormatText text:
                        rendered.Append(text.Text);
                        break;
                    case CfdFormatReference reference:
                        rendered.Append(FormatReference(reference, value));
                        break;
                }
            }
            return rendered.ToString();
        }
        finally
        {
            _formatting.Remove(value);
        }
    }

    private string FormatReference(CfdFormatReference reference, CfdValueNode source)
    {
        var type = reference.TypeName ?? CurrentRecordType;
        var key = reference.Key ?? CurrentRecordKey;
        if (type is null || key is null || reference.Path.Count == 0)
            throw new CfdLoadException(new[]
            {
                new CfdDiagnostic("CFD-VALUE-FORMAT", "formatted string reference has no current record", CurrentPath, source.Span),
            });
        var record = FindAssignableRecord(type, key);
        if (record is null)
            throw new CfdLoadException(new[]
            {
                new CfdDiagnostic("CFD-VALUE-FORMAT", $"formatted string record `{key}` was not found", CurrentPath, source.Span),
            });
        CfdValueNode? current = CfdValueReader.FindField(record.Fields, reference.Path[0]);
        if (current is null)
            throw new CfdLoadException(new[]
            {
                new CfdDiagnostic("CFD-VALUE-FORMAT", $"formatted string field `{reference.Path[0]}` was not found", CurrentPath, source.Span),
            });
        if (reference.Path.Count == 1)
            return Stringify(current);
        if (!Bindings.TryGetValue(record.DeclaredType, out var currentBinding))
            throw Error("CFD-REF-UNKNOWN-TYPE", $"formatted string record uses unknown type `{record.DeclaredType}`");
        var objectType = currentBinding.ObjectFieldType(reference.Path[0]);
        var referenceType = currentBinding.ReferenceFieldType(reference.Path[0]);
        foreach (var field in reference.Path.Skip(1))
        {
            var nextBinding = currentBinding;
            IReadOnlyList<CfdFieldNode>? fields = current switch
            {
                CfdObjectValue objectValue => ObjectFields(objectValue, objectType, out nextBinding),
                CfdReferenceValue recordReference => ReferenceFields(
                    recordReference, referenceType, out nextBinding),
                _ => null,
            };
            current = fields is null ? null : CfdValueReader.FindField(fields, field);
            if (current is null || nextBinding is null)
                throw new CfdLoadException(new[]
                {
                    new CfdDiagnostic("CFD-VALUE-FORMAT", $"formatted string field `{field}` was not found", CurrentPath, source.Span),
                });
            currentBinding = nextBinding;
            objectType = currentBinding.ObjectFieldType(field);
            referenceType = currentBinding.ReferenceFieldType(field);
        }
        return Stringify(current);
    }

    private IReadOnlyList<CfdFieldNode>? ObjectFields(
        CfdObjectValue value,
        string? declaredType,
        out ICfdTypeBinding? binding)
    {
        var actualType = value.DeclaredType ?? declaredType;
        binding = actualType is not null && Bindings.TryGetValue(actualType, out var selected)
            ? selected
            : null;
        return binding is null ? null : value.Fields;
    }

    private IReadOnlyList<CfdFieldNode>? ReferenceFields(
        CfdReferenceValue value,
        string? declaredType,
        out ICfdTypeBinding? binding)
    {
        var actualType = value.TypeName ?? declaredType;
        var record = actualType is null ? null : FindAssignableRecord(actualType, value.Key);
        binding = record is not null && Bindings.TryGetValue(record.DeclaredType, out var selected)
            ? selected
            : null;
        return binding is null ? null : record!.Fields;
    }

    private string Stringify(CfdValueNode value) => value switch
    {
        CfdInvalidValue => "<invalid>",
        CfdScalarValue scalar => scalar.Value,
        CfdStringValue text => text.Value,
        CfdFormattedStringValue formatted => RenderFormatted(formatted),
        CfdReferenceValue reference => $"&{reference.Key}",
        _ => throw new CfdLoadException(new[]
        {
            new CfdDiagnostic("CFD-VALUE-FORMAT", "formatted string reference must resolve to a scalar", CurrentPath, value.Span),
        }),
    };
}

[EditorBrowsable(EditorBrowsableState.Never)]
public interface ICfdTypeBinding
{
    string DeclaredType { get; }
    IReadOnlyList<string> AssignableTypes { get; }
    string? ObjectFieldType(string fieldName);
    string? ReferenceFieldType(string fieldName);
    object Read(CfdRecordNode record, CfdLoadContext context);
}

internal static class CfdLoader
{
    public static IReadOnlyList<CfdDocument> LoadDocuments(
        ICfdTextLoader loader,
        IEnumerable<string> paths)
    {
        if (loader is null) throw new ArgumentNullException(nameof(loader));
        if (paths is null) throw new ArgumentNullException(nameof(paths));
        var sources = new List<CfdSource>();
        var errors = new List<CfdDiagnostic>();
        foreach (var path in paths)
        {
            if (!loader.TryLoad(path, out var text) || text is null)
            {
                errors.Add(new CfdDiagnostic("CFD-SOURCE-MISSING", $"CFD source `{path}` was not found", path));
                continue;
            }
            sources.Add(new CfdSource(path, text));
        }
        if (errors.Count != 0) throw new CfdLoadException(errors);
        return CfdParser.ParseAll(sources);
    }
}

/// <summary>Schema-independent conversions used by generated C# readers.</summary>
[EditorBrowsable(EditorBrowsableState.Never)]
public static class CfdValueReader
{
    private static readonly System.Runtime.CompilerServices.ConditionalWeakTable<
        IReadOnlyList<CfdFieldNode>, CfdFieldIndex> FieldIndexes = new();

    public static string String(CfdValueNode node) => node switch
    {
        CfdStringValue value => value.Value,
        CfdFormattedStringValue value => value.Source,
        CfdConstantValue { Constant.Value: string value } => value,
        _ => throw Invalid(node, "string"),
    };

    public static string String(CfdValueNode node, CfdLoadContext context) =>
        node is CfdFormattedStringValue formatted
            ? context.RenderFormatted(formatted)
            : String(node);

    public static int Int32(CfdValueNode node) => node switch
    {
        CfdConstantValue { Constant.Value: int value } => value,
        CfdConstantValue { Constant.Value: long value } when value is >= int.MinValue and <= int.MaxValue => (int)value,
        _ when int.TryParse(ScalarText(node), NumberStyles.Integer, CultureInfo.InvariantCulture, out var value) => value,
        _ => throw Invalid(node, "32-bit integer", "CFD-VALUE-NUMERIC"),
    };

    public static long Int64(CfdValueNode node) => node switch
    {
        CfdConstantValue { Constant.Value: long value } => value,
        CfdConstantValue { Constant.Value: int value } => value,
        _ when long.TryParse(ScalarText(node), NumberStyles.Integer, CultureInfo.InvariantCulture, out var value) => value,
        _ => throw Invalid(node, "64-bit integer", "CFD-VALUE-NUMERIC"),
    };

    public static float Float32(CfdValueNode node) => node switch
    {
        CfdConstantValue { Constant.Value: float value } when float.IsFinite(value) => value,
        CfdConstantValue { Constant.Value: double value } when double.IsFinite(value) &&
            value is >= -float.MaxValue and <= float.MaxValue => (float)value,
        _ when float.TryParse(ScalarText(node), NumberStyles.Float, CultureInfo.InvariantCulture, out var value)
            && float.IsFinite(value) => value,
        _ => throw Invalid(node, "32-bit finite number", "CFD-VALUE-NUMERIC"),
    };

    public static double Float64(CfdValueNode node) => node switch
    {
        CfdConstantValue { Constant.Value: double value } when double.IsFinite(value) => value,
        CfdConstantValue { Constant.Value: float value } when float.IsFinite(value) => value,
        _ when double.TryParse(ScalarText(node), NumberStyles.Float, CultureInfo.InvariantCulture, out var value)
            && double.IsFinite(value) => value,
        _ => throw Invalid(node, "64-bit finite number", "CFD-VALUE-NUMERIC"),
    };

    public static bool Boolean(CfdValueNode node)
    {
        if (node is CfdConstantValue { Constant.Value: bool constant }) return constant;
        if (node is not CfdScalarValue scalar)
            throw Invalid(node, "boolean", "CFD-VALUE-BOOLEAN");
        if (scalar.Value.Equals("true", StringComparison.Ordinal)) return true;
        if (scalar.Value.Equals("false", StringComparison.Ordinal)) return false;
        throw Invalid(node, "boolean", "CFD-VALUE-BOOLEAN");
    }

    public static T Enum<T>(CfdValueNode node) where T : struct, System.Enum =>
        node is CfdConstantValue { Constant.Value: T constant }
            ? constant
            : typeof(T).IsDefined(typeof(FlagsAttribute), false)
            ? Flags<T>(node, typeof(T).Name, DeclaredMask<T>(), ResolveEnumValue<T>)
            : EnumToken(node, typeof(T).Name, ResolveEnumToken<T>);

    public static T EnumText<T>(string value) where T : struct, System.Enum =>
        TryEnumToken(value, typeof(T).Name, ResolveEnumToken<T>);

    public static T Enum<T>(CfdValueNode node, string enumName, Func<string, T?> resolve)
        where T : struct, System.Enum
    {
        if (node is CfdConstantValue { Constant.Value: T constant }) return constant;
        var token = EnumToken(node, enumName);
        return resolve(token) ?? throw Invalid(node, $"enum `{enumName}`", "CFD-VALUE-ENUM");
    }

    public static T EnumText<T>(string value, string enumName, Func<string, T?> resolve)
        where T : struct, System.Enum =>
        resolve(value) ?? throw Invalid(null, $"enum `{enumName}`", "CFD-VALUE-ENUM");

    public static T Flags<T>(CfdValueNode node, string enumName, long declaredMask, Func<string, long?> resolve)
        where T : struct, System.Enum
    {
        var value = node switch
        {
            CfdScalarValue scalar => ResolveFlagToken(scalar.Value, node, enumName, resolve),
            CfdBitExpressionValue expression => EvaluateFlags(expression.Expression, node, enumName, resolve),
            _ => throw Invalid(node, $"flag enum `{enumName}`", "CFD-VALUE-ENUM"),
        };
        ValidateFlagMask(value, declaredMask, node, enumName);
        return (T)System.Enum.ToObject(typeof(T), value);
    }

    public static T Reference<T>(CfdValueNode node, CfdLoadContext context, string declaredType) =>
        node is CfdConstantValue constant
            ? (T)context.ResolveConstant(constant.Constant)
            : node is CfdReferenceValue reference
            ? context.Resolve<T>(ReferenceType(reference, context, declaredType), reference.Key)
            : throw Invalid(node, $"reference to {declaredType}");

    private static string ReferenceType(
        CfdReferenceValue reference,
        CfdLoadContext context,
        string expectedType)
    {
        if (reference.TypeName is null) return expectedType;
        if (!context.IsAssignableType(reference.TypeName, expectedType))
            throw Invalid(reference, $"reference assignable to `{expectedType}`", "CFD-REF-TYPE");
        return reference.TypeName;
    }

    public static T Object<T>(CfdValueNode node, CfdLoadContext context,
        Func<IReadOnlyList<CfdFieldNode>, string, CfdLoadContext, T> read) =>
        Object(node, context, null, read);

    /// <summary>Returns the explicit type marker carried by an object value.</summary>
    public static string? ObjectDeclaredType(CfdValueNode node) => node switch
    {
        // 生成代码只能依赖公开 ABI，不能识别 Runtime 内部的语法树节点类型。
        CfdObjectValue value => value.DeclaredType,
        CfdDictionaryValue => null,
        _ => throw Invalid(node, "object"),
    };

    /// <summary>Returns whether the value is the explicit <c>None</c> literal.</summary>
    public static bool IsNone(CfdValueNode node) => node is CfdNoneValue;

    public static T Object<T>(CfdValueNode node, CfdLoadContext context, string? expectedType,
        Func<IReadOnlyList<CfdFieldNode>, string, CfdLoadContext, T> read)
    {
        if (node is CfdConstantValue constant)
            return (T)context.ResolveConstant(constant.Constant);
        if (node is CfdObjectValue objectValue)
        {
            if (expectedType is not null && objectValue.DeclaredType is not null &&
                !context.IsAssignableType(objectValue.DeclaredType, expectedType))
                throw Invalid(node, $"object `{expectedType}`", "CFD-VALUE-OBJECT-TYPE");
            return read(objectValue.Fields, string.Empty, context);
        }
        if (node is CfdDictionaryValue dictionary)
            return read(BlockFields(dictionary, node), string.Empty, context);
        throw Invalid(node, "object");
    }

    public static IReadOnlyList<T> Array<T>(CfdValueNode node, CfdLoadContext context,
        Func<CfdValueNode, CfdLoadContext, T> read) => node switch
        {
            CfdConstantValue constant =>
                (IReadOnlyList<T>)context.ResolveConstant(constant.Constant),
            CfdArrayValue value => value.Items.Select(item => read(item, context)).ToList(),
            _ => throw Invalid(node, "array"),
        };

    public static Option<T> Option<T>(CfdValueNode node, CfdLoadContext context,
        Func<CfdValueNode, CfdLoadContext, T> read) => node switch
        {
            CfdConstantValue constant =>
                (Option<T>)context.ResolveConstant(constant.Constant),
            CfdNoneValue => global::CoflowRuntime.Option<T>.None,
            CfdSomeValue some => global::CoflowRuntime.Option<T>.Some(read(some.Value, context)),
            _ => global::CoflowRuntime.Option<T>.Some(read(node, context)),
        };

    public static Result<T, TError> Result<T, TError>(
        CfdValueNode node,
        CfdLoadContext context,
        Func<CfdValueNode, CfdLoadContext, T> readValue,
        Func<CfdValueNode, CfdLoadContext, TError> readError) => node switch
        {
            CfdConstantValue constant =>
                (Result<T, TError>)context.ResolveConstant(constant.Constant),
            CfdOkValue ok => global::CoflowRuntime.Result<T, TError>.Ok(readValue(ok.Value, context)),
            CfdErrValue error => global::CoflowRuntime.Result<T, TError>.Err(readError(error.Value, context)),
            _ => throw Invalid(node, "Result value `Ok(value)` or `Err(error)`"),
        };

    public static IReadOnlyDictionary<TKey, TValue> Dictionary<TKey, TValue>(CfdValueNode node,
        CfdLoadContext context,
        Func<CfdValueNode, CfdLoadContext, TKey> readKey,
        Func<CfdValueNode, CfdLoadContext, TValue> readValue)
        where TKey : notnull
    {
        if (node is CfdConstantValue constant)
            return (IReadOnlyDictionary<TKey, TValue>)context.ResolveConstant(constant.Constant);
        if (node is not CfdDictionaryValue value)
            throw Invalid(node, "dictionary");
        var result = new Dictionary<TKey, TValue>();
        foreach (var entry in value.Entries)
        {
            var key = readKey(entry.Key, context);
            if (!result.TryAdd(key, readValue(entry.Value, context)))
            {
                throw new CfdLoadException(new[]
                {
                    new CfdDiagnostic("CFD-DICT-DUPLICATE", $"dictionary key `{key}` is declared more than once", string.Empty, entry.Span),
                });
            }
        }
        return result;
    }

    public static CfdValueNode Field(IReadOnlyList<CfdFieldNode> fields, string name) =>
        Index(fields).Find(name)
        ?? throw new CfdLoadException(new[] { new CfdDiagnostic("CFD-FIELD-MISSING", $"CFD field `{name}` is missing", string.Empty) });

    public static CfdValueNode? FindField(IReadOnlyList<CfdFieldNode> fields, string name) =>
        Index(fields).Find(name);

    public static void ValidateFields(IReadOnlyList<CfdFieldNode> fields, params string[] expected)
    {
        var index = Index(fields);
        var known = new HashSet<string>(expected, StringComparer.Ordinal);
        var unknown = index.Fields.FirstOrDefault(field => !known.Contains(field.Name));
        if (unknown is not null)
        {
            throw new CfdLoadException(new[]
            {
                new CfdDiagnostic(
                    "CFD-FIELD-UNKNOWN",
                    $"CFD field `{unknown.Name}` is not declared by the generated type",
                    string.Empty,
                    unknown.Span),
            });
        }
    }

    private static CfdFieldIndex Index(IReadOnlyList<CfdFieldNode> fields)
    {
        if (fields is null) throw new ArgumentNullException(nameof(fields));
        return FieldIndexes.GetValue(fields, static value => new CfdFieldIndex(value));
    }

    private sealed class CfdFieldIndex
    {
        private readonly Dictionary<string, CfdValueNode> _values = new(StringComparer.Ordinal);

        internal CfdFieldIndex(IReadOnlyList<CfdFieldNode> fields)
        {
            Fields = fields;
            foreach (var field in fields)
            {
                if (_values.TryAdd(field.Name, field.Value)) continue;
                throw new CfdLoadException(new[]
                {
                    new CfdDiagnostic(
                        "CFD-SYNTAX-DUPLICATE-FIELD",
                        $"field `{field.Name}` is declared more than once",
                        string.Empty,
                        field.Span),
                });
            }
        }

        internal IReadOnlyList<CfdFieldNode> Fields { get; }
        internal CfdValueNode? Find(string name) => _values.TryGetValue(name, out var value) ? value : null;
    }

    private static string ScalarText(CfdValueNode node) => node is CfdScalarValue value
        ? value.Value
        : throw Invalid(node, "scalar value");

    public static string EnumToken(CfdValueNode node, string enumName) => node is CfdScalarValue value
        ? value.Value
        : throw Invalid(node, $"enum `{enumName}`", "CFD-VALUE-ENUM");

    public static T EnumToken<T>(CfdValueNode node, string enumName, Func<string, T?> resolve)
        where T : struct, System.Enum =>
        resolve(EnumToken(node, enumName)) ?? throw Invalid(node, $"enum `{enumName}`", "CFD-VALUE-ENUM");

    private static T TryEnumToken<T>(string value, string enumName, Func<string, T?> resolve)
        where T : struct, System.Enum =>
        resolve(value) ?? throw Invalid(null, $"enum `{enumName}`", "CFD-VALUE-ENUM");

    private static T? ResolveEnumToken<T>(string value) where T : struct, System.Enum
    {
        if (long.TryParse(value, NumberStyles.Integer, CultureInfo.InvariantCulture, out _)) return null;
        var token = value.StartsWith(typeof(T).Name + "::", StringComparison.Ordinal)
            ? value[(typeof(T).Name.Length + 2)..]
            : value;
        return System.Enum.TryParse<T>(token, ignoreCase: false, out var result) &&
            System.Enum.GetNames(typeof(T)).Contains(token, StringComparer.Ordinal)
                ? result
                : null;
    }

    private static long? ResolveEnumValue<T>(string value) where T : struct, System.Enum
    {
        var token = value.StartsWith(typeof(T).Name + "::", StringComparison.Ordinal)
            ? value[(typeof(T).Name.Length + 2)..]
            : value;
        if (long.TryParse(token, NumberStyles.Integer, CultureInfo.InvariantCulture, out var number)) return number;
        return ResolveEnumToken<T>(token) is { } result ? Convert.ToInt64(result, CultureInfo.InvariantCulture) : null;
    }

    private static long DeclaredMask<T>() where T : struct, System.Enum =>
        System.Enum.GetValues(typeof(T)).Cast<object>()
            .Aggregate(0L, (mask, value) => mask | Convert.ToInt64(value, CultureInfo.InvariantCulture));

    private static long ResolveFlagToken(string token, CfdValueNode node, string enumName, Func<string, long?> resolve)
    {
        if (long.TryParse(token, NumberStyles.Integer, CultureInfo.InvariantCulture, out var number)) return number;
        return resolve(token) ?? throw Invalid(node, $"flag enum `{enumName}`", "CFD-VALUE-ENUM");
    }

    private static long EvaluateFlags(CfdBitExpression expression, CfdValueNode node, string enumName, Func<string, long?> resolve) =>
        expression.Kind switch
        {
            CfdBitExpressionKind.Value value => ResolveFlagToken(value.Text, node, enumName, resolve),
            CfdBitExpressionKind.Binary binary => ApplyFlagOperator(binary.Operator,
                EvaluateFlags(binary.Left, node, enumName, resolve),
                EvaluateFlags(binary.Right, node, enumName, resolve)),
            _ => throw Invalid(node, $"flag enum `{enumName}`", "CFD-VALUE-ENUM"),
        };

    private static long ApplyFlagOperator(CfdBitOperator operation, long left, long right) => operation switch
    {
        CfdBitOperator.Or => left | right,
        CfdBitOperator.Xor => left ^ right,
        CfdBitOperator.And => left & right,
        _ => throw new ArgumentOutOfRangeException(nameof(operation)),
    };

    private static void ValidateFlagMask(long value, long mask, CfdValueNode node, string enumName)
    {
        if (value < 0 || (value & ~mask) != 0)
            throw Invalid(node, $"flag enum `{enumName}`", "CFD-VALUE-ENUM");
    }

    private static IReadOnlyList<CfdFieldNode> BlockFields(CfdDictionaryValue value, CfdValueNode node)
    {
        var fields = new List<CfdFieldNode>(value.Entries.Count);
        var names = new HashSet<string>(StringComparer.Ordinal);
        foreach (var entry in value.Entries)
        {
            var name = entry.Key switch
            {
                CfdScalarValue scalar => scalar.Value,
                CfdStringValue text => text.Value,
                _ => throw Invalid(node, "object field name"),
            };
            if (!names.Add(name))
            {
                throw new CfdLoadException(new[]
                {
                    new CfdDiagnostic(
                        "CFD-SYNTAX-DUPLICATE-FIELD",
                        $"field `{name}` is declared more than once",
                        string.Empty,
                        entry.Span),
                });
            }
            fields.Add(new CfdFieldNode(name, entry.Value, entry.Span));
        }
        return fields;
    }

    private static CfdLoadException Invalid(CfdValueNode? node, string expected, string code = "CFD-VALUE-TYPE") =>
        new(new[] { new CfdDiagnostic(code, node is null ? $"value is not {expected}" : $"CFD value at {node.Span} is not {expected}", string.Empty, node?.Span) });
}
