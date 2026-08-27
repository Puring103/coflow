namespace CoflowRuntime;

using System.Collections.Concurrent;
using System.Reflection;
using System.Text.RegularExpressions;

internal readonly record struct CoflowBuiltin(Type ResultType, Func<object?[], object?> Invoke);

internal static class CoflowBuiltinLibrary
{
    private static readonly ConcurrentDictionary<string, Regex> RegexCache = new(StringComparer.Ordinal);
    private static readonly BindingFlags StaticPrivate = BindingFlags.Static | BindingFlags.NonPublic;

    internal static CoflowBuiltin Resolve(string name, Type receiver, IReadOnlyList<Type> arguments)
    {
        var listElement = GenericArgument(receiver, typeof(IReadOnlyList<>));
        var dictionary = GenericArguments(receiver, typeof(IReadOnlyDictionary<,>));
        return name switch
        {
            "len" when arguments.Count == 0 && receiver == typeof(string) =>
                Builtin(typeof(long), values => RuneCount((string)values[0]!)),
            "len" when arguments.Count == 0 && listElement is not null =>
                Generic(nameof(ListCount), new[] { listElement }, typeof(long)),
            "len" when arguments.Count == 0 && dictionary is not null =>
                Generic(nameof(DictionaryCount), dictionary, typeof(long)),
            "contains" when receiver == typeof(string) && Matches(arguments, typeof(string)) =>
                Builtin(typeof(bool), values => ((string)values[0]!).Contains((string)values[1]!, StringComparison.Ordinal)),
            "contains" when listElement is not null && Matches(arguments, listElement) =>
                Generic(nameof(ListContains), new[] { listElement }, typeof(bool)),
            "contains" when dictionary is not null && Matches(arguments, dictionary[0]) =>
                Generic(nameof(DictionaryContainsKey), dictionary, typeof(bool)),
            "isUnique" when listElement is not null && arguments.Count == 0 =>
                Generic(nameof(ListUnique), new[] { listElement }, typeof(bool)),
            "min" when listElement is not null && arguments.Count == 0 && IsOrdered(listElement) =>
                Generic(nameof(ListMin), new[] { listElement }, listElement),
            "max" when listElement is not null && arguments.Count == 0 && IsOrdered(listElement) =>
                Generic(nameof(ListMax), new[] { listElement }, listElement),
            "sum" when listElement == typeof(long) && arguments.Count == 0 =>
                Builtin(typeof(long), values => checked(((IReadOnlyList<long>)values[0]!).Aggregate(0L, checked((sum, item) => sum + item)))),
            "sum" when listElement == typeof(double) && arguments.Count == 0 =>
                Builtin(typeof(double), values => ((IReadOnlyList<double>)values[0]!).Sum()),
            "keys" when dictionary is not null && arguments.Count == 0 =>
                Generic(nameof(DictionaryKeys), dictionary, typeof(IReadOnlyList<>).MakeGenericType(dictionary[0])),
            "values" when dictionary is not null && arguments.Count == 0 =>
                Generic(nameof(DictionaryValues), dictionary, typeof(IReadOnlyList<>).MakeGenericType(dictionary[1])),
            "containsKey" when dictionary is not null && Matches(arguments, dictionary[0]) =>
                Generic(nameof(DictionaryContainsKey), dictionary, typeof(bool)),
            "containsValue" when dictionary is not null && Matches(arguments, dictionary[1]) =>
                Generic(nameof(DictionaryContainsValue), dictionary, typeof(bool)),
            "startsWith" when receiver == typeof(string) && Matches(arguments, typeof(string)) =>
                Builtin(typeof(bool), values => ((string)values[0]!).StartsWith((string)values[1]!, StringComparison.Ordinal)),
            "endsWith" when receiver == typeof(string) && Matches(arguments, typeof(string)) =>
                Builtin(typeof(bool), values => ((string)values[0]!).EndsWith((string)values[1]!, StringComparison.Ordinal)),
            "isBlank" when receiver == typeof(string) && arguments.Count == 0 =>
                Builtin(typeof(bool), values => string.IsNullOrWhiteSpace((string)values[0]!)),
            "matches" when receiver == typeof(string) && Matches(arguments, typeof(string)) =>
                Builtin(typeof(bool), values => RegexCache.GetOrAdd((string)values[1]!,
                    static pattern => new Regex(pattern, RegexOptions.CultureInvariant)).IsMatch((string)values[0]!)),
            "abs" when receiver == typeof(long) && arguments.Count == 0 =>
                Builtin(typeof(long), values => checked(Math.Abs((long)values[0]!))),
            "abs" when receiver == typeof(double) && arguments.Count == 0 =>
                Builtin(typeof(double), values => Math.Abs((double)values[0]!)),
            "sqrt" when receiver == typeof(double) && arguments.Count == 0 =>
                Builtin(typeof(double), values => Math.Sqrt((double)values[0]!)),
            "isFinite" when receiver == typeof(double) && arguments.Count == 0 =>
                Builtin(typeof(bool), values => double.IsFinite((double)values[0]!)),
            "approxEqual" when receiver == typeof(double) && Matches(arguments, typeof(double), typeof(double)) =>
                Builtin(typeof(bool), ApproxEqual),
            "isSorted" when listElement is not null && arguments.Count == 0 && IsOrdered(listElement) =>
                Generic(nameof(ListSorted), new[] { listElement }, typeof(bool)),
            "isStrictlySorted" when listElement is not null && arguments.Count == 0 && IsOrdered(listElement) =>
                Generic(nameof(ListStrictlySorted), new[] { listElement }, typeof(bool)),
            "intersects" when listElement is not null && Matches(arguments, receiver) =>
                Generic(nameof(ListIntersects), new[] { listElement }, typeof(bool)),
            "isDisjoint" when listElement is not null && Matches(arguments, receiver) =>
                Generic(nameof(ListDisjoint), new[] { listElement }, typeof(bool)),
            "isSubsetOf" when listElement is not null && Matches(arguments, receiver) =>
                Generic(nameof(ListSubset), new[] { listElement }, typeof(bool)),
            "isSupersetOf" when listElement is not null && Matches(arguments, receiver) =>
                Generic(nameof(ListSuperset), new[] { listElement }, typeof(bool)),
            _ => throw new ArgumentException($"built-in method `{name}` is not available for `{Display(receiver)}` with the supplied arguments"),
        };
    }

