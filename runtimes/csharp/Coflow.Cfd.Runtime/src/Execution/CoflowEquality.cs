namespace CoflowRuntime.Generated;

using System.Linq.Expressions;

internal static class CoflowEquality
{
    internal static Delegate Create(Type type, IReadOnlyDictionary<string, ICoflowTypeMetadata> metadata)
    {
        var metadataByRuntimeType = metadata.Values.ToDictionary(item => item.RuntimeType);
        return Create(type, metadata, metadataByRuntimeType);
    }

    private static Delegate Create(
        Type type,
        IReadOnlyDictionary<string, ICoflowTypeMetadata> metadata,
        IReadOnlyDictionary<Type, ICoflowTypeMetadata> metadataByRuntimeType)
    {
        var left = Expression.Parameter(type, "left");
        var right = Expression.Parameter(type, "right");
        return CoflowExpressionCompiler.Compile(Expression.Lambda(
            Expression.GetFuncType(type, type, typeof(bool)),
            Equal(type, left, right, metadata, metadataByRuntimeType), left, right));
    }

    private static Expression Equal(
        Type type,
        Expression left,
        Expression right,
        IReadOnlyDictionary<string, ICoflowTypeMetadata> metadata,
        IReadOnlyDictionary<Type, ICoflowTypeMetadata> metadataByRuntimeType)
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
                            Expression.Property(right, nameof(Option<int>.Value)), metadata, metadataByRuntimeType)));
            }
            if (definition == typeof(Result<,>))
            {
                var leftOk = Expression.Property(left, nameof(Result<int, int>.IsOk));
                var rightOk = Expression.Property(right, nameof(Result<int, int>.IsOk));
                return Expression.AndAlso(Expression.Equal(leftOk, rightOk),
                    Expression.Condition(leftOk,
                        Equal(arguments[0],
                            Expression.Property(left, nameof(Result<int, int>.Value)),
                            Expression.Property(right, nameof(Result<int, int>.Value)), metadata, metadataByRuntimeType),
                        Equal(arguments[1],
                            Expression.Property(left, nameof(Result<int, int>.Error)),
                            Expression.Property(right, nameof(Result<int, int>.Error)), metadata, metadataByRuntimeType)));
            }
            if (definition == typeof(IReadOnlyList<>))
                return Expression.Call(typeof(CoflowEquality), nameof(ListEqual), arguments,
                    left, right, Expression.Constant(Create(arguments[0], metadata, metadataByRuntimeType)));
            if (definition == typeof(IReadOnlyDictionary<,>))
                return Expression.Call(typeof(CoflowEquality), nameof(DictionaryEqual), arguments,
                    left, right, Expression.Constant(Create(arguments[1], metadata, metadataByRuntimeType)));
        }

        if (type == typeof(string)) return Expression.Equal(left, right);
        metadataByRuntimeType.TryGetValue(type, out var generated);
        if (generated is null || generated is ICoflowRecordMetadata)
            return type.IsValueType ? Expression.Equal(left, right) : Expression.ReferenceEqual(left, right);
        Expression result = Expression.Constant(true);
        foreach (var field in generated.FieldNames)
        {
            var binding = generated.GetFieldBinding(field);
            var fieldType = binding.RuntimeType;
            var reader = binding.Reader;
            var delegateType = Expression.GetFuncType(type, fieldType);
            var leftField = Expression.Invoke(Expression.Convert(Expression.Constant(reader), delegateType), left);
            var rightField = Expression.Invoke(Expression.Convert(Expression.Constant(reader), delegateType), right);
            result = Expression.AndAlso(result,
                Equal(fieldType, leftField, rightField, metadata, metadataByRuntimeType));
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
        Delegate valueEquality) where TKey : notnull
    {
        if (left.Count != right.Count) return false;
        var equalValue = (Func<TValue, TValue, bool>)valueEquality;
        foreach (var leftItem in left)
        {
            if (!right.TryGetValue(leftItem.Key, out var rightValue) ||
                !equalValue(leftItem.Value, rightValue)) return false;
        }
        return true;
    }
}
