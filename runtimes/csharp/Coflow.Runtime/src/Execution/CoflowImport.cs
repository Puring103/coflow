using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System.Collections.Generic;
using System;
namespace Coflow.Runtime.CompilerServices
{

using System.Collections.ObjectModel;
using System.ComponentModel;

[EditorBrowsable(EditorBrowsableState.Never)]
public sealed class CoflowImportContext
{
    private CoflowSnapshot _snapshot = null!;
    private CoflowImportState _imports = null!;
    private CoflowCollectionArena? _collections;
    private int _importDepth;

    internal CoflowImportContext() { }

    internal void Reset(CoflowSnapshot snapshot, CoflowImportState imports)
    {
        _snapshot = snapshot;
        _imports = imports;
    }

    internal void Clear()
    {
        _snapshot = null!;
        _imports = null!;
        _collections = null;
        _importDepth = 0;
    }

    public T Import<T>(T value) => CoflowImportValue<T>.Normalize(this, value);

    internal T Import<T>(T value, CoflowCollectionArena collections)
    {
        if (_importDepth == 0) _collections = collections;
        else if (!ReferenceEquals(_collections, collections))
            throw new InvalidOperationException("A nested import cannot switch collection Arenas.");
        _importDepth++;
        try
        {
            return Import(value);
        }
        finally
        {
            _importDepth--;
            if (_importDepth == 0) _collections = null;
        }
    }

    internal void ReserveCollectionElements(int count)
    {
        (_collections ?? throw new InvalidOperationException(
            "A collection must be imported into an execution Arena."))
            .ReserveElements(count);
    }

    internal T ImportRecord<T>(CoflowTypeDescriptor<T> descriptor, T value)
    {
        var id = descriptor.GetValueId(value);
        if (!descriptor.IsInitialized(value))
            throw new CoflowBoundaryException($"The `{typeof(T)}` value is an uninitialized default struct.");
        if (id.IsValid)
        {
            _snapshot.ValidateValue(id, descriptor.TypeId, _imports);
            return value;
        }

        // 外部值必须复制后再附加调用期身份，不能修改调用方持有的 class 或 struct。
        var normalized = descriptor.Normalize(this, value);
        var attached = descriptor.WithValueId(
            normalized, _imports.Attach(descriptor.TypeId, normalized!));
        var collections = _collections ??
            throw new InvalidOperationException("An external Coflow value must be imported into an execution Arena.");
        CoflowEncodedValue Encode(Type type, object? item) =>
            CoflowCollectionEncoding.Encode(type, item, collections, budgetAlreadyCharged: true);
        _imports.SetLastValue(attached!, descriptor.EncodeArena(attached!, Encode));
        return attached;
    }

    internal TEnum ImportEnum<TEnum>(TEnum value) where TEnum : struct, Enum
    {
        _snapshot.ValidateEnum(value);
        return value;
    }

    internal T ImportRuntime<T>(T value)
    {
        if (value is null) throw new CoflowBoundaryException($"A required `{typeof(T)}` value is null.");
        if (!CoflowSchemaRuntimeContext.TryGetTypeCodec(value.GetType(), out var descriptor))
            throw new CoflowBoundaryException($"Type `{value.GetType()}` has no Coflow schema codec.");
        if (!typeof(T).IsAssignableFrom(descriptor.Type))
            throw new CoflowBoundaryException($"Type `{descriptor.Type}` is not assignable to `{typeof(T)}`.");
        return (T)descriptor.Import(this, value);
    }
}

internal abstract class CoflowTypeDescriptor
{
    protected CoflowTypeDescriptor(Type type, CoflowTypeId typeId)
    {
        Type = type;
        TypeId = typeId;
    }

    internal Type Type { get; }
    internal CoflowTypeId TypeId { get; }
    internal abstract object Import(CoflowImportContext context, object value);
    internal abstract CoflowEncodedValue EncodeArena(
        object value,
        Func<Type, object?, CoflowEncodedValue>? encodeArenaValue = null);
    internal abstract CoflowValueId GetValueIdObject(object value);
    internal abstract void CollectValueIds(object value, CoflowValueIdCollector collector);
}

internal sealed class CoflowTypeDescriptor<T> : CoflowTypeDescriptor
{
    internal CoflowTypeDescriptor(
        CoflowTypeId typeId,
        int arenaIntegerCount,
        int arenaFloatCount,
        int arenaReferenceCount,
        Func<T, CoflowValueId> getValueId,
        Func<T, bool> isInitialized,
        Func<T, CoflowValueId, T> withValueId,
        Func<CoflowImportContext, T, T> normalize,
        CoflowStructWriter<T> arenaWriter) : base(typeof(T), typeId)
    {
        GetValueId = getValueId ?? throw new ArgumentNullException(nameof(getValueId));
        IsInitialized = isInitialized ?? throw new ArgumentNullException(nameof(isInitialized));
        WithValueId = withValueId ?? throw new ArgumentNullException(nameof(withValueId));
        Normalize = normalize ?? throw new ArgumentNullException(nameof(normalize));
        ArenaIntegerCount = arenaIntegerCount;
        ArenaFloatCount = arenaFloatCount;
        ArenaReferenceCount = arenaReferenceCount;
        ArenaWriter = arenaWriter ?? throw new ArgumentNullException(nameof(arenaWriter));
    }

