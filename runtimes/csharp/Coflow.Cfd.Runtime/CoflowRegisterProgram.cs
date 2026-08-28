namespace CoflowRuntime;

internal enum CoflowRegisterKind : byte { Integer, Float, Reference }

internal readonly record struct CoflowRegister(CoflowRegisterKind Kind, int Index);

internal enum CoflowValueShapeKind : byte { Scalar, Unit, Option, Result }

internal sealed class CoflowValueShape
{
    private static readonly System.Collections.Concurrent.ConcurrentDictionary<Type, CoflowValueShape> Cache = new();

    private CoflowValueShape(
        Type type,
        CoflowValueShapeKind kind,
        CoflowRegisterKind? scalarKind,
        CoflowValueShape? first,
        CoflowValueShape? second)
    {
        Type = type;
        Kind = kind;
        ScalarKind = scalarKind;
        First = first;
        Second = second;
        IntegerCount = (kind is CoflowValueShapeKind.Option or CoflowValueShapeKind.Result ? 1 : 0) +
            (scalarKind == CoflowRegisterKind.Integer ? 1 : 0) +
            (first?.IntegerCount ?? 0) + (second?.IntegerCount ?? 0);
        FloatCount = (scalarKind == CoflowRegisterKind.Float ? 1 : 0) +
            (first?.FloatCount ?? 0) + (second?.FloatCount ?? 0);
        ReferenceCount = (scalarKind == CoflowRegisterKind.Reference ? 1 : 0) +
            (first?.ReferenceCount ?? 0) + (second?.ReferenceCount ?? 0);
    }

    internal Type Type { get; }
    internal CoflowValueShapeKind Kind { get; }
    internal CoflowRegisterKind? ScalarKind { get; }
    internal CoflowValueShape? First { get; }
    internal CoflowValueShape? Second { get; }
    internal int IntegerCount { get; }
    internal int FloatCount { get; }
    internal int ReferenceCount { get; }

    internal static CoflowValueShape Of(Type type) => Cache.GetOrAdd(type, Create);

    private static CoflowValueShape Create(Type type)
    {
        if (type == typeof(Unit))
            return new(type, CoflowValueShapeKind.Unit, null, null, null);
        if (type.IsGenericType)
        {
            var definition = type.GetGenericTypeDefinition();
            var arguments = type.GetGenericArguments();
            if (definition == typeof(Option<>))
                return new(type, CoflowValueShapeKind.Option, null, Of(arguments[0]), null);
            if (definition == typeof(Result<,>))
                return new(type, CoflowValueShapeKind.Result, null, Of(arguments[0]), Of(arguments[1]));
        }
        return new(type, CoflowValueShapeKind.Scalar, Scalar(type), null, null);
    }

    internal static CoflowRegisterKind Scalar(Type type) =>
        type == typeof(long) || type == typeof(bool) || type.IsEnum
            ? CoflowRegisterKind.Integer
            : type == typeof(double)
                ? CoflowRegisterKind.Float
                : CoflowRegisterKind.Reference;
}

internal readonly record struct CoflowValueRegister(
    CoflowValueShape Shape,
    int IntegerBase,
    int FloatBase,
    int ReferenceBase)
{
    internal CoflowRegister Scalar => Shape.ScalarKind switch
    {
        CoflowRegisterKind.Integer => new(CoflowRegisterKind.Integer, IntegerBase),
        CoflowRegisterKind.Float => new(CoflowRegisterKind.Float, FloatBase),
        CoflowRegisterKind.Reference => new(CoflowRegisterKind.Reference, ReferenceBase),
        _ => throw new InvalidOperationException($"`{Shape.Type}` is not a scalar VM value."),
    };

    internal CoflowRegister Tag => Shape.Kind is CoflowValueShapeKind.Option or CoflowValueShapeKind.Result
        ? new(CoflowRegisterKind.Integer, IntegerBase)
        : throw new InvalidOperationException($"`{Shape.Type}` has no tag register.");

    internal CoflowValueRegister First => Child(Shape.First, 1);
    internal CoflowValueRegister Second => Child(
        Shape.Second,
        1 + (Shape.First?.IntegerCount ?? 0),
        Shape.First?.FloatCount ?? 0,
        Shape.First?.ReferenceCount ?? 0);

    private CoflowValueRegister Child(
        CoflowValueShape? child,
        int integerOffset,
        int floatOffset = 0,
        int referenceOffset = 0) => child is null
            ? throw new InvalidOperationException($"`{Shape.Type}` has no requested payload.")
            : new(child, IntegerBase + integerOffset, FloatBase + floatOffset, ReferenceBase + referenceOffset);
}

