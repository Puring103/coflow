namespace Coflow.Runtime.CompilerServices;

using System.Text.RegularExpressions;

internal enum CoflowBuiltinKind : byte
{
    Native,
    CollectionCount,
    DictionaryKeys,
    DictionaryValues,
    CollectionContains,
    DictionaryContainsKey,
    DictionaryContainsValue,
    CollectionUnique,
    CollectionMin,
    CollectionMax,
    CollectionSumInteger,
    CollectionSumFloat,
    CollectionSorted,
    CollectionStrictlySorted,
    CollectionIntersects,
    CollectionDisjoint,
    CollectionSubset,
    CollectionSuperset,
}

internal readonly record struct CoflowBuiltin(
    Type ResultType,
    CoflowBuiltinKind Kind,
    CoflowNativeCall? Call = null)
{
    internal bool HasCollectionArgument => Kind is
        CoflowBuiltinKind.CollectionContains or
        CoflowBuiltinKind.DictionaryContainsKey or
        CoflowBuiltinKind.DictionaryContainsValue or
        CoflowBuiltinKind.CollectionIntersects or
        CoflowBuiltinKind.CollectionDisjoint or
        CoflowBuiltinKind.CollectionSubset or
        CoflowBuiltinKind.CollectionSuperset;
}

internal static class CoflowBuiltinLibrary
{
    internal static CoflowBuiltin Resolve(string name, Type receiver, IReadOnlyList<Type> arguments)
    {
        var element = GenericArgument(receiver, typeof(IReadOnlyList<>));
        var dictionary = GenericArguments(receiver, typeof(IReadOnlyDictionary<,>));
        return name switch
        {
            "len" when arguments.Count == 0 && receiver == typeof(string) =>
                Builtin((Func<string, long>)RuneCount),
            "len" when arguments.Count == 0 && element is not null =>
                Collection(typeof(long), CoflowBuiltinKind.CollectionCount),
            "len" when arguments.Count == 0 && dictionary is not null =>
                Collection(typeof(long), CoflowBuiltinKind.CollectionCount),
            "contains" when receiver == typeof(string) && Matches(arguments, typeof(string)) =>
                Builtin((Func<string, string, bool>)StringContains),
            "contains" when element is not null && Matches(arguments, element) =>
                Collection(typeof(bool), CoflowBuiltinKind.CollectionContains),
            "contains" or "containsKey" when dictionary is not null && Matches(arguments, dictionary[0]) =>
                Collection(typeof(bool), CoflowBuiltinKind.DictionaryContainsKey),
            "containsValue" when dictionary is not null && Matches(arguments, dictionary[1]) =>
                Collection(typeof(bool), CoflowBuiltinKind.DictionaryContainsValue),
            "isUnique" when element is not null && arguments.Count == 0 =>
                Collection(typeof(bool), CoflowBuiltinKind.CollectionUnique),
            "min" when element is not null && arguments.Count == 0 && IsOrdered(element) =>
                Collection(element, CoflowBuiltinKind.CollectionMin),
            "max" when element is not null && arguments.Count == 0 && IsOrdered(element) =>
                Collection(element, CoflowBuiltinKind.CollectionMax),
            "sum" when element == typeof(long) && arguments.Count == 0 =>
                Collection(typeof(long), CoflowBuiltinKind.CollectionSumInteger),
            "sum" when element == typeof(double) && arguments.Count == 0 =>
                Collection(typeof(double), CoflowBuiltinKind.CollectionSumFloat),
            "keys" when dictionary is not null && arguments.Count == 0 =>
                Collection(typeof(IReadOnlyList<>).MakeGenericType(dictionary[0]),
                    CoflowBuiltinKind.DictionaryKeys),
            "values" when dictionary is not null && arguments.Count == 0 =>
                Collection(typeof(IReadOnlyList<>).MakeGenericType(dictionary[1]),
                    CoflowBuiltinKind.DictionaryValues),
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
                Collection(typeof(bool), CoflowBuiltinKind.CollectionSorted),
            "isStrictlySorted" when element is not null && arguments.Count == 0 && IsOrdered(element) =>
                Collection(typeof(bool), CoflowBuiltinKind.CollectionStrictlySorted),
            "intersects" when element is not null && Matches(arguments, receiver) =>
                Collection(typeof(bool), CoflowBuiltinKind.CollectionIntersects),
            "isDisjoint" when element is not null && Matches(arguments, receiver) =>
                Collection(typeof(bool), CoflowBuiltinKind.CollectionDisjoint),
            "isSubsetOf" when element is not null && Matches(arguments, receiver) =>
                Collection(typeof(bool), CoflowBuiltinKind.CollectionSubset),
            "isSupersetOf" when element is not null && Matches(arguments, receiver) =>
                Collection(typeof(bool), CoflowBuiltinKind.CollectionSuperset),
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

    private static CoflowBuiltin Builtin<T1, TResult>(Func<T1, TResult> implementation) =>
        new(typeof(TResult), CoflowBuiltinKind.Native,
            new CoflowNativeCall(new[] { typeof(T1) }, typeof(TResult),
                frame => frame.Write(implementation(frame.Read<T1>(0)))));

    private static CoflowBuiltin Builtin<T1, T2, TResult>(Func<T1, T2, TResult> implementation) =>
        new(typeof(TResult), CoflowBuiltinKind.Native,
            new CoflowNativeCall(new[] { typeof(T1), typeof(T2) }, typeof(TResult),
                frame => frame.Write(implementation(frame.Read<T1>(0), frame.Read<T2>(1)))));

    private static CoflowBuiltin Builtin<T1, T2, T3, TResult>(Func<T1, T2, T3, TResult> implementation) =>
        new(typeof(TResult), CoflowBuiltinKind.Native,
            new CoflowNativeCall(new[] { typeof(T1), typeof(T2), typeof(T3) }, typeof(TResult),
                frame => frame.Write(implementation(
                    frame.Read<T1>(0), frame.Read<T2>(1), frame.Read<T3>(2)))));

    private static CoflowBuiltin Collection(Type resultType, CoflowBuiltinKind kind) =>
        new(resultType, kind);

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