    internal Func<T, CoflowValueId> GetValueId { get; }
    internal Func<T, bool> IsInitialized { get; }
    internal Func<T, CoflowValueId, T> WithValueId { get; }
    internal Func<CoflowImportContext, T, T> Normalize { get; }
    private int ArenaIntegerCount { get; }
    private int ArenaFloatCount { get; }
    private int ArenaReferenceCount { get; }
    private CoflowStructWriter<T> ArenaWriter { get; }

    internal override object Import(CoflowImportContext context, object value) =>
        context.ImportRecord(this, (T)value)!;

    internal override CoflowValueId GetValueIdObject(object value) => GetValueId((T)value);

    internal override void CollectValueIds(object value, CoflowValueIdCollector collector)
    {
        var typed = (T)value;
        if (!collector.Add(GetValueId(typed))) return;
        var writer = new CoflowValueWriter(collector);
        ArenaWriter(ref writer, typed);
    }

    internal override CoflowEncodedValue EncodeArena(
        object value,
        Func<Type, object?, CoflowEncodedValue>? encodeArenaValue = null)
    {
        var integers = new long[ArenaIntegerCount];
        var floats = new double[ArenaFloatCount];
        var references = new object?[ArenaReferenceCount];
        var writer = new CoflowValueWriter(integers, floats, references, encodeArenaValue);
        ArenaWriter(ref writer, (T)value);
        if (writer.WrittenIntegerCount != ArenaIntegerCount || writer.WrittenFloatCount != ArenaFloatCount ||
            writer.WrittenReferenceCount != ArenaReferenceCount)
            throw new InvalidOperationException($"Schema Arena codec for `{typeof(T)}` wrote an invalid lane count.");
        return new CoflowEncodedValue(CoflowValueShape.Of(typeof(T)), integers, floats, references);
    }
}

internal static class CoflowImportValue<T>
{
    internal static readonly Func<CoflowImportContext, T, T> Normalize = Build();

    private static Func<CoflowImportContext, T, T> Build()
    {
        var type = typeof(T);
        if (type == typeof(long) || type == typeof(double) || type == typeof(bool) || type == typeof(Unit))
            return static (_, value) => value;
        if (type == typeof(string))
            return static (_, value) => value is null
                ? throw new CoflowBoundaryException("A required string value is null.")
                : value;
        if (type.IsEnum)
            return BuildGeneric(nameof(ImportEnum), type);
        if (CoflowFunctionHandle.IsFunctionType(type))
            return static (_, value) => value;
        if (typeof(Delegate).IsAssignableFrom(type))
            return static (_, _) => throw new CoflowBoundaryException(
                "An external delegate can only enter Coflow through a Host binding.");
        if (type.IsGenericType)
        {
            var definition = type.GetGenericTypeDefinition();
            var arguments = type.GetGenericArguments();
            if (definition == typeof(Option<>)) return BuildGeneric(nameof(ImportOption), arguments);
            if (definition == typeof(Result<,>)) return BuildGeneric(nameof(ImportResult), arguments);
            if (definition == typeof(IReadOnlyList<>)) return BuildGeneric(nameof(ImportList), arguments);
            if (definition == typeof(IReadOnlyDictionary<,>)) return BuildGeneric(nameof(ImportDictionary), arguments);
        }
        if (!type.IsValueType)
            return static (context, value) => context.ImportRuntime(value);
        return static (context, value) =>
        {
            if (CoflowSchemaRuntimeContext.TryGetTypeCodec<T>(out var descriptor))
                return context.ImportRecord(descriptor, value);
            throw new CoflowBoundaryException($"Type `{typeof(T)}` has no Coflow schema codec.");
        };
    }

    private static Func<CoflowImportContext, T, T> BuildGeneric(string name, params Type[] arguments)
    {
        var method = typeof(CoflowImportValue<T>).GetMethod(
            name, System.Reflection.BindingFlags.Static | System.Reflection.BindingFlags.NonPublic)!
            .MakeGenericMethod(arguments);
        return (Func<CoflowImportContext, T, T>)method.CreateDelegate(
            typeof(Func<CoflowImportContext, T, T>));
    }

    private static TEnum ImportEnum<TEnum>(CoflowImportContext context, TEnum value)
        where TEnum : struct, Enum => context.ImportEnum(value);

    private static Option<TValue> ImportOption<TValue>(CoflowImportContext context, Option<TValue> value) =>
        value.HasValue ? Option<TValue>.Some(context.Import(value.Value)) : Option<TValue>.None;

    private static Result<TOk, TError> ImportResult<TOk, TError>(
        CoflowImportContext context, Result<TOk, TError> value) => value.IsOk
            ? Result<TOk, TError>.Ok(context.Import(value.Value))
            : Result<TOk, TError>.Err(context.Import(value.Error));