internal readonly record struct CoflowExecutableInstruction(
    CoflowOpCode Code,
    int Operand,
    int Operand2,
    int Operand3,
    Type? ValueType,
    int StackDepth,
    int BlockCharge,
    object? Operation,
    CoflowEncodedValue? Constant);

internal sealed class CoflowRegisterProgram
{
    internal CoflowRegisterProgram(
        Type[][] stacks,
        CoflowValueRegister[] parameters,
        CoflowValueRegister[] locals,
        CoflowValueRegister[] temporaries,
        int[] blockCharges,
        IReadOnlyList<CoflowInstruction> instructions,
        IReadOnlyList<object?> operations,
        IReadOnlyList<CoflowEncodedValue?> encodedConstants,
        int integerRegisterCount,
        int floatRegisterCount,
        int referenceRegisterCount)
    {
        Stacks = stacks;
        StackDepths = stacks.Select(stack => stack.Length).ToArray();
        Parameters = parameters;
        Locals = locals;
        Temporaries = temporaries;
        BlockCharges = blockCharges;
        StackRegisters = stacks.Select(stack => stack.Select((type, depth) =>
        {
            var temporary = temporaries[depth];
            return new CoflowValueRegister(
                CoflowValueShape.Of(type),
                temporary.IntegerBase,
                temporary.FloatBase,
                temporary.ReferenceBase);
        }).ToArray()).ToArray();
        NativeCallSites = BuildNativeCallSites(instructions, operations);
        Instructions = BuildInstructions(instructions, operations, encodedConstants);
        ParameterIntegerCount = parameters.Sum(value => value.Shape.IntegerCount);
        ParameterFloatCount = parameters.Sum(value => value.Shape.FloatCount);
        ParameterReferenceCount = parameters.Sum(value => value.Shape.ReferenceCount);
        IntegerRegisterCount = integerRegisterCount;
        FloatRegisterCount = floatRegisterCount;
        ReferenceRegisterCount = referenceRegisterCount;
    }

    internal Type[][] Stacks { get; }
    internal int[] StackDepths { get; }
    internal CoflowValueRegister[][] StackRegisters { get; }
    internal CoflowValueRegister[] Parameters { get; }
    internal CoflowValueRegister[] Locals { get; }
    internal CoflowValueRegister[] Temporaries { get; }
    internal int[] BlockCharges { get; }
    internal CoflowNativeCallSite?[] NativeCallSites { get; }
    internal CoflowExecutableInstruction[] Instructions { get; }
    internal int IntegerRegisterCount { get; }
    internal int FloatRegisterCount { get; }
    internal int ReferenceRegisterCount { get; }
    internal int ParameterIntegerCount { get; }
    internal int ParameterFloatCount { get; }
    internal int ParameterReferenceCount { get; }
    internal CoflowValueRegister Stack(int pc, int depth) => StackRegisters[pc][depth];
    internal CoflowValueRegister Temporary(CoflowValueShape shape, int depth)
    {
        var temporary = Temporaries[depth];
        return new(shape, temporary.IntegerBase, temporary.FloatBase, temporary.ReferenceBase);
    }

    private CoflowNativeCallSite?[] BuildNativeCallSites(
        IReadOnlyList<CoflowInstruction> instructions,
        IReadOnlyList<object?> operations)
    {
        var sites = new CoflowNativeCallSite?[instructions.Count];
        for (var pc = 0; pc < instructions.Count; pc++)
        {
            var instruction = instructions[pc];
            if (instruction.Code != CoflowOpCode.Native) continue;
            var call = (CoflowNativeCall)operations[instruction.Operand]!;
            var depth = StackDepths[pc];
            var arguments = new CoflowValueRegister[call.ArgumentCount];
            for (var index = 0; index < arguments.Length; index++)
                arguments[index] = StackRegisters[pc][depth - arguments.Length + index];
            sites[pc] = new CoflowNativeCallSite(
                call,
                arguments,
                Temporary(CoflowValueShape.Of(instruction.ValueType!), depth - arguments.Length));
        }
        return sites;
    }

