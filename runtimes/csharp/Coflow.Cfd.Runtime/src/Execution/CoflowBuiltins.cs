namespace CoflowRuntime.Generated;

using System.Reflection;
using System.Text.RegularExpressions;

internal readonly record struct CoflowBuiltin(Type ResultType, Delegate Invoke);

internal static class CoflowBuiltinLibrary
{
    private static readonly BindingFlags StaticPrivate = BindingFlags.Static | BindingFlags.NonPublic;

    internal static CoflowBuiltin Resolve(string name, Type receiver, IReadOnlyList<Type> arguments)
    {
        var element = GenericArgument(receiver, typeof(IReadOnlyList<>));
        var dictionary = GenericArguments(receiver, typeof(IReadOnlyDictionary<,>));
        return name switch
        {
            "len" when arguments.Count == 0 && receiver == typeof(string) =>
                Builtin((Func<string, long>)RuneCount),
            "len" when arguments.Count == 0 && element is not null =>
                Generic(nameof(ListCount), new[] { element }),
            "len" when arguments.Count == 0 && dictionary is not null =>
                Generic(nameof(DictionaryCount), dictionary),
            "contains" when receiver == typeof(string) && Matches(arguments, typeof(string)) =>
                Builtin((Func<string, string, bool>)StringContains),
            "contains" when element is not null && Matches(arguments, element) =>
                Generic(nameof(ListContains), new[] { element }),
            "contains" or "containsKey" when dictionary is not null && Matches(arguments, dictionary[0]) =>
                Generic(nameof(DictionaryContainsKey), dictionary),
            "containsValue" when dictionary is not null && Matches(arguments, dictionary[1]) =>
                Generic(nameof(DictionaryContainsValue), dictionary),
            "isUnique" when element is not null && arguments.Count == 0 =>
                Generic(nameof(ListUnique), new[] { element }),
            "min" when element is not null && arguments.Count == 0 && IsOrdered(element) =>
                Generic(nameof(ListMin), new[] { element }),
            "max" when element is not null && arguments.Count == 0 && IsOrdered(element) =>
                Generic(nameof(ListMax), new[] { element }),
            "sum" when element == typeof(long) && arguments.Count == 0 =>
                Builtin((Func<IReadOnlyList<long>, long>)SumInt64),
            "sum" when element == typeof(double) && arguments.Count == 0 =>
                Builtin((Func<IReadOnlyList<double>, double>)SumFloat64),
            "keys" when dictionary is not null && arguments.Count == 0 =>
                Generic(nameof(DictionaryKeys), dictionary),
            "values" when dictionary is not null && arguments.Count == 0 =>
                Generic(nameof(DictionaryValues), dictionary),
            "startsWith" when receiver == typeof(string) && Matches(arguments, typeof(string)) =>
                Builtin((Func<string, string, bool>)StringStartsWith),
            "endsWith" when receiver == typeof(string) && Matches(arguments, typeof(string)) =>
                Builtin((Func<string, string, bool>)StringEndsWith),
            "isBlank" when receiver == typeof(string) && arguments.Count == 0 =>
                Builtin((Func<string, bool>)StringIsBlank),
            "matches" when receiver == typeof(string) && Matches(arguments, typeof(string)) =>
                throw new ArgumentException("matches must be resolved with its literal pattern"),
            "abs" when receiver == typeof(long) && arguments.Count == 0 =>
                Builtin((Func<long, long>)(static value => checked(Math.Abs(value)))),
            "abs" when receiver == typeof(double) && arguments.Count == 0 =>
                Builtin((Func<double, double>)Math.Abs),
            "sqrt" when receiver == typeof(double) && arguments.Count == 0 =>
                Builtin((Func<double, double>)Math.Sqrt),
            "isFinite" when receiver == typeof(double) && arguments.Count == 0 =>
                Builtin((Func<double, bool>)double.IsFinite),
            "approxEqual" when receiver == typeof(double) && Matches(arguments, typeof(double), typeof(double)) =>
                Builtin((Func<double, double, double, bool>)ApproxEqual),
            "isSorted" when element is not null && arguments.Count == 0 && IsOrdered(element) =>
                Generic(nameof(ListSorted), new[] { element }),
            "isStrictlySorted" when element is not null && arguments.Count == 0 && IsOrdered(element) =>
                Generic(nameof(ListStrictlySorted), new[] { element }),
            "intersects" when element is not null && Matches(arguments, receiver) =>
                Generic(nameof(ListIntersects), new[] { element }),
            "isDisjoint" when element is not null && Matches(arguments, receiver) =>
                Generic(nameof(ListDisjoint), new[] { element }),
            "isSubsetOf" when element is not null && Matches(arguments, receiver) =>
                Generic(nameof(ListSubset), new[] { element }),
            "isSupersetOf" when element is not null && Matches(arguments, receiver) =>
                Generic(nameof(ListSuperset), new[] { element }),
            _ => throw new ArgumentException(
                $"built-in method `{name}` is not available for `{receiver.Name}` with the supplied arguments"),
        };
    }

    internal static void ValidateRegexPattern(string pattern)
    {
        try { _ = CompileRegex(pattern); }
        catch (ArgumentException error)
        {
            throw new ArgumentException($"invalid regular expression: {error.Message}", error);
        }
    }

    internal static CoflowBuiltin ResolveRegex(string pattern)
    {
        var regex = CompileRegex(pattern);
        return Builtin((Func<string, bool>)regex.IsMatch);
    }

    private static CoflowBuiltin Builtin(Delegate implementation) =>
        new(implementation.GetType().GetMethod("Invoke")!.ReturnType, implementation);