    private static IReadOnlyList<TValue> ImportList<TValue>(
        CoflowImportContext context, IReadOnlyList<TValue> value)
    {
        if (value is null) throw new CoflowBoundaryException("A required list value is null.");
        context.ReserveCollectionElements(value.Count);
        var copy = new TValue[value.Count];
        for (var index = 0; index < copy.Length; index++) copy[index] = context.Import(value[index]);
        return Array.AsReadOnly(copy);
    }

    private static IReadOnlyDictionary<TKey, TValue> ImportDictionary<TKey, TValue>(
        CoflowImportContext context, IReadOnlyDictionary<TKey, TValue> value) where TKey : notnull
    {
        if (value is null) throw new CoflowBoundaryException("A required dictionary value is null.");
        context.ReserveCollectionElements(value.Count);
        var copy = new Dictionary<TKey, TValue>();
        foreach (var pair in value)
            copy.Add(context.Import(pair.Key), context.Import(pair.Value));
        return new ReadOnlyDictionary<TKey, TValue>(copy);
    }
}

internal sealed class CoflowValueIdCollector
{
    private readonly HashSet<CoflowValueId> _ids = new();

    internal IReadOnlyCollection<CoflowValueId> Ids => _ids;
    internal bool Add(CoflowValueId id) => id.IsValid && _ids.Add(id);
}

internal static class CoflowEscapeValue<T>
{
    internal static readonly bool MayContainRecord = MayContain(typeof(T));
    internal static readonly Action<T, CoflowValueIdCollector> Collect = Build();

    private static bool MayContain(Type type)
    {
        if (CoflowFunctionHandle.IsFunctionType(type)) return true;
        if (!type.IsGenericType)
            return type != typeof(long) && type != typeof(double) && type != typeof(bool) &&
                type != typeof(string) && type != typeof(Unit) && !type.IsEnum;
        return type.GetGenericArguments().Any(MayContain);
    }

    private static Action<T, CoflowValueIdCollector> Build()
    {
        var type = typeof(T);
        if (CoflowFunctionHandle.IsFunctionType(type))
            return static (value, collector) =>
            {
                if (value is ICoflowFunctionHandle function)
                    collector.Add(function.EnvironmentId);
            };
        if (!type.IsGenericType && MayContainRecord)
            return static (value, collector) =>
            {
                if (value is not object instance) return;
                if (CoflowSchemaRuntimeContext.TryGetTypeCodec(instance.GetType(), out var descriptor))
                    descriptor.CollectValueIds(instance, collector);
                else if (CoflowSchemaRuntimeContext.TryGetStructCodec(typeof(T), out var schemaStruct))
                    schemaStruct.CollectValueIds(instance, collector);
            };
        if (!type.IsGenericType) return static (_, _) => { };
        var definition = type.GetGenericTypeDefinition();
        var arguments = type.GetGenericArguments();
        if (definition == typeof(Option<>)) return BuildGeneric(nameof(CollectOption), arguments);
        if (definition == typeof(Result<,>)) return BuildGeneric(nameof(CollectResult), arguments);
        if (definition == typeof(IReadOnlyList<>)) return BuildGeneric(nameof(CollectList), arguments);
        if (definition == typeof(IReadOnlyDictionary<,>)) return BuildGeneric(nameof(CollectDictionary), arguments);
        return static (value, collector) =>
        {
            if (CoflowSchemaRuntimeContext.TryGetStructCodec(typeof(T), out var descriptor))
                descriptor.CollectValueIds(value!, collector);
        };
    }

    private static Action<T, CoflowValueIdCollector> BuildGeneric(string name, params Type[] arguments)
    {
        var method = typeof(CoflowEscapeValue<T>).GetMethod(
            name, System.Reflection.BindingFlags.Static | System.Reflection.BindingFlags.NonPublic)!
            .MakeGenericMethod(arguments);
        return (Action<T, CoflowValueIdCollector>)method.CreateDelegate(
            typeof(Action<T, CoflowValueIdCollector>));
    }

    private static void CollectOption<TValue>(Option<TValue> value, CoflowValueIdCollector collector)
    {
        if (value.HasValue) CoflowEscapeValue<TValue>.Collect(value.Value, collector);
    }

    private static void CollectResult<TOk, TError>(
        Result<TOk, TError> value, CoflowValueIdCollector collector)
    {
        if (value.IsOk) CoflowEscapeValue<TOk>.Collect(value.Value, collector);
        else CoflowEscapeValue<TError>.Collect(value.Error, collector);
    }

    private static void CollectList<TValue>(
        IReadOnlyList<TValue> value, CoflowValueIdCollector collector)
    {
        if (value is null) return;
        foreach (var item in value) CoflowEscapeValue<TValue>.Collect(item, collector);
    }

    private static void CollectDictionary<TKey, TValue>(
        IReadOnlyDictionary<TKey, TValue> value, CoflowValueIdCollector collector) where TKey : notnull
    {
        if (value is null) return;
        foreach (var pair in value)
        {
            CoflowEscapeValue<TKey>.Collect(pair.Key, collector);
            CoflowEscapeValue<TValue>.Collect(pair.Value, collector);
        }
    }
}
}