    private CoflowExecutableInstruction[] BuildInstructions(
        IReadOnlyList<CoflowInstruction> instructions,
        IReadOnlyList<object?> operations,
        IReadOnlyList<CoflowEncodedValue?> encodedConstants)
    {
        var decoded = new CoflowExecutableInstruction[instructions.Count];
        for (var pc = 0; pc < instructions.Count; pc++)
        {
            var instruction = instructions[pc];
            decoded[pc] = new CoflowExecutableInstruction(
                instruction.Code,
                instruction.Operand,
                instruction.Operand2,
                instruction.Operand3,
                instruction.ValueType,
                StackDepths[pc],
                BlockCharges[pc],
                instruction.Code == CoflowOpCode.Constant ? null :
                    instruction.Operand >= 0 && instruction.Operand < operations.Count
                        ? operations[instruction.Operand] : null,
                instruction.Code == CoflowOpCode.Constant
                    ? encodedConstants[instruction.Operand] : null);
        }
        return decoded;
    }
}

internal static class CoflowRegisterLowering
{
    internal static CoflowRegisterProgram Lower(CoflowProgram program)
    {
        var instructions = program.Instructions;
        var states = new Type[instructions.Length + 1][];
        var locals = new Type?[program.LocalCount];
        states[0] = Array.Empty<Type>();
        var changed = true;
        for (var pass = 0; changed && pass <= instructions.Length + program.LocalCount + 1; pass++)
        {
            changed = false;
            for (var pc = 0; pc < instructions.Length; pc++)
            {
                var before = states[pc];
                if (before is null) continue;
                foreach (var successor in Transfer(program, pc, before, locals))
                    changed |= Merge(states, successor.Pc, successor.Stack);
            }
        }
        if (states.Take(instructions.Length).Any(value => value is null))
            throw Invalid(program, "program contains unreachable or invalid instruction boundaries");

        var integer = 0;
        var floating = 0;
        var reference = 0;
        var parameters = Allocate(program.ParameterTypes, ref integer, ref floating, ref reference);
        var localTypes = locals.Select(value => value ?? typeof(Unit)).ToArray();
        var localRegisters = Allocate(localTypes, ref integer, ref floating, ref reference);
        var maxDepth = states.Where(value => value is not null).Max(value => value!.Length);
        var temporaries = new CoflowValueRegister[maxDepth];
        for (var depth = 0; depth < maxDepth; depth++)
        {
            var shapes = states.Where(value => value is not null && value.Length > depth)
                .Select(value => CoflowValueShape.Of(value![depth])).ToArray();
            var integerWidth = shapes.Max(value => value.IntegerCount);
            var floatWidth = shapes.Max(value => value.FloatCount);
            var referenceWidth = shapes.Max(value => value.ReferenceCount);
            temporaries[depth] = new CoflowValueRegister(
                CoflowValueShape.Of(typeof(Unit)), integer, floating, reference);
            integer += integerWidth;
            floating += floatWidth;
            reference += referenceWidth;
        }
        return new CoflowRegisterProgram(
            states.Take(instructions.Length).Select(value => value!).ToArray(),
            parameters,
            localRegisters,
            temporaries,
            BuildBlockCharges(instructions),
            instructions,
            program.Operations,
            program.EncodedConstants,
            integer,
            floating,
            reference);
    }

