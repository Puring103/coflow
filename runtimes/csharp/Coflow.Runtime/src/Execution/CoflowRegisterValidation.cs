namespace Coflow.Runtime.CompilerServices;

internal static partial class CoflowRegisterLowering
{
    private static IEnumerable<(int Pc, Type[] Stack)> Transfer(
        CoflowLoweringInput program,
        int pc,
        Type[] input,
        Type?[] locals)
    {
        var instruction = program.Instructions[pc];
        var stack = input.ToList();
        Type Pop()
        {
            if (stack.Count == 0) throw Invalid(program, $"stack underflow at instruction {pc}");
            var value = stack[^1];
            stack.RemoveAt(stack.Count - 1);
            return value;
        }
        int Index(int value, int count, string kind)
        {
            if ((uint)value >= (uint)count)
                throw Invalid(program, $"instruction {pc} has an invalid {kind} index {value}");
            return value;
        }
        T Operation<T>() where T : notnull
        {
            var index = Index(instruction.Operand, program.Operations.Length, "operation");
            return program.Operations[index] is T value
                ? value
                : throw Invalid(program, $"instruction {pc} has an invalid {typeof(T).Name} descriptor");
        }
        void RequireKind(Type actual, CoflowRegisterKind expected)
        {
            var shape = CoflowValueShape.Of(actual);
            if (shape.Kind != CoflowValueShapeKind.Scalar || shape.ScalarKind != expected)
                throw Invalid(program, $"instruction {pc} ({instruction.Code}) reads `{actual}` as {expected}");
        }
        void PopMany(int count) { for (var index = 0; index < count; index++) Pop(); }
        void PopArguments(IReadOnlyList<Type> expected)
        {
            for (var index = expected.Count - 1; index >= 0; index--)
            {
                var actual = Pop();
                if (actual != expected[index] && !expected[index].IsAssignableFrom(actual))
                    throw Invalid(program, $"instruction {pc} argument {index} expects `{expected[index]}`, found `{actual}`");
            }
        }
        var resultType = instruction.ValueType ?? typeof(object);
        switch (instruction.Code)
        {
            case CoflowOpCode.Constant:
                stack.Add(instruction.ValueType ?? program.EncodedConstants[
                    Index(instruction.Operand, program.EncodedConstants.Length, "constant")]?.Shape.Type ?? typeof(object));
                break;
            case CoflowOpCode.Argument:
                stack.Add(program.ParameterTypes[
                Index(instruction.Operand, program.ParameterTypes.Length, "argument")]); break;
            case CoflowOpCode.Local:
                stack.Add(locals[Index(instruction.Operand, locals.Length, "local")] ??
                    throw Invalid(program, $"local {instruction.Operand} is read before assignment"));
                break;
            case CoflowOpCode.StoreLocal:
                {
                    var type = Pop();
                    var local = Index(instruction.Operand, locals.Length, "local");
                    if (locals[local] is { } existing && existing != type)
                        throw Invalid(program, $"local {instruction.Operand} changes type from `{existing}` to `{type}`");
                    locals[local] = type;
                    break;
                }
            case CoflowOpCode.LoadField:
                {
                    var receiver = CoflowValueShape.Of(Pop());
                    if (receiver.Kind != CoflowValueShapeKind.Struct &&
                        receiver.Kind != CoflowValueShapeKind.Record &&
                        (receiver.Kind != CoflowValueShapeKind.Scalar ||
                         receiver.ScalarKind != CoflowRegisterKind.Reference))
                        throw Invalid(program,
                            $"instruction {pc} ({instruction.Code}) requires a reference or struct receiver");
                    var access = Operation<CoflowFieldAccess>();
                    if (access.ReceiverIsStruct)
                    {
                        if (receiver.Kind != CoflowValueShapeKind.Struct)
                            throw Invalid(program,
                                $"instruction {pc} ({instruction.Code}) requires a struct receiver");
                    }
                    else if (access.IsHost
                        ? receiver.Kind != CoflowValueShapeKind.Scalar ||
                          receiver.ScalarKind != CoflowRegisterKind.Reference
                        : receiver.Kind != CoflowValueShapeKind.Record)
                    {
                        throw Invalid(program,
                            $"instruction {pc} ({instruction.Code}) has an invalid receiver layout");
                    }
                    stack.Add(access.RuntimeType);
                    break;
                }
            case CoflowOpCode.MakeOptionSome:
            case CoflowOpCode.MakeResultOk:
            case CoflowOpCode.MakeResultErr:
                {
                    var source = Pop();
                    var target = CoflowValueShape.Of(resultType);
                    var payload = instruction.Code == CoflowOpCode.MakeResultErr ? target.Second : target.First;
                    if (target.Kind != (instruction.Code == CoflowOpCode.MakeOptionSome
                            ? CoflowValueShapeKind.Option : CoflowValueShapeKind.Result) || payload?.Type != source)
                        throw Invalid(program, $"instruction {pc} cannot construct `{resultType}` from `{source}`");
                    stack.Add(resultType);
                    break;
                }
            case CoflowOpCode.ReadFirstPayload:
            case CoflowOpCode.ReadSecondPayload:
                {
                    var source = CoflowValueShape.Of(Pop());
                    var payload = instruction.Code == CoflowOpCode.ReadFirstPayload ? source.First : source.Second;
                    if (payload?.Type != resultType)
                        throw Invalid(program, $"instruction {pc} payload type does not match `{resultType}`");
                    stack.Add(resultType);
                    break;
                }
            case CoflowOpCode.Propagate:
                {
                    var source = CoflowValueShape.Of(Pop());
                    var returned = CoflowValueShape.Of(program.ReturnType);
                    if (source.Kind is not (CoflowValueShapeKind.Option or CoflowValueShapeKind.Result) ||
                        source.First?.Type != resultType || source.Kind != returned.Kind ||
                        source.Kind == CoflowValueShapeKind.Result && source.Second?.Type != returned.Second?.Type)
                        throw Invalid(program, $"instruction {pc} has incompatible propagation layouts");
                    stack.Add(resultType);
                    break;
                }
            case CoflowOpCode.MakeOptionNone:
                if (CoflowValueShape.Of(resultType).Kind != CoflowValueShapeKind.Option)
                    throw Invalid(program, $"instruction {pc} creates None with non-Option type `{resultType}`");
                stack.Add(resultType); break;
            case CoflowOpCode.ReadValueTag:
                {
                    var shape = CoflowValueShape.Of(Pop());
                    if (shape.Kind is not (CoflowValueShapeKind.Option or CoflowValueShapeKind.Result))
                        throw Invalid(program, $"instruction {pc} reads a tag from `{shape.Type}`");
                    stack.Add(typeof(bool));
                    break;
                }
            case CoflowOpCode.Reinterpret:
                {
                    var source = CoflowValueShape.Of(Pop());
                    var target = CoflowValueShape.Of(resultType);
                    // CollectionId 虽然占用 integer lane，但它不是可参与数值重解释的整数。
                    if ((source.Kind == CoflowValueShapeKind.Collection ||
                         target.Kind == CoflowValueShapeKind.Collection) &&
                        (source.Kind != CoflowValueShapeKind.Collection ||
                         target.Kind != CoflowValueShapeKind.Collection ||
                         source.Type != target.Type))
                        throw Invalid(program, $"instruction {pc} reinterprets a collection handle as `{resultType}`");
                    if (source.IntegerCount != target.IntegerCount || source.FloatCount != target.FloatCount ||
                        source.ReferenceCount != target.ReferenceCount)
                        throw Invalid(program, $"instruction {pc} reinterprets incompatible layouts");
                    stack.Add(resultType);
                    break;
                }
            case CoflowOpCode.ConvertIntToFloat:
                RequireKind(Pop(), CoflowRegisterKind.Integer); stack.Add(typeof(double)); break;
            case CoflowOpCode.ConvertFloatToInt:
                RequireKind(Pop(), CoflowRegisterKind.Float); stack.Add(typeof(long)); break;
            case CoflowOpCode.IsType:
                _ = Operation<Type>();
                var tested = CoflowValueShape.Of(Pop());
                if (tested.Kind != CoflowValueShapeKind.Record &&
                    tested.ScalarKind != CoflowRegisterKind.Reference)
                    throw Invalid(program, $"instruction {pc} requires a schema record or reference value");
                stack.Add(typeof(bool)); break;
            case CoflowOpCode.MakeArray:
                {
                    var shape = CoflowValueShape.Of(resultType);
                    if (shape.Kind != CoflowValueShapeKind.Collection ||
                        resultType.GetGenericTypeDefinition() != typeof(IReadOnlyList<>))
                        throw Invalid(program, $"instruction {pc} creates an array with non-array type `{resultType}`");
                    var element = resultType.GetGenericArguments()[0];
                    for (var index = 0; index < instruction.Operand; index++)
                        if (Pop() != element)
                            throw Invalid(program, $"instruction {pc} array element does not match `{element}`");
                    stack.Add(resultType);
                    break;
                }
            case CoflowOpCode.MakeDictionary:
                {
                    var shape = CoflowValueShape.Of(resultType);
                    if (shape.Kind != CoflowValueShapeKind.Collection ||
                        resultType.GetGenericTypeDefinition() != typeof(IReadOnlyDictionary<,>))
                        throw Invalid(program,
                            $"instruction {pc} creates a dictionary with non-dictionary type `{resultType}`");
                    var arguments = resultType.GetGenericArguments();
                    for (var index = 0; index < instruction.Operand; index++)
                    {
                        if (Pop() != arguments[1] || Pop() != arguments[0])
                            throw Invalid(program, $"instruction {pc} dictionary entry has an invalid type");
                    }
                    stack.Add(resultType);
                    break;
                }
            case CoflowOpCode.ArrayIndex:
            case CoflowOpCode.DictionaryIndex:
                {
                    var key = Pop();
                    var collection = Pop();
                    if (!collection.IsGenericType)
                        throw Invalid(program, $"instruction {pc} indexes a non-collection value `{collection}`");
                    var definition = collection.GetGenericTypeDefinition();
                    var arguments = collection.GetGenericArguments();
                    Type expected;
                    if (instruction.Code == CoflowOpCode.ArrayIndex)
                    {
                        RequireKind(key, CoflowRegisterKind.Integer);
                        if (definition != typeof(IReadOnlyList<>))
                            throw Invalid(program, $"instruction {pc} indexes a non-array value `{collection}`");
                        expected = typeof(Option<>).MakeGenericType(arguments[0]);
                    }
                    else
                    {
                        if (definition != typeof(IReadOnlyDictionary<,>) || key != arguments[0])
                            throw Invalid(program, $"instruction {pc} uses an invalid dictionary key `{key}`");
                        expected = typeof(Option<>).MakeGenericType(arguments[1]);
                    }
                    if (resultType != expected)
                        throw Invalid(program, $"instruction {pc} index result must be `{expected}`");
                    stack.Add(resultType);
                    break;
                }
            case CoflowOpCode.CollectionCount:
                {
                    var collection = CoflowValueShape.Of(Pop());
                    if (collection.Kind != CoflowValueShapeKind.Collection || resultType != typeof(long))
                        throw Invalid(program, $"instruction {pc} requires a collection and returns int");
                    stack.Add(typeof(long));
                    break;
                }
            case CoflowOpCode.ArrayItem:
            case CoflowOpCode.DictionaryKey:
            case CoflowOpCode.DictionaryValue:
                {
                    RequireKind(Pop(), CoflowRegisterKind.Integer);
                    var collection = Pop();
                    if (!collection.IsGenericType)
                        throw Invalid(program, $"instruction {pc} reads a non-collection value `{collection}`");
                    var definition = collection.GetGenericTypeDefinition();
                    var arguments = collection.GetGenericArguments();
                    var expected = instruction.Code switch
                    {
                        CoflowOpCode.ArrayItem when definition == typeof(IReadOnlyList<>) => arguments[0],
                        CoflowOpCode.DictionaryKey when definition == typeof(IReadOnlyDictionary<,>) => arguments[0],
                        CoflowOpCode.DictionaryValue when definition == typeof(IReadOnlyDictionary<,>) => arguments[1],
                        _ => throw Invalid(program,
                            $"instruction {pc} uses `{instruction.Code}` with `{collection}`"),
                    };
                    if (resultType != expected)
                        throw Invalid(program, $"instruction {pc} collection item result must be `{expected}`");
                    stack.Add(resultType);
                    break;
                }
            case CoflowOpCode.DictionaryKeys:
            case CoflowOpCode.DictionaryValues:
                {
                    var collection = Pop();
                    if (!collection.IsGenericType ||
                        collection.GetGenericTypeDefinition() != typeof(IReadOnlyDictionary<,>))
                        throw Invalid(program, $"instruction {pc} projects a non-dictionary `{collection}`");
                    var arguments = collection.GetGenericArguments();
                    var expectedElement = instruction.Code == CoflowOpCode.DictionaryKeys
                        ? arguments[0] : arguments[1];
                    if (!resultType.IsGenericType ||
                        resultType.GetGenericTypeDefinition() != typeof(IReadOnlyList<>) ||
                        resultType.GetGenericArguments()[0] != expectedElement)
                        throw Invalid(program, $"instruction {pc} dictionary projection result is incompatible");
                    stack.Add(resultType);
                    break;
                }
            case CoflowOpCode.CollectionBuiltin:
                {
                    var builtin = Operation<CoflowBuiltin>();
                    if (builtin.Kind == CoflowBuiltinKind.Native || builtin.ResultType != resultType)
                        throw Invalid(program, $"instruction {pc} has an invalid collection builtin descriptor");
                    var argument = builtin.HasCollectionArgument ? Pop() : null;
                    var receiver = Pop();
                    if (!receiver.IsGenericType)
                        throw Invalid(program, $"instruction {pc} uses a non-collection builtin receiver");
                    var definition = receiver.GetGenericTypeDefinition();
                    var arguments = receiver.GetGenericArguments();
                    var element = definition == typeof(IReadOnlyList<>) ? arguments[0] : null;
                    Type? expectedArgument = builtin.Kind switch
                    {
                        CoflowBuiltinKind.CollectionContains when definition == typeof(IReadOnlyList<>) => arguments[0],
                        CoflowBuiltinKind.DictionaryContainsKey when definition == typeof(IReadOnlyDictionary<,>) => arguments[0],
                        CoflowBuiltinKind.DictionaryContainsValue when definition == typeof(IReadOnlyDictionary<,>) => arguments[1],
                        CoflowBuiltinKind.CollectionIntersects or CoflowBuiltinKind.CollectionDisjoint or
                            CoflowBuiltinKind.CollectionSubset or CoflowBuiltinKind.CollectionSuperset
                            when definition == typeof(IReadOnlyList<>) => receiver,
                        _ when !builtin.HasCollectionArgument && definition == typeof(IReadOnlyList<>) => null,
                        _ => throw Invalid(program, $"instruction {pc} collection builtin is incompatible with `{receiver}`"),
                    };
                    if (argument != expectedArgument)
                        throw Invalid(program, $"instruction {pc} collection builtin argument is incompatible");
                    var expectedResult = builtin.Kind switch
                    {
                        CoflowBuiltinKind.CollectionMin or CoflowBuiltinKind.CollectionMax
                            when element == typeof(long) || element == typeof(double) ||
                                element == typeof(string) || element?.IsEnum == true => element,
                        CoflowBuiltinKind.CollectionSumInteger when element == typeof(long) => typeof(long),
                        CoflowBuiltinKind.CollectionSumFloat when element == typeof(double) => typeof(double),
                        CoflowBuiltinKind.CollectionContains or CoflowBuiltinKind.DictionaryContainsKey or
                            CoflowBuiltinKind.DictionaryContainsValue or CoflowBuiltinKind.CollectionUnique or
                            CoflowBuiltinKind.CollectionSorted or CoflowBuiltinKind.CollectionStrictlySorted or
                            CoflowBuiltinKind.CollectionIntersects or CoflowBuiltinKind.CollectionDisjoint or
                            CoflowBuiltinKind.CollectionSubset or CoflowBuiltinKind.CollectionSuperset => typeof(bool),
                        _ => throw Invalid(program, $"instruction {pc} has an invalid collection builtin operation"),
                    };
                    if (resultType != expectedResult)
                        throw Invalid(program, $"instruction {pc} collection builtin result is incompatible");
                    stack.Add(resultType);
                    break;
                }
            case CoflowOpCode.BeginArrayBuilder:
                {
                    RequireKind(Pop(), CoflowRegisterKind.Integer);
                    if (!resultType.IsGenericType ||
                        resultType.GetGenericTypeDefinition() != typeof(IReadOnlyList<>))
                        throw Invalid(program, $"instruction {pc} creates a builder for non-array `{resultType}`");
                    stack.Add(resultType);
                    break;
                }
            case CoflowOpCode.AppendArrayBuilder:
                {
                    var item = Pop();
                    var collection = Pop();
                    if (!collection.IsGenericType ||
                        collection.GetGenericTypeDefinition() != typeof(IReadOnlyList<>) ||
                        collection.GetGenericArguments()[0] != item || resultType != typeof(Unit))
                        throw Invalid(program, $"instruction {pc} appends an incompatible array item");
                    stack.Add(typeof(Unit));
                    break;
                }
            case CoflowOpCode.Native:
                {
                    var call = Operation<CoflowNativeCall>();
                    if (call.ResultType != resultType)
                        throw Invalid(program, $"instruction {pc} native result type does not match `{resultType}`");
                    for (var index = call.ArgumentCount - 1; index >= 0; index--)
                    {
                        var actual = Pop();
                        if (actual != call.ParameterTypes[index] &&
                            !(call.ParameterTypes[index].IsAssignableFrom(actual) &&
                                CoflowValueShape.Scalar(actual) == CoflowRegisterKind.Reference))
                            throw Invalid(program, $"instruction {pc} native argument {index} expects `{call.ParameterTypes[index]}`, found `{actual}`");
                    }
                    stack.Add(resultType); break;
                }
            case CoflowOpCode.MakeClosure:
                {
                    var closure = Operation<CoflowClosureTemplate>();
                    if (closure.CaptureCount < 0 || closure.CaptureCount > closure.Program.ParameterCount ||
                        closure.CaptureCount > stack.Count)
                        throw Invalid(program, $"instruction {pc} has an invalid closure capture count");
                    var captures = stack.Skip(stack.Count - closure.CaptureCount).ToArray();
                    PopMany(closure.CaptureCount);
                    var expectedCaptures = closure.Program.ParameterTypes
                        .Skip(closure.Program.ParameterCount - closure.CaptureCount).ToArray();
                    if (!captures.SequenceEqual(expectedCaptures))
                        throw Invalid(program, $"instruction {pc} closure capture signature does not match target");
                    stack.Add(instruction.ValueType ?? typeof(Delegate)); break;
                }
            case CoflowOpCode.Pop: Pop(); break;
            case CoflowOpCode.NegateInt:
            case CoflowOpCode.Not:
            case CoflowOpCode.BitNot: RequireKind(Pop(), CoflowRegisterKind.Integer); stack.Add(resultType); break;
            case CoflowOpCode.NegateFloat: RequireKind(Pop(), CoflowRegisterKind.Float); stack.Add(resultType); break;
            case CoflowOpCode.AddInt:
            case CoflowOpCode.SubtractInt:
            case CoflowOpCode.MultiplyInt:
            case CoflowOpCode.DivideInt:
            case CoflowOpCode.IntegerDivide:
            case CoflowOpCode.Remainder:
            case CoflowOpCode.PowerInt:
            case CoflowOpCode.ShiftLeft:
            case CoflowOpCode.ShiftRight:
            case CoflowOpCode.BitAnd:
            case CoflowOpCode.BitXor:
            case CoflowOpCode.BitOr:
                RequireKind(Pop(), CoflowRegisterKind.Integer); RequireKind(Pop(), CoflowRegisterKind.Integer);
                stack.Add(resultType); break;
            case CoflowOpCode.AddFloat:
            case CoflowOpCode.SubtractFloat:
            case CoflowOpCode.MultiplyFloat:
            case CoflowOpCode.DivideFloat:
            case CoflowOpCode.PowerFloat:
                RequireKind(Pop(), CoflowRegisterKind.Float); RequireKind(Pop(), CoflowRegisterKind.Float);
                stack.Add(resultType); break;
            case CoflowOpCode.AddString:
                RequireKind(Pop(), CoflowRegisterKind.Reference); RequireKind(Pop(), CoflowRegisterKind.Reference);
                stack.Add(typeof(string)); break;
            case CoflowOpCode.LessInt:
            case CoflowOpCode.LessOrEqualInt:
            case CoflowOpCode.GreaterInt:
            case CoflowOpCode.GreaterOrEqualInt:
                RequireKind(Pop(), CoflowRegisterKind.Integer); RequireKind(Pop(), CoflowRegisterKind.Integer);
                stack.Add(typeof(bool)); break;
            case CoflowOpCode.LessFloat:
            case CoflowOpCode.LessOrEqualFloat:
            case CoflowOpCode.GreaterFloat:
            case CoflowOpCode.GreaterOrEqualFloat:
                RequireKind(Pop(), CoflowRegisterKind.Float); RequireKind(Pop(), CoflowRegisterKind.Float);
                stack.Add(typeof(bool)); break;
            case CoflowOpCode.LessString:
            case CoflowOpCode.LessOrEqualString:
            case CoflowOpCode.GreaterString:
            case CoflowOpCode.GreaterOrEqualString:
                RequireKind(Pop(), CoflowRegisterKind.Reference); RequireKind(Pop(), CoflowRegisterKind.Reference);
                stack.Add(typeof(bool)); break;
            case CoflowOpCode.EqualInteger:
                RequireKind(Pop(), CoflowRegisterKind.Integer); RequireKind(Pop(), CoflowRegisterKind.Integer);
                stack.Add(typeof(bool)); break;
            case CoflowOpCode.EqualFloat:
                RequireKind(Pop(), CoflowRegisterKind.Float); RequireKind(Pop(), CoflowRegisterKind.Float);
                stack.Add(typeof(bool)); break;
            case CoflowOpCode.EqualReference:
                RequireKind(Pop(), CoflowRegisterKind.Reference); RequireKind(Pop(), CoflowRegisterKind.Reference);
                stack.Add(typeof(bool)); break;
            case CoflowOpCode.JumpIfFalseKeep:
            case CoflowOpCode.JumpIfTrueKeep:
                if (stack.Count == 0)
                    throw Invalid(program, $"stack underflow at instruction {pc}");
                RequireKind(stack[^1], CoflowRegisterKind.Integer);
                yield return (instruction.Operand, stack.ToArray());
                stack.RemoveAt(stack.Count - 1);
                break;
            case CoflowOpCode.JumpIfFalse:
                RequireKind(Pop(), CoflowRegisterKind.Integer);
                yield return (instruction.Operand, stack.ToArray());
                break;
            case CoflowOpCode.Jump:
                yield return (instruction.Operand, stack.ToArray()); yield break;
            case CoflowOpCode.Call:
                {
                    var call = Operation<CoflowCallSite>();
                    if (call.ArgumentCount != call.VmParameterTypes.Length)
                        throw Invalid(program, $"instruction {pc} call-site arity {call.ArgumentCount} does not match target {call.Identity} arity {call.VmParameterTypes.Length}");
                    PopArguments(call.VmParameterTypes);
                    stack.Add(call.Signature.ResultType); break;
                }
            case CoflowOpCode.CallIndirect:
                {
                    if (instruction.Operand < 0 || instruction.Operand >= stack.Count)
                        throw Invalid(program, $"instruction {pc} has an invalid indirect-call arity");
                    var arguments = stack.Skip(stack.Count - instruction.Operand).ToArray();
                    PopMany(instruction.Operand);
                    var callable = Pop();
                    ValidateCallable(callable, arguments, resultType, program, pc);
                    stack.Add(resultType);
                    break;
                }
            case CoflowOpCode.TailCall:
                {
                    var call = Operation<CoflowCallSite>();
                    if (call.ArgumentCount != call.VmParameterTypes.Length)
                        throw Invalid(program, $"instruction {pc} tail-call arity does not match target");
                    PopArguments(call.VmParameterTypes);
                    if (call.Signature.ResultType != program.ReturnType)
                        throw Invalid(program, $"instruction {pc} tail-call result does not match function return");
                    if (stack.Count != 0)
                        throw Invalid(program, $"instruction {pc} tail-call leaves values on the stack");
                    yield break;
                }
            case CoflowOpCode.TailCallIndirect:
                {
                    if (instruction.Operand < 0 || instruction.Operand >= stack.Count)
                        throw Invalid(program, $"instruction {pc} has an invalid indirect tail-call arity");
                    var arguments = stack.Skip(stack.Count - instruction.Operand).ToArray();
                    PopMany(instruction.Operand);
                    var callable = Pop();
                    ValidateCallable(callable, arguments, program.ReturnType, program, pc);
                    if (stack.Count != 0)
                        throw Invalid(program, $"instruction {pc} indirect tail-call leaves values on the stack");
                    yield break;
                }
            case CoflowOpCode.Return:
                {
                    var actual = Pop();
                    if (actual != program.ReturnType)
                        throw Invalid(program, $"return type `{actual}` does not match `{program.ReturnType}`");
                    if (stack.Count != 0)
                        throw Invalid(program, $"instruction {pc} return leaves values on the stack");
                    yield break;
                }
            default: throw Invalid(program, $"unknown opcode `{instruction.Code}`");
        }
        yield return (pc + 1, stack.ToArray());
    }

