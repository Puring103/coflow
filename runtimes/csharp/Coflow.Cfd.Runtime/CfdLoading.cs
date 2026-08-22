namespace Coflow.Cfd.Runtime;

using System.Globalization;

/// <summary>
/// Per-load identity and reference state. Generated bindings register one
/// explicit reader for each schema type; no constructor discovery or reflection
/// is needed at runtime.
/// </summary>
public sealed class CfdLoadContext
{
    private readonly Dictionary<(string DeclaredType, string Key), object> _cache = new();
    private readonly HashSet<(string DeclaredType, string Key)> _resolving = new();
    private readonly Stack<(string DeclaredType, string Key)> _currentRecords = new();
    private readonly HashSet<CfdFormattedStringValue> _formatting = new();

    public CfdLoadContext(
        IReadOnlyList<CfdDocument> documents,
        CfdLoadOptions? options = null,
        IEnumerable<ICfdTypeBinding>? bindings = null)
    {
        Documents = documents;
        Options = options ?? new CfdLoadOptions();
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
        var seen = new HashSet<(string DomainType, string Key)>();
        var diagnostics = new List<CfdDiagnostic>();
        foreach (var document in documents)
        {
            foreach (var record in document.Records)
            {
                if (record.GroupType is not null &&
                    bindingMap.TryGetValue(record.DeclaredType, out var groupBinding) &&
                    !groupBinding.AssignableTypes.Contains(record.GroupType, StringComparer.Ordinal))
                {
                    diagnostics.Add(new CfdDiagnostic(
                        "CFD-RECORD-GROUP-TYPE",
                        $"record type `{record.DeclaredType}` is not assignable to group `{record.GroupType}`",
                        document.Path,
                        record.Span));
                }
                var domains = bindingMap.TryGetValue(record.DeclaredType, out var binding)
                    ? binding.AssignableTypes
                    : new[] { record.DeclaredType };
                var duplicate = false;
                foreach (var domain in domains)
                {
                    duplicate |= !seen.Add((domain, record.Key));
                }
                if (duplicate)
                {
                    diagnostics.Add(new CfdDiagnostic(
                        "CFD-SYNTAX-DUPLICATE-RECORD",
                        $"record key `{record.Key}` is declared more than once in an assignable type domain",
                        document.Path,
                        record.Span));
                }
            }
        }
        if (diagnostics.Count != 0)
            throw new CfdLoadException(diagnostics);
    }

    public IReadOnlyList<CfdDocument> Documents { get; }
    public CfdLoadOptions Options { get; }
    public IReadOnlyDictionary<string, ICfdTypeBinding> Bindings { get; }
    public List<CfdDiagnostic> Diagnostics { get; } = new();

    public IDisposable EnterRecord(string declaredType, string key)
    {
        _currentRecords.Push((declaredType, key));
        return new RecordScope(_currentRecords);
    }

    internal string? CurrentRecordType => _currentRecords.Count == 0 ? null : _currentRecords.Peek().DeclaredType;
    internal string? CurrentRecordKey => _currentRecords.Count == 0 ? null : _currentRecords.Peek().Key;

    private sealed class RecordScope : IDisposable
    {
        private readonly Stack<(string DeclaredType, string Key)> _records;
        private bool _disposed;

        public RecordScope(Stack<(string DeclaredType, string Key)> records) => _records = records;

        public void Dispose()
        {
            if (_disposed) return;
            _disposed = true;
            if (_records.Count != 0) _records.Pop();
        }
    }

    public T Resolve<T>(string key) => Resolve<T>(typeof(T).Name, key);

    public CfdRecordNode? FindRecord(string declaredType, string key) => Documents
        .SelectMany(document => document.Records)
        .FirstOrDefault(record => record.DeclaredType == declaredType && record.Key == key);

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

