using System;

namespace Coflow.Runtime.CompilerServices;

internal sealed record CoflowEncodedValue(CoflowValueShape Shape, long[] Integers, double[] Floats, object?[] References)
{
    internal int LaneCount => checked(Integers.Length + Floats.Length + References.Length);

    internal static CoflowEncodedValue Encode(Type type, object? value, Func<Type, object?, CoflowEncodedValue>? encodeArenaValue = null)
    {
        CoflowValueShape coflowValueShape = CoflowValueShape.Of(type);
        long[] integers = new long[coflowValueShape.IntegerCount];
        double[] floats = new double[coflowValueShape.FloatCount];
        object?[] references = new object?[coflowValueShape.ReferenceCount];
        Encode(coflowValueShape, value, 0, 0, 0, integers, floats, references, encodeArenaValue);
        return new CoflowEncodedValue(coflowValueShape, integers, floats, references);
    }

    internal static CoflowEncodedValue EncodeArenaField(Type type, object? value, Func<Type, object?, CoflowEncodedValue>? encodeArenaValue = null)
    {
        if (value != null && !type.IsValueType && CoflowTypes.TryGet(type, out var _) && CoflowTypeCodecs.TryGet(value.GetType(), out CoflowTypeDescriptor descriptor))
        {
            CoflowValueId valueIdObject = descriptor.GetValueIdObject(value);
            return new CoflowEncodedValue(CoflowValueShape.Of(type), new long[1] { (long)valueIdObject.Packed }, Array.Empty<double>(), Array.Empty<object>());
        }
        if (CoflowStructCodecs.TryGet(type, out CoflowStructDescriptor descriptor2))
        {
            return descriptor2.Encode(value!, encodeArenaValue);
        }
        return Encode(type, value, encodeArenaValue);
    }

    private static void Encode(CoflowValueShape shape, object? value, int integerBase, int floatBase, int referenceBase, long[] integers, double[] floats, object?[] references, Func<Type, object?, CoflowEncodedValue>? encodeArenaValue = null)
    {
        if (shape.Kind == CoflowValueShapeKind.Unit)
        {
            return;
        }
        if (shape.Kind == CoflowValueShapeKind.Record)
        {
            if (value == null || !CoflowTypeCodecs.TryGet(value.GetType(), out CoflowTypeDescriptor descriptor))
            {
                throw new InvalidOperationException($"No schema codec exists for `{shape.Type}`.");
            }
            integers[integerBase] = (long)descriptor.GetValueIdObject(value).Packed;
            return;
        }
        if (shape.Kind == CoflowValueShapeKind.Function)
        {
            if (value is not ICoflowFunctionHandle function)
                throw new InvalidOperationException($"`{shape.Type}` is not a Coflow function handle.");
            integers[integerBase] = function.FunctionId.Packed;
            integers[integerBase + 1] = unchecked((long)function.EnvironmentId.Packed);
            return;
        }
        if (shape.Kind == CoflowValueShapeKind.Collection)
        {
            if (encodeArenaValue == null)
            {
                throw new InvalidOperationException($"Collection `{shape.Type}` requires a collection Arena encoder.");
            }
            CoflowEncodedValue coflowEncodedValue = encodeArenaValue(shape.Type, value);
            integers[integerBase] = coflowEncodedValue.Integers[0];
            return;
        }
        if (shape.Kind == CoflowValueShapeKind.Scalar)
        {
            switch (shape.ScalarKind)
            {
                case CoflowRegisterKind.Integer:
                    integers[integerBase] = shape.Type == typeof(bool)
                        ? (bool)value! ? 1 : 0
                        : Convert.ToInt64(value);
                    break;
                case CoflowRegisterKind.Float:
                    floats[floatBase] = (double)value!;
                    break;
                default:
                    references[referenceBase] = value;
                    break;
            }
            return;
        }
        if (shape.Kind == CoflowValueShapeKind.Struct)
        {
            if (!CoflowStructCodecs.TryGet(shape.Type, out CoflowStructDescriptor descriptor2))
            {
                throw new InvalidOperationException($"No schema struct codec exists for `{shape.Type}`.");
            }
            CoflowEncodedValue coflowEncodedValue2 = descriptor2.Encode(value!, encodeArenaValue);
            Array.Copy(coflowEncodedValue2.Integers, 0, integers, integerBase, coflowEncodedValue2.Integers.Length);
            Array.Copy(coflowEncodedValue2.Floats, 0, floats, floatBase, coflowEncodedValue2.Floats.Length);
            Array.Copy(coflowEncodedValue2.References, 0, references, referenceBase, coflowEncodedValue2.References.Length);
            return;
        }
        var union = CoflowUnionAccessors.For(shape.Type, shape.Kind);
        var flag = union.Tag(value!);
        integers[integerBase] = (flag ? 1 : 0);
        if (shape.Kind == CoflowValueShapeKind.Option)
        {
            if (flag)
            {
                Encode(shape.First!, union.First(value!), integerBase + 1, floatBase, referenceBase, integers, floats, references, encodeArenaValue);
            }
        }
        else
        {
            CoflowValueShape shape2 = (flag ? shape.First : shape.Second)!;
            Encode(shape2, flag ? union.First(value!) : union.Second!(value!),
                integerBase + 1 + (!flag ? shape.First!.IntegerCount : 0),
                floatBase + (!flag ? shape.First!.FloatCount : 0),
                referenceBase + (!flag ? shape.First!.ReferenceCount : 0),
                integers, floats, references, encodeArenaValue);
        }
    }
}

internal sealed record CoflowUnionAccessors(
    Func<object, bool> Tag,
    Func<object, object?> First,
    Func<object, object?>? Second)
{
    private static readonly System.Collections.Concurrent.ConcurrentDictionary<
        Type, CoflowUnionAccessors> Cache = new();

    internal static CoflowUnionAccessors For(Type type, CoflowValueShapeKind kind) =>
        Cache.GetOrAdd(type, value => Build(value, kind));

    private static CoflowUnionAccessors Build(Type type, CoflowValueShapeKind kind)
    {
        if (kind is not (CoflowValueShapeKind.Option or CoflowValueShapeKind.Result))
            throw new InvalidOperationException($"`{type}` is not an Option or Result type.");
        var value = System.Linq.Expressions.Expression.Parameter(typeof(object), "value");
        var typed = System.Linq.Expressions.Expression.Convert(value, type);
        Func<string, Type, Delegate> reader = (name, resultType) =>
        {
            var property = type.GetProperty(name) ?? throw new InvalidOperationException(
                $"`{type}` has no `{name}` property.");
            var body = System.Linq.Expressions.Expression.Convert(
                System.Linq.Expressions.Expression.Property(typed, property), resultType);
            var delegateType = typeof(Func<,>).MakeGenericType(typeof(object), resultType);
            return CoflowExpressionCompiler.Compile(
                System.Linq.Expressions.Expression.Lambda(delegateType, body, value));
        };
        return new CoflowUnionAccessors(
            (Func<object, bool>)reader(kind == CoflowValueShapeKind.Option ? "HasValue" : "IsOk", typeof(bool)),
            (Func<object, object?>)reader("Value", typeof(object)),
            kind == CoflowValueShapeKind.Result
                ? (Func<object, object?>)reader("Error", typeof(object))
                : null);
    }
}
