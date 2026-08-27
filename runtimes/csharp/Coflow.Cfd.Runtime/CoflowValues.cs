namespace CoflowRuntime;

using System.Diagnostics.CodeAnalysis;

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