    private static CoflowBuiltin Generic(string method, Type[] types)
    {
        var info = typeof(CoflowBuiltinLibrary).GetMethod(method, StaticPrivate)!.MakeGenericMethod(types);
        var signature = info.GetParameters().Select(parameter => parameter.ParameterType)
            .Append(info.ReturnType).ToArray();
        return new CoflowBuiltin(info.ReturnType,
            info.CreateDelegate(System.Linq.Expressions.Expression.GetDelegateType(signature)));
    }

    private static Regex CompileRegex(string pattern) => new(pattern, RegexOptions.CultureInvariant);

    private static bool Matches(IReadOnlyList<Type> actual, params Type[] expected) => actual.SequenceEqual(expected);
    private static Type? GenericArgument(Type type, Type definition) =>
        type.IsGenericType && type.GetGenericTypeDefinition() == definition ? type.GetGenericArguments()[0] : null;
    private static Type[]? GenericArguments(Type type, Type definition) =>
        type.IsGenericType && type.GetGenericTypeDefinition() == definition ? type.GetGenericArguments() : null;
    private static bool IsOrdered(Type type) => type == typeof(long) || type == typeof(double) ||
        type == typeof(string) || type.IsEnum;

    private static long RuneCount(string value)
    {
        long count = 0;
        for (var index = 0; index < value.Length; index++, count++)
            if (char.IsHighSurrogate(value[index]) && index + 1 < value.Length &&
                char.IsLowSurrogate(value[index + 1])) index++;
        return count;
    }

    private static long ListCount<T>(IReadOnlyList<T> values) => values.Count;
    private static long DictionaryCount<TKey, TValue>(IReadOnlyDictionary<TKey, TValue> values) where TKey : notnull => values.Count;
    private static bool ListContains<T>(IReadOnlyList<T> values, T item) => values.Contains(item);
    private static bool DictionaryContainsKey<TKey, TValue>(IReadOnlyDictionary<TKey, TValue> values, TKey key) where TKey : notnull => values.ContainsKey(key);
    private static bool DictionaryContainsValue<TKey, TValue>(IReadOnlyDictionary<TKey, TValue> values, TValue item) where TKey : notnull =>
        values.Values.Contains(item);
    private static bool ListUnique<T>(IReadOnlyList<T> values) => values.Distinct().Count() == values.Count;
    private static T ListMin<T>(IReadOnlyList<T> values) => AggregateOrdered(values, minimum: true);
    private static T ListMax<T>(IReadOnlyList<T> values) => AggregateOrdered(values, minimum: false);
    private static T AggregateOrdered<T>(IReadOnlyList<T> values, bool minimum)
    {
        if (values.Count == 0) throw new InvalidOperationException("aggregate requires a non-empty array");
        var result = values[0];
        var comparer = Comparer<T>.Default;
        for (var index = 1; index < values.Count; index++)
            if (minimum ? comparer.Compare(values[index], result) < 0 : comparer.Compare(values[index], result) > 0)
                result = values[index];
        return result;
    }
    private static IReadOnlyList<TKey> DictionaryKeys<TKey, TValue>(IReadOnlyDictionary<TKey, TValue> values) where TKey : notnull =>
        Array.AsReadOnly(values.Keys.ToArray());
    private static IReadOnlyList<TValue> DictionaryValues<TKey, TValue>(IReadOnlyDictionary<TKey, TValue> values) where TKey : notnull =>
        Array.AsReadOnly(values.Values.ToArray());
    private static bool ListSorted<T>(IReadOnlyList<T> values) => Sorted(values, strict: false);
    private static bool ListStrictlySorted<T>(IReadOnlyList<T> values) => Sorted(values, strict: true);
    private static bool Sorted<T>(IReadOnlyList<T> values, bool strict)
    {
        var comparer = Comparer<T>.Default;
        for (var index = 1; index < values.Count; index++)
        { var order = comparer.Compare(values[index - 1], values[index]); if (strict ? order >= 0 : order > 0) return false; }
        return true;
    }
    private static bool ListIntersects<T>(IReadOnlyList<T> left, IReadOnlyList<T> right) =>
        new HashSet<T>(left).Overlaps(right);
    private static bool ListDisjoint<T>(IReadOnlyList<T> left, IReadOnlyList<T> right) => !ListIntersects(left, right);
    private static bool ListSubset<T>(IReadOnlyList<T> left, IReadOnlyList<T> right) =>
        new HashSet<T>(left).IsSubsetOf(right);
    private static bool ListSuperset<T>(IReadOnlyList<T> left, IReadOnlyList<T> right) =>
        new HashSet<T>(left).IsSupersetOf(right);
    private static long SumInt64(IReadOnlyList<long> values)
    { var result = 0L; foreach (var item in values) result = checked(result + item); return result; }
    private static double SumFloat64(IReadOnlyList<double> values)
    { var result = 0.0; foreach (var item in values) result += item; return result; }
    private static bool StringContains(string value, string item) => value.Contains(item, StringComparison.Ordinal);
    private static bool StringStartsWith(string value, string item) => value.StartsWith(item, StringComparison.Ordinal);
    private static bool StringEndsWith(string value, string item) => value.EndsWith(item, StringComparison.Ordinal);
    private static bool StringIsBlank(string value) => string.IsNullOrWhiteSpace(value);
    private static bool ApproxEqual(double left, double right, double epsilon)
    {
        if (!double.IsFinite(epsilon) || epsilon < 0)
            throw new InvalidOperationException("approxEqual epsilon must be finite and non-negative");
        return Math.Abs(left - right) <= epsilon;
    }
}
