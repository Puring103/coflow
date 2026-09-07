namespace Coflow.Runtime.CompilerServices;

using System.ComponentModel;

[EditorBrowsable(EditorBrowsableState.Never)]
public enum CoflowAnnotationArgumentKind { Name, String, Int, Float, Bool }

[EditorBrowsable(EditorBrowsableState.Never)]
public sealed class CoflowAnnotationArgument
{
    public CoflowAnnotationArgument(CoflowAnnotationArgumentKind kind, object value)
    {
        Kind = kind;
        Value = value ?? throw new ArgumentNullException(nameof(value));
    }
    public CoflowAnnotationArgumentKind Kind { get; }
    public object Value { get; }
}

[EditorBrowsable(EditorBrowsableState.Never)]
public sealed class CoflowAnnotation
{
    public CoflowAnnotation(string name, IReadOnlyList<CoflowAnnotationArgument> arguments)
    {
        Name = name ?? throw new ArgumentNullException(nameof(name));
        Arguments = arguments ?? throw new ArgumentNullException(nameof(arguments));
    }
    public string Name { get; }
    public IReadOnlyList<CoflowAnnotationArgument> Arguments { get; }
}

[EditorBrowsable(EditorBrowsableState.Never)]
public sealed class CoflowConstant
{
    private readonly object? _value;
    private readonly Func<CfdLoadContext, object>? _factory;

    public CoflowConstant(string declaredName, Type runtimeType, object value)
    {
        DeclaredName = declaredName ?? throw new ArgumentNullException(nameof(declaredName));
        RuntimeType = runtimeType ?? throw new ArgumentNullException(nameof(runtimeType));
        _value = value ?? throw new ArgumentNullException(nameof(value));
        if (!runtimeType.IsInstanceOfType(value))
            throw new ArgumentException("The constant value does not match its schema type.", nameof(value));
    }

    public CoflowConstant(string declaredName, Type runtimeType, Func<CfdLoadContext, object> factory)
    {
        DeclaredName = declaredName ?? throw new ArgumentNullException(nameof(declaredName));
        RuntimeType = runtimeType ?? throw new ArgumentNullException(nameof(runtimeType));
        _factory = factory ?? throw new ArgumentNullException(nameof(factory));
    }

    public string DeclaredName { get; }
    public Type RuntimeType { get; }
    public object Value => _value ?? throw new InvalidOperationException(
        $"Coflow constant `{DeclaredName}` is resolved while a module is loaded.");

    internal object Resolve(CfdLoadContext context)
    {
        var value = _factory is null ? _value! : _factory(context);
        if (!RuntimeType.IsInstanceOfType(value))
            throw new CoflowLoadException(new[] { new CfdDiagnostic(
                "COFLOW-CONSTANT-TYPE",
                $"constant `{DeclaredName}` produced `{value?.GetType()}` instead of `{RuntimeType}`",
                string.Empty) });
        return value;
    }
}

[EditorBrowsable(EditorBrowsableState.Never)]
public static class CoflowConstantValues
{
    public static IReadOnlyList<T> List<T>(params T[] values) =>
        Array.AsReadOnly(values ?? throw new ArgumentNullException(nameof(values)));

    public static IReadOnlyDictionary<TKey, TValue> Dictionary<TKey, TValue>(
        params KeyValuePair<TKey, TValue>[] entries) where TKey : notnull
    {
        if (entries is null) throw new ArgumentNullException(nameof(entries));
        var values = new Dictionary<TKey, TValue>();
        foreach (var entry in entries)
            if (!values.TryAdd(entry.Key, entry.Value))
                throw new ArgumentException($"Duplicate constant key `{entry.Key}`.", nameof(entries));
        return new System.Collections.ObjectModel.ReadOnlyDictionary<TKey, TValue>(values);
    }
}

[EditorBrowsable(EditorBrowsableState.Never)]
public interface ICoflowSchema
{
    CoflowSchemaRuntime Runtime { get; }
    IReadOnlyList<ICoflowTypeMetadata> Types { get; }
    IReadOnlyList<ICoflowEnumMetadata> Enums { get; }
    IReadOnlyList<CoflowConstant> Constants { get; }
}

[EditorBrowsable(EditorBrowsableState.Never)]
public interface ICoflowEnumMetadata
{
    string DeclaredType { get; }
    Type RuntimeType { get; }
    bool IsFlags { get; }
    IReadOnlyList<CoflowAnnotation> Annotations { get; }
    IReadOnlyDictionary<string, object> Variants { get; }
    IReadOnlyList<CoflowAnnotation> VariantAnnotations(string variantName);
    object FromInt64(long value);
}

[EditorBrowsable(EditorBrowsableState.Never)]
public interface ICoflowTypeMetadata : ICfdTypeBinding
{
    CoflowTypeId TypeId { get; }
    Type RuntimeType { get; }
    bool IsSingleton { get; }
    bool IsAbstract { get; }
    bool IsSealed { get; }
    IReadOnlyList<CoflowAnnotation> Annotations { get; }
    IReadOnlyList<CoflowTypeId> AssignableTypeIds { get; }
    object CreateObject(CfdLoadContext context, IReadOnlyDictionary<string, object?> fields);
    CoflowVmFactory CreateVmObjectFactory(CfdLoadContext context);
    CoflowVmFactory CreateVmDefaultFactory(string fieldName, CfdLoadContext context);
    CoflowValueId GetValueId(object value);
    object WithValueId(object value, CoflowValueId id);
}

[EditorBrowsable(EditorBrowsableState.Never)]
public sealed class CoflowFieldMetadata
{
    public CoflowFieldMetadata(CoflowFieldBinding binding,
        IReadOnlyList<CoflowAnnotation> annotations, bool hasDefault,
        string? objectType = null, string? referenceType = null)
    {
        Binding = binding ?? throw new ArgumentNullException(nameof(binding));
        Annotations = annotations ?? throw new ArgumentNullException(nameof(annotations));
        HasDefault = hasDefault;
        ObjectType = objectType;
        ReferenceType = referenceType;
    }

    public string Name => Binding.Name;
    public CoflowFieldBinding Binding { get; }
    public IReadOnlyList<CoflowAnnotation> Annotations { get; }
    public bool HasDefault { get; }
    public string? ObjectType { get; }
    public string? ReferenceType { get; }
}

internal static class CoflowMetadataFields
{
    internal static CoflowFieldMetadata? Find(this ICfdTypeBinding metadata, string name) =>
        metadata.Fields.FirstOrDefault(field => string.Equals(field.Name, name, StringComparison.Ordinal));

    internal static CoflowFieldMetadata Require(this ICoflowTypeMetadata metadata, string name) =>
        metadata.Find(name) ?? throw new ArgumentException($"unknown field `{name}`", nameof(name));
}

[EditorBrowsable(EditorBrowsableState.Never)]
public interface ICoflowRecordMetadata : ICoflowTypeMetadata
{
    Type KeyType { get; }
    object ParseKey(string key);
    object GetKey(object value);
    CoflowTable CreateTable(object[] values);
    object CreateRecord(string key, CfdLoadContext context);
    void PopulateRecord(object target, CfdRecordNode record, CfdLoadContext context);
}

[EditorBrowsable(EditorBrowsableState.Never)]
public interface ICoflowHostMetadata : ICoflowTypeMetadata
{
    object? BindHost(object? value, CfdLoadContext context);
}
