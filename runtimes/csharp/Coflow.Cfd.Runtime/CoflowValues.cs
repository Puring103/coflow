namespace CoflowRuntime;

using System.Diagnostics.CodeAnalysis;
using System.Collections;

public readonly struct Unit : IEquatable<Unit>
{
    public static Unit Value => default;

    public bool Equals(Unit other) => true;
    public override bool Equals(object? obj) => obj is Unit;
    public override int GetHashCode() => 0;
    public override string ToString() => "()";
}

public readonly struct Option<T> : IEquatable<Option<T>>
{
    private readonly T? _value;

    private Option(T value)
    {
        _value = value;
        HasValue = true;
    }

    public bool HasValue { get; }

    public T Value => HasValue
        ? _value!
        : throw new InvalidOperationException("Option contains no value.");

    public static Option<T> None => default;
    public static Option<T> Some(T value) => value is null
        ? throw new ArgumentNullException(nameof(value))
        : new Option<T>(value);

    public bool TryGetValue([MaybeNullWhen(false)] out T value)
    {
        value = _value!;
        return HasValue;
    }

    public bool Equals(Option<T> other) => HasValue == other.HasValue &&
        (!HasValue || EqualityComparer<T>.Default.Equals(_value!, other._value!));

    public override bool Equals(object? obj) => obj is Option<T> other && Equals(other);
    public override int GetHashCode() => HasValue ? EqualityComparer<T>.Default.GetHashCode(_value!) : 0;
    public override string ToString() => HasValue ? $"Some({_value})" : "None";
}

public readonly struct Result<T, TError> : IEquatable<Result<T, TError>>
{
    private readonly T? _value;
    private readonly TError? _error;

    private Result(bool isOk, T? value, TError? error)
    {
        _value = value;
        _error = error;
        IsOk = isOk;
    }

    public bool IsOk { get; }
    public bool IsErr => !IsOk;
    public T Value => IsOk ? _value! : throw new InvalidOperationException("Result contains an error.");
    public TError Error => IsErr ? _error! : throw new InvalidOperationException("Result contains a value.");

    public static Result<T, TError> Ok(T value) => value is null
        ? throw new ArgumentNullException(nameof(value))
        : new Result<T, TError>(true, value, default);

    public static Result<T, TError> Err(TError error) => error is null
        ? throw new ArgumentNullException(nameof(error))
        : new Result<T, TError>(false, default, error);

    public bool Equals(Result<T, TError> other) => IsOk == other.IsOk &&
        (IsOk
            ? EqualityComparer<T>.Default.Equals(_value!, other._value!)
            : EqualityComparer<TError>.Default.Equals(_error!, other._error!));

    public override bool Equals(object? obj) => obj is Result<T, TError> other && Equals(other);
    public override int GetHashCode() => IsOk
        ? HashCode.Combine(true, EqualityComparer<T>.Default.GetHashCode(_value!))
        : HashCode.Combine(false, EqualityComparer<TError>.Default.GetHashCode(_error!));
}

internal static class CoflowValueEquality
{
    public static bool Equal(
        object? left,
        object? right,
        Type type,
        IReadOnlyList<ICoflowTypeMetadata> metadata)
    {
        if (type == typeof(double) && left is double leftFloat && right is double rightFloat)
            return leftFloat == rightFloat;
        if (ReferenceEquals(left, right)) return true;
        if (left is null || right is null) return false;
        if (type.IsGenericType)
        {
            var definition = type.GetGenericTypeDefinition();
            var arguments = type.GetGenericArguments();
            if (definition == typeof(Option<>))
            {
                var hasValue = (bool)type.GetProperty(nameof(Option<int>.HasValue))!.GetValue(left)!;
                if (hasValue != (bool)type.GetProperty(nameof(Option<int>.HasValue))!.GetValue(right)!)
                    return false;
                return !hasValue || Equal(
                    type.GetProperty(nameof(Option<int>.Value))!.GetValue(left),
                    type.GetProperty(nameof(Option<int>.Value))!.GetValue(right),
                    arguments[0], metadata);
            }
            if (definition == typeof(Result<,>))
            {
                var isOk = (bool)type.GetProperty(nameof(Result<int, int>.IsOk))!.GetValue(left)!;
                if (isOk != (bool)type.GetProperty(nameof(Result<int, int>.IsOk))!.GetValue(right)!)
                    return false;
                var branch = isOk ? 0 : 1;
                var property = type.GetProperty(isOk
                    ? nameof(Result<int, int>.Value) : nameof(Result<int, int>.Error))!;
                return Equal(property.GetValue(left), property.GetValue(right), arguments[branch], metadata);
            }
            if (definition == typeof(IReadOnlyList<>))
                return SequenceEqual((IEnumerable)left, (IEnumerable)right, arguments[0], metadata);
            if (definition == typeof(IReadOnlyDictionary<,>))
                return DictionaryEqual((IEnumerable)left, (IEnumerable)right, arguments, metadata);
        }

        var generated = metadata.FirstOrDefault(item => item.RuntimeType == left.GetType());
        if (generated is not null && generated.RuntimeType == right.GetType())
        {
            var leftIsRecord = CoflowGenerationStorage.TryLocation(left, out _, out _);
            var rightIsRecord = CoflowGenerationStorage.TryLocation(right, out _, out _);
            if (leftIsRecord || rightIsRecord) return false;
            return generated.FieldNames.All(field => Equal(
                generated.GetField(left, field), generated.GetField(right, field),
                generated.GetFieldType(field), metadata));
        }
        return left.Equals(right);
    }

    private static bool SequenceEqual(
        IEnumerable left,
        IEnumerable right,
        Type elementType,
        IReadOnlyList<ICoflowTypeMetadata> metadata)
    {
        var leftItems = left.GetEnumerator();
        var rightItems = right.GetEnumerator();
        while (true)
        {
            var hasLeft = leftItems.MoveNext();
            var hasRight = rightItems.MoveNext();
            if (hasLeft != hasRight) return false;
            if (!hasLeft) return true;
            if (!Equal(leftItems.Current, rightItems.Current, elementType, metadata)) return false;
        }
    }

    private static bool DictionaryEqual(
        IEnumerable left,
        IEnumerable right,
        Type[] arguments,
        IReadOnlyList<ICoflowTypeMetadata> metadata)
    {
        var leftItems = left.GetEnumerator();
        var rightItems = right.GetEnumerator();
        while (true)
        {
            var hasLeft = leftItems.MoveNext();
            var hasRight = rightItems.MoveNext();
            if (hasLeft != hasRight) return false;
            if (!hasLeft) return true;
            var leftEntry = leftItems.Current!;
            var rightEntry = rightItems.Current!;
            var entryType = leftEntry.GetType();
            if (!Equal(entryType.GetProperty("Key")!.GetValue(leftEntry),
                    entryType.GetProperty("Key")!.GetValue(rightEntry), arguments[0], metadata) ||
                !Equal(entryType.GetProperty("Value")!.GetValue(leftEntry),
                    entryType.GetProperty("Value")!.GetValue(rightEntry), arguments[1], metadata))
                return false;
        }
    }
}