    private static CoflowBuiltin Builtin(Type result, Func<object?[], object?> invoke) => new(result, invoke);

    private static CoflowBuiltin Generic(string method, Type[] types, Type result)
    {
        var info = typeof(CoflowBuiltinLibrary).GetMethod(method, StaticPrivate)!.MakeGenericMethod(types);
        return new CoflowBuiltin(result,
            (Func<object?[], object?>)info.CreateDelegate(typeof(Func<object?[], object?>)));
    }

    private static bool Matches(IReadOnlyList<Type> actual, params Type[] expected) => actual.SequenceEqual(expected);
    private static Type? GenericArgument(Type type, Type definition) =>
        type.IsGenericType && type.GetGenericTypeDefinition() == definition ? type.GetGenericArguments()[0] : null;
    private static Type[]? GenericArguments(Type type, Type definition) =>
        type.IsGenericType && type.GetGenericTypeDefinition() == definition ? type.GetGenericArguments() : null;
    private static bool IsOrdered(Type type) => type == typeof(long) || type == typeof(double) ||
        type == typeof(string) || type.IsEnum;
    private static string Display(Type type) => type.Name;

    private static long RuneCount(string value)
    {
        long count = 0;
        for (var index = 0; index < value.Length; index++, count++)
            if (char.IsHighSurrogate(value[index]) && index + 1 < value.Length &&
                char.IsLowSurrogate(value[index + 1])) index++;
        return count;
    }