            var binding = Bindings.TryGetValue(node.DeclaredType, out var selected)
                ? selected
                : throw Error("CFD-REF-UNKNOWN-TYPE", $"CFD reference `{key}` uses unknown type `{node.DeclaredType}`");
            var value = WithRecordPath(node, () => binding.Read(node, this));
            if (value is not T typed)
                throw Error("CFD-REF-TYPE", $"CFD reference `{key}` produced an incompatible `{typeof(T).Name}` value");
            _cache[(declaredType, key)] = typed;
            return typed;
        }
        finally
        {
            _resolving.Remove((declaredType, key));
        }
    }

    private CfdRecordNode? FindAssignableRecord(string declaredType, string key)
    {
        var matches = Documents
            .SelectMany(document => document.Records)
            .Where(record => record.Key == key)
            .Where(record => record.DeclaredType == declaredType ||
                Bindings.TryGetValue(record.DeclaredType, out var binding) &&
                binding.AssignableTypes.Contains(declaredType, StringComparer.Ordinal))
            .ToList();
        if (matches.Count > 1)
            throw Error("CFD-REF-AMBIGUOUS", $"CFD reference `{key}` is ambiguous as `{declaredType}`");
        return matches.FirstOrDefault();
    }

    public T WithRecordPath<T>(CfdRecordNode record, Func<T> read)
    {
        try
        {
            return read();
        }
        catch (CfdLoadException error)
        {
            var path = Documents.FirstOrDefault(document => document.Records.Contains(record))?.Path
                ?? string.Empty;
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
            return Documents.FirstOrDefault(document => document.Records.Any(record =>
                record.DeclaredType == current.DeclaredType && record.Key == current.Key))?.Path ?? string.Empty;
        }
    }

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
        CfdValueNode? current = record.Fields.FirstOrDefault(field => field.Name == reference.Path[0])?.Value;
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
            current = fields?.FirstOrDefault(item => item.Name == field)?.Value;
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
        var record = declaredType is null ? null : FindAssignableRecord(declaredType, value.Key);
        binding = record is not null && Bindings.TryGetValue(record.DeclaredType, out var selected)
            ? selected
            : null;
        return binding is null ? null : record!.Fields;
    }

    private string Stringify(CfdValueNode value) => value switch
    {
        CfdNullValue => "null",
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

public interface ICfdTypeBinding
{
    string DeclaredType { get; }
    IReadOnlyList<string> AssignableTypes { get; }
    string? ObjectFieldType(string fieldName);
    string? ReferenceFieldType(string fieldName);
    object Read(CfdRecordNode record, CfdLoadContext context);
}

public static class CfdLoader
{
    public static IReadOnlyList<CfdDocument> LoadDocuments(
        ICfdTextLoader loader,
        IEnumerable<string> paths,
        CfdLoadOptions? options = null)
    {
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
        return CfdParser.ParseAll(sources, options);
    }
}

/// <summary>Schema-independent conversions used by generated C# readers.</summary>
public static class CfdValueReader
{
    public static string String(CfdValueNode node) => node switch
    {
        CfdStringValue value => value.Value,
        CfdFormattedStringValue value => value.Source,
        _ => throw Invalid(node, "string"),
    };

    public static string String(CfdValueNode node, CfdLoadContext context) =>
        node is CfdFormattedStringValue formatted
            ? context.RenderFormatted(formatted)
            : String(node);

    public static int Int32(CfdValueNode node) =>
        int.TryParse(ScalarText(node), NumberStyles.Integer, CultureInfo.InvariantCulture, out var value)
            ? value
            : throw Invalid(node, "32-bit integer", "CFD-VALUE-NUMERIC");

    public static long Int64(CfdValueNode node) =>
        long.TryParse(ScalarText(node), NumberStyles.Integer, CultureInfo.InvariantCulture, out var value)
            ? value
            : throw Invalid(node, "64-bit integer", "CFD-VALUE-NUMERIC");

    public static float Float32(CfdValueNode node) =>
        float.TryParse(ScalarText(node), NumberStyles.Float, CultureInfo.InvariantCulture, out var value)
            && !float.IsNaN(value) && !float.IsInfinity(value)
                ? value
                : throw Invalid(node, "32-bit finite number", "CFD-VALUE-NUMERIC");

    public static double Float64(CfdValueNode node) =>
        double.TryParse(ScalarText(node), NumberStyles.Float, CultureInfo.InvariantCulture, out var value)
            && !double.IsNaN(value) && !double.IsInfinity(value)
                ? value
                : throw Invalid(node, "64-bit finite number", "CFD-VALUE-NUMERIC");

    public static bool Boolean(CfdValueNode node)
    {
        if (node is not CfdScalarValue scalar)
            throw Invalid(node, "boolean", "CFD-VALUE-BOOLEAN");
        if (scalar.Value.Equals("true", StringComparison.Ordinal)) return true;
        if (scalar.Value.Equals("false", StringComparison.Ordinal)) return false;
        throw Invalid(node, "boolean", "CFD-VALUE-BOOLEAN");
    }

    public static T Enum<T>(CfdValueNode node) where T : struct, System.Enum =>
        typeof(T).IsDefined(typeof(FlagsAttribute), false)
            ? Flags<T>(node, typeof(T).Name, DeclaredMask<T>(), ResolveEnumValue<T>)
            : EnumToken(node, typeof(T).Name, ResolveEnumToken<T>);

    public static T EnumText<T>(string value) where T : struct, System.Enum =>
        TryEnumToken(value, typeof(T).Name, ResolveEnumToken<T>);

    public static T Enum<T>(CfdValueNode node, string enumName, Func<string, T?> resolve)
        where T : struct, System.Enum
    {
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
        node is CfdReferenceValue reference
            ? context.Resolve<T>(declaredType, reference.Key)
            : throw Invalid(node, $"reference to {declaredType}");

    public static T Object<T>(CfdValueNode node, CfdLoadContext context,
        Func<IReadOnlyList<CfdFieldNode>, string, CfdLoadContext, T> read) =>
        Object(node, context, null, read);

    public static T Object<T>(CfdValueNode node, CfdLoadContext context, string? expectedType,
        Func<IReadOnlyList<CfdFieldNode>, string, CfdLoadContext, T> read)
    {
        if (node is CfdObjectValue objectValue)
        {
            if (expectedType is not null && objectValue.DeclaredType is not null &&
                !string.Equals(expectedType, objectValue.DeclaredType, StringComparison.Ordinal))
                throw Invalid(node, $"object `{expectedType}`", "CFD-VALUE-OBJECT-TYPE");
            return read(objectValue.Fields, string.Empty, context);
        }
        if (node is CfdDictionaryValue dictionary)
            return read(BlockFields(dictionary, node), string.Empty, context);
        throw Invalid(node, "object");
    }

    public static IReadOnlyList<T> Array<T>(CfdValueNode node, CfdLoadContext context,
        Func<CfdValueNode, CfdLoadContext, T> read) => node is CfdArrayValue value
            ? value.Items.Select(item => read(item, context)).ToList()
            : throw Invalid(node, "array");

    public static IReadOnlyDictionary<TKey, TValue> Dictionary<TKey, TValue>(CfdValueNode node,
        CfdLoadContext context,
        Func<CfdValueNode, CfdLoadContext, TKey> readKey,
        Func<CfdValueNode, CfdLoadContext, TValue> readValue)
        where TKey : notnull
    {
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
        fields.FirstOrDefault(field => field.Name == name)?.Value
        ?? throw new CfdLoadException(new[] { new CfdDiagnostic("CFD-FIELD-MISSING", $"CFD field `{name}` is missing", string.Empty) });

    public static CfdValueNode? FindField(IReadOnlyList<CfdFieldNode> fields, string name) =>
        fields.FirstOrDefault(field => field.Name == name)?.Value;

    public static void ValidateFields(IReadOnlyList<CfdFieldNode> fields, params string[] expected)
    {
        var duplicate = fields
            .GroupBy(field => field.Name, StringComparer.Ordinal)
            .FirstOrDefault(group => group.Count() > 1);
        if (duplicate is not null)
        {
            var field = duplicate.First();
            throw new CfdLoadException(new[]
            {
                new CfdDiagnostic(
                    "CFD-SYNTAX-DUPLICATE-FIELD",
                    $"field `{field.Name}` is declared more than once",
                    string.Empty,
                    field.Span),
            });
        }
        var known = new HashSet<string>(expected, StringComparer.Ordinal);
        var unknown = fields.FirstOrDefault(field => !known.Contains(field.Name));
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

    public static T Nullable<T>(CfdValueNode node, CfdLoadContext context,
        Func<CfdValueNode, CfdLoadContext, T> read) where T : struct =>
        node is CfdNullValue ? default : read(node, context);

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
        var token = value.StartsWith(typeof(T).Name + ".", StringComparison.Ordinal)
            ? value[(typeof(T).Name.Length + 1)..]
            : value;
        return System.Enum.TryParse<T>(token, ignoreCase: false, out var result) &&
            System.Enum.GetNames(typeof(T)).Contains(token, StringComparer.Ordinal)
                ? result
                : null;
    }

    private static long? ResolveEnumValue<T>(string value) where T : struct, System.Enum
    {
        var token = value.StartsWith(typeof(T).Name + ".", StringComparison.Ordinal)
            ? value[(typeof(T).Name.Length + 1)..]
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