    private static bool Merge(CoflowLoweringInput program, Type[][] states, int pc, Type[] incoming)
    {
        if (pc < 0 || pc >= states.Length) throw Invalid(program, "jump target is outside the program");
        if (states[pc] is null) { states[pc] = incoming; return true; }
        if (!states[pc].SequenceEqual(incoming))
            throw Invalid(program,
                $"incompatible stack layout at instruction {pc}: " +
                $"[{string.Join(", ", states[pc].Select(type => type.Name))}] vs " +
                $"[{string.Join(", ", incoming.Select(type => type.Name))}]");
        return false;
    }

    private static void ValidateCallable(
        Type callable,
        IReadOnlyList<Type> arguments,
        Type result,
        CoflowLoweringInput program,
        int pc)
    {
        if (!CoflowFunctionHandle.IsFunctionType(callable))
            throw Invalid(program, $"instruction {pc} indirect target `{callable}` has incompatible result");
        var signature = callable.GetGenericArguments();
        if (signature[^1] != result)
            throw Invalid(program, $"instruction {pc} indirect target `{callable}` has incompatible result");
        var parameters = signature[..^1];
        if (parameters.Length != arguments.Count)
            throw Invalid(program, $"instruction {pc} indirect target arity does not match");
        for (var index = 0; index < parameters.Length; index++)
            if (parameters[index] != arguments[index] &&
                !parameters[index].IsAssignableFrom(arguments[index]))
                throw Invalid(program, $"instruction {pc} indirect argument {index} has incompatible type");
    }
    private static InvalidOperationException Invalid(CoflowLoweringInput program, string message) =>
        new($"invalid Coflow program `{program.Identity}`: {message}");
}