    private static object ListCount<T>(object?[] values) => (long)((IReadOnlyList<T>)values[0]!).Count;
    private static object DictionaryCount<TKey, TValue>(object?[] values) where TKey : notnull =>
        (long)((IReadOnlyDictionary<TKey, TValue>)values[0]!).Count;
    private static object ListContains<T>(object?[] values) =>
        ((IReadOnlyList<T>)values[0]!).Contains((T)values[1]!);
    private static object DictionaryContainsKey<TKey, TValue>(object?[] values) where TKey : notnull =>
        ((IReadOnlyDictionary<TKey, TValue>)values[0]!).ContainsKey((TKey)values[1]!);
    private static object DictionaryContainsValue<TKey, TValue>(object?[] values) where TKey : notnull =>
        ((IReadOnlyDictionary<TKey, TValue>)values[0]!).Values.Contains((TValue)values[1]!);
    private static object ListUnique<T>(object?[] values) =>
        ((IReadOnlyList<T>)values[0]!).Distinct().Count() == ((IReadOnlyList<T>)values[0]!).Count;
    private static object ListMin<T>(object?[] values) => AggregateOrdered<T>(values, minimum: true)!;
    private static object ListMax<T>(object?[] values) => AggregateOrdered<T>(values, minimum: false)!;
    private static T AggregateOrdered<T>(object?[] values, bool minimum)
    {
        var list = (IReadOnlyList<T>)values[0]!;
        if (list.Count == 0) throw new InvalidOperationException("aggregate requires a non-empty array");
        var result = list[0];
        var comparer = Comparer<T>.Default;
        for (var index = 1; index < list.Count; index++)
            if (minimum ? comparer.Compare(list[index], result) < 0 : comparer.Compare(list[index], result) > 0)
                result = list[index];
        return result;
    }
    private static object DictionaryKeys<TKey, TValue>(object?[] values) where TKey : notnull =>
        Array.AsReadOnly(((IReadOnlyDictionary<TKey, TValue>)values[0]!).Keys.ToArray());
    private static object DictionaryValues<TKey, TValue>(object?[] values) where TKey : notnull =>
        Array.AsReadOnly(((IReadOnlyDictionary<TKey, TValue>)values[0]!).Values.ToArray());
    private static object ListSorted<T>(object?[] values) => Sorted((IReadOnlyList<T>)values[0]!, strict: false);
    private static object ListStrictlySorted<T>(object?[] values) => Sorted((IReadOnlyList<T>)values[0]!, strict: true);
    private static bool Sorted<T>(IReadOnlyList<T> values, bool strict)
    {
        var comparer = Comparer<T>.Default;
        for (var index = 1; index < values.Count; index++)
        {
            var order = comparer.Compare(values[index - 1], values[index]);
            if (strict ? order >= 0 : order > 0) return false;
        }
        return true;
    }
    private static object ListIntersects<T>(object?[] values) =>
        new HashSet<T>((IReadOnlyList<T>)values[0]!).Overlaps((IReadOnlyList<T>)values[1]!);
    private static object ListDisjoint<T>(object?[] values) => !(bool)ListIntersects<T>(values);
    private static object ListSubset<T>(object?[] values) =>
        new HashSet<T>((IReadOnlyList<T>)values[0]!).IsSubsetOf((IReadOnlyList<T>)values[1]!);
    private static object ListSuperset<T>(object?[] values) =>
        new HashSet<T>((IReadOnlyList<T>)values[0]!).IsSupersetOf((IReadOnlyList<T>)values[1]!);

    private static object ApproxEqual(object?[] values)
    {
        var left = (double)values[0]!;
        var right = (double)values[1]!;
        var epsilon = (double)values[2]!;
        if (!double.IsFinite(epsilon) || epsilon < 0)
            throw new InvalidOperationException("approxEqual epsilon must be finite and non-negative");
        return Math.Abs(left - right) <= epsilon;
    }
}
