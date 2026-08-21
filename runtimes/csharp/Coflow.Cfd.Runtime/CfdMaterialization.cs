namespace Coflow.Cfd.Runtime;

using System.Collections;
using System.Globalization;
using System.Reflection;

public sealed class CfdLoadContext
{
    private readonly Dictionary<(Type Type, string? DeclaredType, string Key), object> _cache = new();

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

    public T Resolve<T>(string key) => (T)Resolve(typeof(T), key, null);

    public T Resolve<T>(string declaredType, string key) =>
        (T)Resolve(typeof(T), key, declaredType);

    internal object Resolve(Type target, string key) => Resolve(target, key, null);

    private object Resolve(Type target, string key, string? declaredType)
    {
        if (_cache.TryGetValue((target, declaredType, key), out var cached)) return cached;
        foreach (var node in Documents.SelectMany(document => document.Records)
            .Where(node => node.Key == key && (declaredType is null || node.DeclaredType == declaredType)))
        {
            try
            {
                var value = CfdMaterializer.CreateForContext(target, node, this);
                if (value is not null && target.IsInstanceOfType(value))
                {
                    _cache[(target, declaredType, key)] = value;
                    return value;
                }
            }
            catch (Exception) when (target != typeof(string)) { }
        }
        throw new KeyNotFoundException($"CFD reference `{key}` could not be resolved as `{target.Name}`");
    }
}

public interface ICfdMaterializer<out T>
{
    T Materialize(CfdRecordNode node, CfdLoadContext context);
}

public interface ICfdTypeBinding
{
    string DeclaredType { get; }
    object Read(CfdRecordNode record, CfdLoadContext context);
}

public static class CfdLoader
{
    public static IReadOnlyList<CfdDocument> LoadDocuments(
        ICfdSourceProvider provider,
        IEnumerable<string> paths,
        CfdLoadOptions? options = null)
    {
        var sources = new List<CfdSource>();
        var errors = new List<CfdDiagnostic>();
        foreach (var path in paths)
        {
            if (!provider.TryLoad(path, out var text) || text is null)
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

/// <summary>
/// Materializes generated model types from the schema-free node tree. Generated
/// bindings select the target type; this constructor mapper handles generic
/// value shapes and remains usable for hand-written model types.
/// </summary>
public static class CfdMaterializer
{
    public static T Materialize<T>(CfdRecordNode node, CfdLoadContext context) =>
        (T)Create(typeof(T), node.Fields, node.Key, context)!;

    internal static object? CreateForContext(Type target, CfdRecordNode node, CfdLoadContext context) =>
        context.Bindings.TryGetValue(node.DeclaredType, out var binding)
            ? binding.Read(node, context)
            : Create(target, node.Fields, node.Key, context);

    private static object? Create(Type target, IReadOnlyList<CfdFieldNode> fields, string key, CfdLoadContext context)
    {
        if (target == typeof(string)) return key;
        var constructors = target.GetConstructors(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance)
            .OrderByDescending(constructor => constructor.GetParameters().Length);
        foreach (var constructor in constructors)
        {
            var parameters = constructor.GetParameters();
            var values = new object?[parameters.Length];
            var valid = true;
            for (var index = 0; index < parameters.Length; index++)
            {
                var parameter = parameters[index];
                var field = fields.FirstOrDefault(candidate => string.Equals(candidate.Name, parameter.Name, StringComparison.OrdinalIgnoreCase));
                if (field is null && string.Equals(parameter.Name, "id", StringComparison.OrdinalIgnoreCase))
                    values[index] = ConvertScalar(key, parameter.ParameterType);
                else if (field is null && parameter.HasDefaultValue)
                    values[index] = parameter.DefaultValue;
                else if (field is null)
                {
                    values[index] = parameter.ParameterType.IsValueType ? Activator.CreateInstance(parameter.ParameterType) : null;
                }
                else
                {
                    values[index] = ConvertValue(field.Value, parameter.ParameterType, context);
                }
                if (values[index] is null && parameter.ParameterType.IsValueType && Nullable.GetUnderlyingType(parameter.ParameterType) is null)
                    valid = false;
            }
            if (valid)
            {
                try { return constructor.Invoke(values); }
                catch (TargetInvocationException) { }
            }
        }
        throw new InvalidOperationException($"generated type `{target.FullName}` has no usable constructor");
    }

    private static object? ConvertValue(CfdValueNode value, Type target, CfdLoadContext context)
    {
        var nullable = Nullable.GetUnderlyingType(target);
        if (value is CfdNullValue) return null;
        if (nullable is not null) return ConvertValue(value, nullable, context);
        if (value is CfdStringValue text) return ConvertScalar(text.Value, target);
        if (value is CfdScalarValue scalar) return ConvertScalar(scalar.Value, target);
        if (value is CfdReferenceValue reference) return context.Resolve(target, reference.Key);
        if (value is CfdArrayValue array)
        {
            var itemType = target.IsArray ? target.GetElementType()! : target.GetGenericArguments().FirstOrDefault() ?? typeof(object);
            var items = array.Items.Select(item => ConvertValue(item, itemType, context)).ToArray();
            if (target.IsArray)
            {
                var result = Array.CreateInstance(itemType, items.Length);
                for (var index = 0; index < items.Length; index++) result.SetValue(items[index], index);
                return result;
            }
            var list = (IList)Activator.CreateInstance(typeof(List<>).MakeGenericType(itemType))!;
            foreach (var item in items) list.Add(item);
            return list;
        }
        if (value is CfdObjectValue objectValue)
            return Create(target, objectValue.Fields, string.Empty, context);
        if (value is CfdDictionaryValue dictionary)
        {
            if (!target.IsGenericType && target != typeof(object))
            {
                var fields = dictionary.Entries
                    .Select(entry => new CfdFieldNode(ScalarText(entry.Key), entry.Value, entry.Span))
                    .ToList();
                return Create(target, fields, string.Empty, context);
            }
            var arguments = target.GetGenericArguments();
            var keyType = arguments.Length > 0 ? arguments[0] : typeof(string);
            var valueType = arguments.Length > 1 ? arguments[1] : typeof(object);
            var result = (IDictionary)Activator.CreateInstance(typeof(Dictionary<,>).MakeGenericType(keyType, valueType))!;
            foreach (var entry in dictionary.Entries)
                result.Add(ConvertValue(entry.Key, keyType, context)!, ConvertValue(entry.Value, valueType, context));
            return result;
        }
        return target.IsValueType ? Activator.CreateInstance(target) : null;
    }

    private static object? ConvertScalar(string value, Type target)
    {
        if (target == typeof(string)) return value;
        if (target == typeof(bool)) return bool.Parse(value);
        if (target == typeof(int)) return int.Parse(value, CultureInfo.InvariantCulture);
        if (target == typeof(long)) return long.Parse(value, CultureInfo.InvariantCulture);
        if (target == typeof(float)) return float.Parse(value, CultureInfo.InvariantCulture);
        if (target == typeof(double)) return double.Parse(value, CultureInfo.InvariantCulture);
        if (target == typeof(decimal)) return decimal.Parse(value, CultureInfo.InvariantCulture);
        if (target.IsEnum) return Enum.Parse(target, value, ignoreCase: false);
        return Convert.ChangeType(value, target, CultureInfo.InvariantCulture);
    }

    private static string ScalarText(CfdValueNode value) => value switch
    {
        CfdStringValue text => text.Value,
        CfdScalarValue scalar => scalar.Value,
        _ => throw new InvalidOperationException("CFD dictionary keys must be scalar values"),
    };
}
