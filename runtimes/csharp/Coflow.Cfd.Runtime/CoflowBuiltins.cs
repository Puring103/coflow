namespace CoflowRuntime;

using System.Reflection;
using System.Text.RegularExpressions;

internal readonly record struct CoflowBuiltin(Type ResultType, Func<object?[], object?> Invoke);

internal static class CoflowBuiltinLibrary
{
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
                Builtin(typeof(bool), StringContains),
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
                Builtin(typeof(long), SumInt64),
            "sum" when listElement == typeof(double) && arguments.Count == 0 =>
                Builtin(typeof(double), SumFloat64),
            "keys" when dictionary is not null && arguments.Count == 0 =>
                Generic(nameof(DictionaryKeys), dictionary, typeof(IReadOnlyList<>).MakeGenericType(dictionary[0])),
            "values" when dictionary is not null && arguments.Count == 0 =>
                Generic(nameof(DictionaryValues), dictionary, typeof(IReadOnlyList<>).MakeGenericType(dictionary[1])),
            "containsKey" when dictionary is not null && Matches(arguments, dictionary[0]) =>
                Generic(nameof(DictionaryContainsKey), dictionary, typeof(bool)),
            "containsValue" when dictionary is not null && Matches(arguments, dictionary[1]) =>
                Generic(nameof(DictionaryContainsValue), dictionary, typeof(bool)),
            "startsWith" when receiver == typeof(string) && Matches(arguments, typeof(string)) =>
                Builtin(typeof(bool), StringStartsWith),
            "endsWith" when receiver == typeof(string) && Matches(arguments, typeof(string)) =>
                Builtin(typeof(bool), StringEndsWith),
            "isBlank" when receiver == typeof(string) && arguments.Count == 0 =>
                Builtin(typeof(bool), StringIsBlank),
            "matches" when receiver == typeof(string) && Matches(arguments, typeof(string)) =>
                throw new ArgumentException("matches must be resolved with its literal pattern"),
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

    internal static void ValidateRegexPattern(string pattern)
    {
        try
        {
            _ = CompileRegex(pattern);
        }
        catch (ArgumentException error)
        {
            throw new ArgumentException($"invalid Rust regex pattern: {error.Message}", error);
        }
    }

    internal static CoflowBuiltin ResolveRegex(string pattern)
    {
        var regex = CompileRegex(pattern);
        return Builtin(typeof(bool), values =>
        {
            var input = (string)values[0]!;
            CoflowVm.ChargeWork((long)input.Length + pattern.Length);
            return regex.IsMatch(input);
        });
    }

    private static Regex CompileRegex(string pattern) =>
        new(TranslateRustRegex(pattern), RegexOptions.CultureInvariant);

    private static CoflowBuiltin Builtin(Type result, Func<object?[], object?> invoke) => new(result, invoke);