    private static int[] BuildBlockCharges(IReadOnlyList<CoflowInstruction> instructions)
    {
        var leaders = new bool[instructions.Count + 1];
        leaders[0] = true;
        for (var pc = 0; pc < instructions.Count; pc++)
        {
            var instruction = instructions[pc];
            var boundary = instruction.Code is CoflowOpCode.Native or CoflowOpCode.Call or
                CoflowOpCode.CallIndirect or CoflowOpCode.TailCall or CoflowOpCode.TailCallIndirect or
                CoflowOpCode.Return or CoflowOpCode.Jump or CoflowOpCode.JumpIfFalse or
                CoflowOpCode.JumpIfFalseKeep or CoflowOpCode.JumpIfTrueKeep;
            if (boundary)
            {
                leaders[pc] = true;
                if (pc + 1 < leaders.Length) leaders[pc + 1] = true;
            }
            if (instruction.Code is CoflowOpCode.Jump or CoflowOpCode.JumpIfFalse or
                CoflowOpCode.JumpIfFalseKeep or CoflowOpCode.JumpIfTrueKeep)
                leaders[instruction.Operand] = true;
        }
        var charges = new int[instructions.Count];
        for (var start = 0; start < instructions.Count;)
        {
            if (!leaders[start]) { start++; continue; }
            var end = start + 1;
            while (end < instructions.Count && !leaders[end]) end++;
            charges[start] = end - start;
            start = end;
        }
        return charges;
    }

    private static CoflowValueRegister[] Allocate(
        IReadOnlyList<Type> types,
        ref int integer,
        ref int floating,
        ref int reference)
    {
        var result = new CoflowValueRegister[types.Count];
        for (var index = 0; index < types.Count; index++)
        {
            var shape = CoflowValueShape.Of(types[index]);
            result[index] = new CoflowValueRegister(shape, integer, floating, reference);
            integer += shape.IntegerCount;
            floating += shape.FloatCount;
            reference += shape.ReferenceCount;
        }
        return result;
    }

    private static IEnumerable<(int Pc, Type[] Stack)> Transfer(
        CoflowProgram program,
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
            case CoflowOpCode.Argument: stack.Add(program.ParameterTypes[
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
                RequireKind(Pop(), CoflowRegisterKind.Reference);
                stack.Add(Operation<CoflowFieldAccess>().RuntimeType);
                break;
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
                RequireKind(Pop(), CoflowRegisterKind.Reference); stack.Add(typeof(bool)); break;
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
                if (call.ArgumentCount != call.Entry.Signature.ParameterTypes.Count)
                    throw Invalid(program, $"instruction {pc} call-site arity does not match target");
                PopArguments(call.Entry.Signature.ParameterTypes);
                stack.Add(call.Entry.Signature.ResultType); break;
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
                if (call.ArgumentCount != call.Entry.Signature.ParameterTypes.Count)
                    throw Invalid(program, $"instruction {pc} tail-call arity does not match target");
                PopArguments(call.Entry.Signature.ParameterTypes);
                if (call.Entry.Signature.ResultType != program.ReturnType)
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

    private static bool Merge(Type[][] states, int pc, Type[] incoming)
    {
        if (pc < 0 || pc >= states.Length) throw new InvalidOperationException("jump target is outside the program");
        if (states[pc] is null) { states[pc] = incoming; return true; }
        if (!states[pc].SequenceEqual(incoming))
            throw new InvalidOperationException($"incompatible stack layout at instruction {pc}");
        return false;
    }

    internal static CoflowRegisterKind Kind(Type type) => CoflowValueShape.Scalar(type);
    private static void ValidateCallable(
        Type callable,
        IReadOnlyList<Type> arguments,
        Type result,
        CoflowProgram program,
        int pc)
    {
        var invoke = callable.GetMethod("Invoke");
        if (invoke is null || invoke.ReturnType != (result == typeof(Unit) ? typeof(void) : result))
            throw Invalid(program, $"instruction {pc} indirect target `{callable}` has incompatible result");
        var parameters = invoke.GetParameters();
        if (parameters.Length != arguments.Count)
            throw Invalid(program, $"instruction {pc} indirect target arity does not match");
        for (var index = 0; index < parameters.Length; index++)
            if (parameters[index].ParameterType != arguments[index] &&
                !parameters[index].ParameterType.IsAssignableFrom(arguments[index]))
                throw Invalid(program, $"instruction {pc} indirect argument {index} has incompatible type");
    }
    private static InvalidOperationException Invalid(CoflowProgram program, string message) =>
        new($"invalid Coflow program `{program.Identity}`: {message}");
}
