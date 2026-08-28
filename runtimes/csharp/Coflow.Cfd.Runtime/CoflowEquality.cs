namespace CoflowRuntime;

using System.Linq.Expressions;

internal static class CoflowEquality
{
    internal static Delegate Create(Type type, IReadOnlyDictionary<string, ICoflowTypeMetadata> metadata)
    {
        var left = Expression.Parameter(type, "left");
        var right = Expression.Parameter(type, "right");
        return Expression.Lambda(Expression.GetFuncType(type, type, typeof(bool)),
            Equal(type, left, right, metadata), left, right).Compile();
    }

    private static Expression Equal(
        Type type,
        Expression left,
        Expression right,
        IReadOnlyDictionary<string, ICoflowTypeMetadata> metadata)
    {
        if (type.IsGenericType)
        {
            var definition = type.GetGenericTypeDefinition();
            var arguments = type.GetGenericArguments();
            if (definition == typeof(Option<>))
            {
                var leftActive = Expression.Property(left, nameof(Option<int>.HasValue));
                var rightActive = Expression.Property(right, nameof(Option<int>.HasValue));
                return Expression.AndAlso(Expression.Equal(leftActive, rightActive),
                    Expression.OrElse(Expression.Not(leftActive),
                        Equal(arguments[0],
                            Expression.Property(left, nameof(Option<int>.Value)),
                            Expression.Property(right, nameof(Option<int>.Value)), metadata)));
            }
            if (definition == typeof(Result<,>))
            {
                var leftOk = Expression.Property(left, nameof(Result<int, int>.IsOk));
                var rightOk = Expression.Property(right, nameof(Result<int, int>.IsOk));
                return Expression.AndAlso(Expression.Equal(leftOk, rightOk),
                    Expression.Condition(leftOk,
                        Equal(arguments[0],
                            Expression.Property(left, nameof(Result<int, int>.Value)),
                            Expression.Property(right, nameof(Result<int, int>.Value)), metadata),
                        Equal(arguments[1],
                            Expression.Property(left, nameof(Result<int, int>.Error)),
                            Expression.Property(right, nameof(Result<int, int>.Error)), metadata)));
            }
            if (definition == typeof(IReadOnlyList<>))
                return Expression.Call(typeof(CoflowEquality), nameof(ListEqual), arguments,
                    left, right, Expression.Constant(Create(arguments[0], metadata)));
            if (definition == typeof(IReadOnlyDictionary<,>))
                return Expression.Call(typeof(CoflowEquality), nameof(DictionaryEqual), arguments,
                    left, right, Expression.Constant(Create(arguments[0], metadata)),
                    Expression.Constant(Create(arguments[1], metadata)));
        }

        if (type == typeof(string)) return Expression.Equal(left, right);
        var generated = metadata.Values.FirstOrDefault(item => item.RuntimeType == type);
        if (generated is null || generated is ICoflowRecordMetadata)
            return type.IsValueType ? Expression.Equal(left, right) : Expression.ReferenceEqual(left, right);
        Expression result = Expression.Constant(true);
        foreach (var field in generated.FieldNames)
        {
            var fieldType = generated.GetFieldType(field);
            var reader = generated.GetFieldReader(field);
            var delegateType = Expression.GetFuncType(type, fieldType);
            var leftField = Expression.Invoke(Expression.Convert(Expression.Constant(reader), delegateType), left);
            var rightField = Expression.Invoke(Expression.Convert(Expression.Constant(reader), delegateType), right);
            result = Expression.AndAlso(result, Equal(fieldType, leftField, rightField, metadata));
        }
        return result;
    }

    private static bool ListEqual<T>(IReadOnlyList<T> left, IReadOnlyList<T> right, Delegate equality)
    {
        if (left.Count != right.Count) return false;
        var equal = (Func<T, T, bool>)equality;
        for (var index = 0; index < left.Count; index++)
            if (!equal(left[index], right[index])) return false;
        return true;
    }

    private static bool DictionaryEqual<TKey, TValue>(
        IReadOnlyDictionary<TKey, TValue> left,
        IReadOnlyDictionary<TKey, TValue> right,
        Delegate keyEquality,
        Delegate valueEquality) where TKey : notnull
    {
        if (left.Count != right.Count) return false;
        var equalKey = (Func<TKey, TKey, bool>)keyEquality;
        var equalValue = (Func<TValue, TValue, bool>)valueEquality;
        var unmatched = right.ToList();
        foreach (var leftItem in left)
        {
            var match = unmatched.FindIndex(item => equalKey(leftItem.Key, item.Key));
            if (match < 0 || !equalValue(leftItem.Value, unmatched[match].Value)) return false;
            unmatched.RemoveAt(match);
        }
        return unmatched.Count == 0;
    }
}