    private static string TranslateRustRegex(string pattern)
    {
        var translated = new System.Text.StringBuilder(pattern.Length);
        var escaped = false;
        var characterClass = false;
        for (var index = 0; index < pattern.Length; index++)
        {
            var current = pattern[index];
            if (escaped)
            {
                if (!characterClass && (char.IsDigit(current) || current == 'k'))
                    throw new ArgumentException("backreferences are not supported by Rust regex");
                translated.Append(current);
                escaped = false;
                continue;
            }
            if (current == '\\')
            {
                translated.Append(current);
                escaped = true;
                continue;
            }
            if (current == '[') characterClass = true;
            else if (current == ']' && characterClass) characterClass = false;
            if (!characterClass && current == '(' && index + 2 < pattern.Length && pattern[index + 1] == '?')
            {
                var marker = pattern[index + 2];
                if (marker is '=' or '!' or '<' or '>' or '(')
                    throw new ArgumentException(
                        "lookaround, atomic, conditional, and balancing groups are not supported by Rust regex");
                if (marker == 'P' && index + 3 < pattern.Length && pattern[index + 3] == '<')
                {
                    var nameEnd = pattern.IndexOf('>', index + 4);
                    if (nameEnd < 0) throw new ArgumentException("unterminated named capture group");
                    translated.Append("(?:");
                    index = nameEnd;
                    continue;
                }
                if (marker != ':' && marker is not ('i' or 'm' or 's' or 'x' or '-'))
                    throw new ArgumentException($"group construct `(?{marker}` is not supported by Rust regex");
            }
            translated.Append(current);
        }
        if (escaped) throw new ArgumentException("unterminated escape");
        return translated.ToString();
    }

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
        CoflowVm.ChargeWork(value.Length);
        long count = 0;
        for (var index = 0; index < value.Length; index++, count++)
            if (char.IsHighSurrogate(value[index]) && index + 1 < value.Length &&
                char.IsLowSurrogate(value[index + 1])) index++;
        return count;
    }

    private static object ListCount<T>(object?[] values) => (long)((IReadOnlyList<T>)values[0]!).Count;
    private static object DictionaryCount<TKey, TValue>(object?[] values) where TKey : notnull =>
        (long)((IReadOnlyDictionary<TKey, TValue>)values[0]!).Count;
    private static object ListContains<T>(object?[] values)
    {
        var list = (IReadOnlyList<T>)values[0]!;
        CoflowVm.ChargeWork(list.Count);
        return list.Contains((T)values[1]!);
    }
    private static object DictionaryContainsKey<TKey, TValue>(object?[] values) where TKey : notnull =>
        ((IReadOnlyDictionary<TKey, TValue>)values[0]!).ContainsKey((TKey)values[1]!);
    private static object DictionaryContainsValue<TKey, TValue>(object?[] values) where TKey : notnull
    {
        var dictionary = (IReadOnlyDictionary<TKey, TValue>)values[0]!;
        CoflowVm.ChargeWork(dictionary.Count);
        return dictionary.Values.Contains((TValue)values[1]!);
    }
    private static object ListUnique<T>(object?[] values)
    {
        var list = (IReadOnlyList<T>)values[0]!;
        CoflowVm.ChargeWork(list.Count);
        return list.Distinct().Count() == list.Count;
    }
    private static object ListMin<T>(object?[] values) => AggregateOrdered<T>(values, minimum: true)!;
    private static object ListMax<T>(object?[] values) => AggregateOrdered<T>(values, minimum: false)!;
    private static T AggregateOrdered<T>(object?[] values, bool minimum)
    {
        var list = (IReadOnlyList<T>)values[0]!;
        if (list.Count == 0) throw new InvalidOperationException("aggregate requires a non-empty array");
        CoflowVm.ChargeWork(list.Count);
        var result = list[0];
        var comparer = Comparer<T>.Default;
        for (var index = 1; index < list.Count; index++)
            if (minimum ? comparer.Compare(list[index], result) < 0 : comparer.Compare(list[index], result) > 0)
                result = list[index];
        return result;
    }
    private static object DictionaryKeys<TKey, TValue>(object?[] values) where TKey : notnull
    {
        var dictionary = (IReadOnlyDictionary<TKey, TValue>)values[0]!;
        CoflowVm.ChargeWork(dictionary.Count);
        return Array.AsReadOnly(dictionary.Keys.ToArray());
    }
    private static object DictionaryValues<TKey, TValue>(object?[] values) where TKey : notnull
    {
        var dictionary = (IReadOnlyDictionary<TKey, TValue>)values[0]!;
        CoflowVm.ChargeWork(dictionary.Count);
        return Array.AsReadOnly(dictionary.Values.ToArray());
    }
    private static object ListSorted<T>(object?[] values) => Sorted((IReadOnlyList<T>)values[0]!, strict: false);
    private static object ListStrictlySorted<T>(object?[] values) => Sorted((IReadOnlyList<T>)values[0]!, strict: true);
    private static bool Sorted<T>(IReadOnlyList<T> values, bool strict)
    {
        CoflowVm.ChargeWork(values.Count);
        var comparer = Comparer<T>.Default;
        for (var index = 1; index < values.Count; index++)
        {
            var order = comparer.Compare(values[index - 1], values[index]);
            if (strict ? order >= 0 : order > 0) return false;
        }
        return true;
    }
    private static object ListIntersects<T>(object?[] values)
    {
        ChargeLists<T>(values);
        return new HashSet<T>((IReadOnlyList<T>)values[0]!).Overlaps((IReadOnlyList<T>)values[1]!);
    }
    private static object ListDisjoint<T>(object?[] values) => !(bool)ListIntersects<T>(values);
    private static object ListSubset<T>(object?[] values)
    {
        ChargeLists<T>(values);
        return new HashSet<T>((IReadOnlyList<T>)values[0]!).IsSubsetOf((IReadOnlyList<T>)values[1]!);
    }
    private static object ListSuperset<T>(object?[] values)
    {
        ChargeLists<T>(values);
        return new HashSet<T>((IReadOnlyList<T>)values[0]!).IsSupersetOf((IReadOnlyList<T>)values[1]!);
    }

    private static void ChargeLists<T>(object?[] values) => CoflowVm.ChargeWork(
        (long)((IReadOnlyList<T>)values[0]!).Count + ((IReadOnlyList<T>)values[1]!).Count);

    private static object SumInt64(object?[] values)
    {
        var list = (IReadOnlyList<long>)values[0]!;
        CoflowVm.ChargeWork(list.Count);
        var result = 0L;
        foreach (var item in list) result = checked(result + item);
        return result;
    }

    private static object SumFloat64(object?[] values)
    {
        var list = (IReadOnlyList<double>)values[0]!;
        CoflowVm.ChargeWork(list.Count);
        var result = 0.0;
        foreach (var item in list) result += item;
        return result;
    }

    private static object StringContains(object?[] values)
    {
        var value = (string)values[0]!;
        CoflowVm.ChargeWork(value.Length);
        return value.Contains((string)values[1]!, StringComparison.Ordinal);
    }

    private static object StringStartsWith(object?[] values)
    {
        var value = (string)values[0]!;
        CoflowVm.ChargeWork(Math.Min(value.Length, ((string)values[1]!).Length));
        return value.StartsWith((string)values[1]!, StringComparison.Ordinal);
    }

    private static object StringEndsWith(object?[] values)
    {
        var value = (string)values[0]!;
        CoflowVm.ChargeWork(Math.Min(value.Length, ((string)values[1]!).Length));
        return value.EndsWith((string)values[1]!, StringComparison.Ordinal);
    }

    private static object StringIsBlank(object?[] values)
    {
        var value = (string)values[0]!;
        CoflowVm.ChargeWork(value.Length);
        return string.IsNullOrWhiteSpace(value);
    }

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
