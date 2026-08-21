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

    public CfdLoadContext(
        IReadOnlyList<CfdDocument> documents,
        CfdLoadOptions? options = null,
        IEnumerable<ICfdTypeBinding>? bindings = null)
    {
        Documents = documents;
        Options = options ?? new CfdLoadOptions();
        Bindings = (bindings ?? Array.Empty<ICfdTypeBinding>())
            .ToDictionary(binding => binding.DeclaredType, StringComparer.Ordinal);
    }

    public IReadOnlyList<CfdDocument> Documents { get; }
    public CfdLoadOptions Options { get; }
    public IReadOnlyDictionary<string, ICfdTypeBinding> Bindings { get; }
    public List<CfdDiagnostic> Diagnostics { get; } = new();

    public T Resolve<T>(string key) => Resolve<T>(typeof(T).Name, key);

    public T Resolve<T>(string declaredType, string key)
    {
        if (_cache.TryGetValue((declaredType, key), out var cached))
            return (T)cached;

        if (!_resolving.Add((declaredType, key)))
            throw Error("CFD-REF-CYCLE", $"CFD reference cycle detected at `{declaredType}.{key}`");

        var exactNode = Documents
            .SelectMany(document => document.Records)
            .FirstOrDefault(record => record.Key == key && record.DeclaredType == declaredType);
        var node = exactNode ?? Documents
            .SelectMany(document => document.Records)
            .FirstOrDefault(record => record.Key == key);
        if (node is null)
            throw Error("CFD-REF-MISSING", $"CFD reference `{key}` could not be resolved as `{declaredType}`");

        var binding = Bindings.TryGetValue(node.DeclaredType, out var selected)
            ? selected
            : throw Error("CFD-REF-UNKNOWN-TYPE", $"CFD reference `{key}` uses unknown type `{node.DeclaredType}`");

        var value = binding.Read(node, this);
        if (value is not T typed)
            throw Error("CFD-REF-TYPE", $"CFD reference `{key}` produced an incompatible `{typeof(T).Name}` value");
        _cache[(declaredType, key)] = typed;
        _resolving.Remove((declaredType, key));
        return typed;
    }

    private static CfdLoadException Error(string code, string message) =>
        new(new[] { new CfdDiagnostic(code, message, string.Empty) });
}

public interface ICfdTypeBinding
{
    string DeclaredType { get; }
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
        CfdScalarValue value => value.Value,
        _ => throw Invalid(node, "string"),
    };

    public static int Int32(CfdValueNode node) => int.Parse(String(node), CultureInfo.InvariantCulture);
    public static long Int64(CfdValueNode node) => long.Parse(String(node), CultureInfo.InvariantCulture);
    public static float Float32(CfdValueNode node) => float.Parse(String(node), CultureInfo.InvariantCulture);
    public static double Float64(CfdValueNode node) => double.Parse(String(node), CultureInfo.InvariantCulture);
    public static bool Boolean(CfdValueNode node) => bool.Parse(String(node));

    public static T Enum<T>(CfdValueNode node) where T : struct, System.Enum =>
        System.Enum.Parse<T>(String(node), ignoreCase: false);

    public static T EnumText<T>(string value) where T : struct, System.Enum =>
        System.Enum.Parse<T>(value, ignoreCase: false);

    public static T Reference<T>(CfdValueNode node, CfdLoadContext context, string declaredType) =>
        node is CfdReferenceValue reference
            ? context.Resolve<T>(declaredType, reference.Key)
            : throw Invalid(node, $"reference to {declaredType}");

    public static T Object<T>(CfdValueNode node, CfdLoadContext context,
        Func<IReadOnlyList<CfdFieldNode>, string, CfdLoadContext, T> read) => node switch
    {
        CfdObjectValue value => read(value.Fields, string.Empty, context),
        _ => throw Invalid(node, "object"),
    };

    public static IReadOnlyList<T> Array<T>(CfdValueNode node, CfdLoadContext context,
        Func<CfdValueNode, CfdLoadContext, T> read) => node is CfdArrayValue value
            ? value.Items.Select(item => read(item, context)).ToList()
            : throw Invalid(node, "array");

    public static IReadOnlyDictionary<TKey, TValue> Dictionary<TKey, TValue>(CfdValueNode node,
        CfdLoadContext context,
        Func<CfdValueNode, CfdLoadContext, TKey> readKey,
        Func<CfdValueNode, CfdLoadContext, TValue> readValue)
        where TKey : notnull => node is CfdDictionaryValue value
            ? value.Entries.ToDictionary(entry => readKey(entry.Key, context), entry => readValue(entry.Value, context))
            : throw Invalid(node, "dictionary");

    public static CfdValueNode Field(IReadOnlyList<CfdFieldNode> fields, string name) =>
        fields.FirstOrDefault(field => field.Name == name)?.Value
        ?? throw new CfdLoadException(new[] { new CfdDiagnostic("CFD-FIELD-MISSING", $"CFD field `{name}` is missing", string.Empty) });

    public static T Nullable<T>(CfdValueNode node, CfdLoadContext context,
        Func<CfdValueNode, CfdLoadContext, T> read) where T : struct =>
        node is CfdNullValue ? default : read(node, context);

    private static CfdLoadException Invalid(CfdValueNode node, string expected) =>
        new(new[] { new CfdDiagnostic("CFD-VALUE-TYPE", $"CFD value at {node.Span} is not {expected}", string.Empty, node.Span) });
}
