namespace Coflow.Runtime.CompilerServices;

// 只借用现有列及偏移；比较过程不解码 CLR 集合或复制寄存器。
internal readonly record struct CoflowValueView(
    IList<long> Integers, IList<double> Floats, IList<object?> References,
    int IntegerBase, int FloatBase, int ReferenceBase)
{
    internal long Integer => Integers[IntegerBase];
    internal double Float => Floats[FloatBase];
    internal object? Reference => References[ReferenceBase];
    internal CoflowValueView Advance(int integers, int floats = 0, int references = 0) =>
        this with { IntegerBase = IntegerBase + integers, FloatBase = FloatBase + floats,
            ReferenceBase = ReferenceBase + references };
}

internal static class CoflowEquality
{
    internal delegate bool Comparer(CoflowExecutionSession session, CoflowValueView left, CoflowValueView right);

    internal static CoflowNativeCall Create(Type type)
    {
        var compare = Build(CoflowValueShape.Of(type));
        return new CoflowNativeCall(new[] { type, type }, typeof(bool),
            frame => frame.Write(frame.Compare(compare)));
    }

    private static Comparer Build(CoflowValueShape shape)
    {
        switch (shape.Kind)
        {
            case CoflowValueShapeKind.Unit:
                return static (_, _, _) => true;
            case CoflowValueShapeKind.Scalar:
                if (shape.ScalarKind == CoflowRegisterKind.Integer)
                    return static (_, left, right) => left.Integer == right.Integer;
                if (shape.ScalarKind == CoflowRegisterKind.Float)
                    return static (_, left, right) => left.Float == right.Float;
                return shape.Type == typeof(string)
                    ? static (_, left, right) => (string?)left.Reference == (string?)right.Reference
                    : static (_, left, right) => ReferenceEquals(left.Reference, right.Reference);
            case CoflowValueShapeKind.Record:
                return (session, left, right) => ReferenceEquals(
                    session.ApiValue(CoflowValueId.FromPacked(unchecked((ulong)left.Integer)), shape.Type),
                    session.ApiValue(CoflowValueId.FromPacked(unchecked((ulong)right.Integer)), shape.Type));
            case CoflowValueShapeKind.Option:
            case CoflowValueShapeKind.Result:
            {
                var first = Build(shape.First!);
                var second = shape.Second is null ? null : Build(shape.Second);
                return (session, left, right) =>
                {
                    if (left.Integer != right.Integer) return false;
                    if (left.Integer != 0)
                        return first(session, left.Advance(1), right.Advance(1));
                    return second is null || second(session,
                        left.Advance(1 + shape.First!.IntegerCount, shape.First.FloatCount, shape.First.ReferenceCount),
                        right.Advance(1 + shape.First!.IntegerCount, shape.First.FloatCount, shape.First.ReferenceCount));
                };
            }
            case CoflowValueShapeKind.Struct:
            {
                CoflowSchemaRuntimeContext.TryGetStructCodec(shape.Type, out var descriptor);
                var fields = descriptor.FieldTypes.Select(type => CoflowValueShape.Of(type)).ToArray();
                var comparisons = fields.Select(Build).ToArray();
                return (session, left, right) =>
                {
                    // struct 首列是运行时身份，不属于值相等的字段。
                    left = left.Advance(1);
                    right = right.Advance(1);
                    for (var index = 0; index < fields.Length; index++)
                    {
                        if (!comparisons[index](session, left, right)) return false;
                        var field = fields[index];
                        left = left.Advance(field.IntegerCount, field.FloatCount, field.ReferenceCount);
                        right = right.Advance(field.IntegerCount, field.FloatCount, field.ReferenceCount);
                    }
                    return true;
                };
            }
            case CoflowValueShapeKind.Collection:
            {
                var arguments = shape.Type.GetGenericArguments();
                var dictionary = arguments.Length == 2;
                var compare = Build(CoflowValueShape.Of(arguments[dictionary ? 1 : 0]));
                return (session, left, right) =>
                {
                    var leftId = CoflowCollectionId.FromPacked(unchecked((ulong)left.Integer));
                    var rightId = CoflowCollectionId.FromPacked(unchecked((ulong)right.Integer));
                    var leftArena = session.CollectionArena(leftId);
                    var rightArena = session.CollectionArena(rightId);
                    var count = leftArena.ItemCount(leftId);
                    if (count != rightArena.ItemCount(rightId)) return false;
                    for (var index = 0; index < count; index++)
                    {
                        var other = dictionary
                            ? rightArena.FindDictionaryKey(rightId, leftArena.View(leftId, index))
                            : index;
                        if (other < 0 || !compare(session,
                            leftArena.View(leftId, index, dictionary),
                            rightArena.View(rightId, other, dictionary))) return false;
                    }
                    return true;
                };
            }
            default:
                throw new InvalidOperationException($"Type `{shape.Type}` does not support equality.");
        }
    }
}
