namespace Coflow.Runtime.CompilerServices;

internal static partial class CoflowRegisterLowering
{
    internal static CoflowRegisterProgram Lower(CoflowLoweringInput program)
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
                    changed |= Merge(program, states, successor.Pc, successor.Stack);
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
        var returnValue = Allocate(program.ReturnType, ref integer, ref floating, ref reference);
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
        var directSignatures = instructions
            .Where(instruction => instruction.Code is CoflowOpCode.Call or CoflowOpCode.TailCall)
            .Select(instruction => (CoflowCallSite)program.Operations[instruction.Operand]!)
            .Select(call => (IReadOnlyList<Type>)call.VmParameterTypes)
            .ToArray();
        var outgoingIntegerBase = integer;
        var outgoingFloatBase = floating;
        var outgoingReferenceBase = reference;
        if (directSignatures.Length != 0)
        {
            integer += directSignatures.Max(types => types.Sum(type => CoflowValueShape.Of(type).IntegerCount));
            floating += directSignatures.Max(types => types.Sum(type => CoflowValueShape.Of(type).FloatCount));
            reference += directSignatures.Max(types => types.Sum(type => CoflowValueShape.Of(type).ReferenceCount));
        }
        var (executable, instructionSpans, immediates, operations) = LowerInstructions(
            program,
            states.Take(instructions.Length).Select(value => value!).ToArray(),
            parameters,
            localRegisters,
            returnValue,
            temporaries,
            outgoingIntegerBase,
            outgoingFloatBase,
            outgoingReferenceBase);
        CompactPhysicalRegisters(
            parameters,
            executable,
            operations,
            ref integer,
            ref floating,
            ref reference);
        return new CoflowRegisterProgram(
            parameters,
            executable,
            instructionSpans,
            immediates,
            operations,
            integer,
            floating,
            reference);
    }

    private static void CompactPhysicalRegisters(
        CoflowValueRegister[] parameters,
        CoflowRegisterInstruction[] instructions,
        CoflowRegisterOperations operations,
        ref int integerCount,
        ref int floatCount,
        ref int referenceCount)
    {
        var integers = new bool[integerCount];
        var floats = new bool[floatCount];
        var references = new bool[referenceCount];

        void Mark(CoflowValueRegister value)
        {
            MarkRange(integers, value.IntegerBase, value.Shape.IntegerCount);
            MarkRange(floats, value.FloatBase, value.Shape.FloatCount);
            MarkRange(references, value.ReferenceBase, value.Shape.ReferenceCount);
        }
        foreach (var parameter in parameters) Mark(parameter);
        foreach (var instruction in instructions)
        {
            MarkOperand(instruction.A, OperandKind(instruction.Code, 0), integers, floats, references);
            MarkOperand(instruction.B, OperandKind(instruction.Code, 1), integers, floats, references);
            MarkOperand(instruction.C, OperandKind(instruction.Code, 2), integers, floats, references);
        }
        VisitOperationRegisters(operations, Mark);

        var integerMap = DenseMap(integers, out integerCount);
        var floatMap = DenseMap(floats, out floatCount);
        var referenceMap = DenseMap(references, out referenceCount);
        CoflowValueRegister Map(CoflowValueRegister value) => value with
        {
            IntegerBase = MapBase(integerMap, value.IntegerBase, value.Shape.IntegerCount),
            FloatBase = MapBase(floatMap, value.FloatBase, value.Shape.FloatCount),
            ReferenceBase = MapBase(referenceMap, value.ReferenceBase, value.Shape.ReferenceCount),
        };

        for (var index = 0; index < parameters.Length; index++) parameters[index] = Map(parameters[index]);
        for (var index = 0; index < instructions.Length; index++)
        {
            var instruction = instructions[index];
            instructions[index] = instruction with
            {
                A = MapOperand(instruction.A, OperandKind(instruction.Code, 0),
                    integerMap, floatMap, referenceMap),
                B = MapOperand(instruction.B, OperandKind(instruction.Code, 1),
                    integerMap, floatMap, referenceMap),
                C = MapOperand(instruction.C, OperandKind(instruction.Code, 2),
                    integerMap, floatMap, referenceMap),
            };
        }
        MapOperationRegisters(operations, Map);
    }

    private static void VisitOperationRegisters(
        CoflowRegisterOperations operations,
        Action<CoflowValueRegister> visit)
    {
        foreach (var value in operations.Constants) VisitOperation(value, visit);
        foreach (var value in operations.Transfers) VisitOperation(value, visit);
        foreach (var value in operations.FieldValues) VisitOperation(value, visit);
        foreach (var value in operations.NativeCalls) VisitOperation(value, visit);
        foreach (var value in operations.Collections) VisitOperation(value, visit);
        foreach (var value in operations.Indexes) VisitOperation(value, visit);
        foreach (var value in operations.CollectionReads) VisitOperation(value, visit);
        foreach (var value in operations.Projections) VisitOperation(value, visit);
        foreach (var value in operations.CollectionBuiltins) VisitOperation(value, visit);
        foreach (var value in operations.ArrayBuilders) VisitOperation(value, visit);
        foreach (var value in operations.Targets) VisitOperation(value, visit);
        foreach (var value in operations.Propagates) VisitOperation(value, visit);
        foreach (var value in operations.Closures) VisitOperation(value, visit);
        foreach (var value in operations.Calls) VisitOperation(value, visit);
        foreach (var value in operations.IndirectCalls) VisitOperation(value, visit);
    }

    private static void MapOperationRegisters(
        CoflowRegisterOperations operations,
        Func<CoflowValueRegister, CoflowValueRegister> map)
    {
        MapArray(operations.Constants, map);
        MapArray(operations.Transfers, map);
        MapArray(operations.FieldValues, map);
        MapArray(operations.NativeCalls, map);
        MapArray(operations.Collections, map);
        MapArray(operations.Indexes, map);
        MapArray(operations.CollectionReads, map);
        MapArray(operations.Projections, map);
        MapArray(operations.CollectionBuiltins, map);
        MapArray(operations.ArrayBuilders, map);
        MapArray(operations.Targets, map);
        MapArray(operations.Propagates, map);
        MapArray(operations.Closures, map);
        for (var index = 0; index < operations.Calls.Length; index++)
            operations.Calls[index] = MapCallSite(operations.Calls[index], map);
        MapArray(operations.IndirectCalls, map);
    }

    private static void MapArray<T>(T[] values,
        Func<CoflowValueRegister, CoflowValueRegister> map) where T : class
    {
        for (var index = 0; index < values.Length; index++)
            values[index] = (T)MapOperation(values[index], map)!;
    }

    private static void MarkRange(bool[] used, int start, int count)
    {
        for (var index = 0; index < count; index++) used[start + index] = true;
    }

    private static int[] DenseMap(bool[] used, out int count)
    {
        var result = new int[used.Length];
        count = 0;
        for (var index = 0; index < used.Length; index++)
            result[index] = used[index] ? count++ : -1;
        return result;
    }

    private static int MapBase(int[] map, int value, int count) => count == 0 ? 0 : map[value];

    private static void MarkOperand(
        int value,
        CoflowRegisterKind? kind,
        bool[] integers,
        bool[] floats,
        bool[] references)
    {
        if (kind is null) return;
        (kind == CoflowRegisterKind.Integer ? integers :
            kind == CoflowRegisterKind.Float ? floats : references)[value] = true;
    }

    private static int MapOperand(
        int value,
        CoflowRegisterKind? kind,
        int[] integers,
        int[] floats,
        int[] references) => kind switch
        {
            CoflowRegisterKind.Integer => integers[value],
            CoflowRegisterKind.Float => floats[value],
            CoflowRegisterKind.Reference => references[value],
            _ => value,
        };

    private static CoflowRegisterKind? OperandKind(CoflowRegisterOpCode code, int operand)
    {
        if (operand == 0)
        {
            if (code is CoflowRegisterOpCode.ConstantInteger or CoflowRegisterOpCode.MoveInteger or
                CoflowRegisterOpCode.LoadHostFieldInteger or CoflowRegisterOpCode.LoadArenaFieldInteger or
                CoflowRegisterOpCode.LoadArenaFieldValue or CoflowRegisterOpCode.ReadValueTag or
                CoflowRegisterOpCode.ConvertFloatToInt or CoflowRegisterOpCode.IsType or CoflowRegisterOpCode.IsArenaType or
                CoflowRegisterOpCode.NegateInt or CoflowRegisterOpCode.Not or CoflowRegisterOpCode.BitNot or
                CoflowRegisterOpCode.AddInt or CoflowRegisterOpCode.SubtractInt or
                CoflowRegisterOpCode.MultiplyInt or CoflowRegisterOpCode.DivideInt or
                CoflowRegisterOpCode.IntegerDivide or CoflowRegisterOpCode.Remainder or
                CoflowRegisterOpCode.PowerInt or CoflowRegisterOpCode.ShiftLeft or
                CoflowRegisterOpCode.ShiftRight or CoflowRegisterOpCode.BitAnd or
                CoflowRegisterOpCode.BitXor or CoflowRegisterOpCode.BitOr or
                CoflowRegisterOpCode.LessInt or CoflowRegisterOpCode.LessOrEqualInt or
                CoflowRegisterOpCode.GreaterInt or CoflowRegisterOpCode.GreaterOrEqualInt or
                CoflowRegisterOpCode.EqualInteger or CoflowRegisterOpCode.LessFloat or
                CoflowRegisterOpCode.LessOrEqualFloat or CoflowRegisterOpCode.GreaterFloat or
                CoflowRegisterOpCode.GreaterOrEqualFloat or CoflowRegisterOpCode.EqualFloat or
                CoflowRegisterOpCode.LessString or CoflowRegisterOpCode.LessOrEqualString or
                CoflowRegisterOpCode.GreaterString or CoflowRegisterOpCode.GreaterOrEqualString or
                CoflowRegisterOpCode.EqualReference or CoflowRegisterOpCode.JumpIfFalse or
                CoflowRegisterOpCode.JumpIfTrue) return CoflowRegisterKind.Integer;
            if (code is CoflowRegisterOpCode.ConstantFloat or CoflowRegisterOpCode.MoveFloat or
                CoflowRegisterOpCode.LoadHostFieldFloat or CoflowRegisterOpCode.LoadArenaFieldFloat or CoflowRegisterOpCode.ConvertIntToFloat or
                CoflowRegisterOpCode.NegateFloat or CoflowRegisterOpCode.AddFloat or
                CoflowRegisterOpCode.SubtractFloat or CoflowRegisterOpCode.MultiplyFloat or
                CoflowRegisterOpCode.DivideFloat or CoflowRegisterOpCode.PowerFloat)
                return CoflowRegisterKind.Float;
            if (code is CoflowRegisterOpCode.ConstantReference or CoflowRegisterOpCode.MoveReference or
                CoflowRegisterOpCode.LoadHostFieldReference or CoflowRegisterOpCode.LoadHostFieldValue or
                CoflowRegisterOpCode.LoadArenaFieldReference or
                CoflowRegisterOpCode.AddString)
                return CoflowRegisterKind.Reference;
            return null;
        }
        if (operand == 1)
        {
            if (code is CoflowRegisterOpCode.MoveInteger or CoflowRegisterOpCode.LoadArenaFieldInteger or
                CoflowRegisterOpCode.LoadArenaFieldFloat or CoflowRegisterOpCode.LoadArenaFieldReference or
                CoflowRegisterOpCode.IsArenaType or
                CoflowRegisterOpCode.ReadValueTag or
                CoflowRegisterOpCode.ConvertIntToFloat or CoflowRegisterOpCode.NegateInt or
                CoflowRegisterOpCode.Not or CoflowRegisterOpCode.BitNot or
                CoflowRegisterOpCode.AddInt or CoflowRegisterOpCode.SubtractInt or
                CoflowRegisterOpCode.MultiplyInt or CoflowRegisterOpCode.DivideInt or
                CoflowRegisterOpCode.IntegerDivide or CoflowRegisterOpCode.Remainder or
                CoflowRegisterOpCode.PowerInt or CoflowRegisterOpCode.ShiftLeft or
                CoflowRegisterOpCode.ShiftRight or CoflowRegisterOpCode.BitAnd or
                CoflowRegisterOpCode.BitXor or CoflowRegisterOpCode.BitOr or
                CoflowRegisterOpCode.LessInt or CoflowRegisterOpCode.LessOrEqualInt or
                CoflowRegisterOpCode.GreaterInt or CoflowRegisterOpCode.GreaterOrEqualInt or
                CoflowRegisterOpCode.EqualInteger) return CoflowRegisterKind.Integer;
            if (code is CoflowRegisterOpCode.MoveFloat or CoflowRegisterOpCode.ConvertFloatToInt or
                CoflowRegisterOpCode.NegateFloat or CoflowRegisterOpCode.AddFloat or
                CoflowRegisterOpCode.SubtractFloat or CoflowRegisterOpCode.MultiplyFloat or
                CoflowRegisterOpCode.DivideFloat or CoflowRegisterOpCode.PowerFloat or
                CoflowRegisterOpCode.LessFloat or CoflowRegisterOpCode.LessOrEqualFloat or
                CoflowRegisterOpCode.GreaterFloat or CoflowRegisterOpCode.GreaterOrEqualFloat or
                CoflowRegisterOpCode.EqualFloat) return CoflowRegisterKind.Float;
            if (code is CoflowRegisterOpCode.LoadHostFieldInteger or CoflowRegisterOpCode.LoadHostFieldFloat or
                CoflowRegisterOpCode.LoadHostFieldReference or CoflowRegisterOpCode.MoveReference or
                CoflowRegisterOpCode.IsType or CoflowRegisterOpCode.AddString or
                CoflowRegisterOpCode.LessString or CoflowRegisterOpCode.LessOrEqualString or
                CoflowRegisterOpCode.GreaterString or CoflowRegisterOpCode.GreaterOrEqualString or
                CoflowRegisterOpCode.EqualReference) return CoflowRegisterKind.Reference;
            return null;
        }
        if (code is CoflowRegisterOpCode.AddInt or CoflowRegisterOpCode.SubtractInt or
            CoflowRegisterOpCode.MultiplyInt or CoflowRegisterOpCode.DivideInt or
            CoflowRegisterOpCode.IntegerDivide or CoflowRegisterOpCode.Remainder or
            CoflowRegisterOpCode.PowerInt or CoflowRegisterOpCode.ShiftLeft or
            CoflowRegisterOpCode.ShiftRight or CoflowRegisterOpCode.BitAnd or
            CoflowRegisterOpCode.BitXor or CoflowRegisterOpCode.BitOr or
            CoflowRegisterOpCode.LessInt or CoflowRegisterOpCode.LessOrEqualInt or
            CoflowRegisterOpCode.GreaterInt or CoflowRegisterOpCode.GreaterOrEqualInt or
            CoflowRegisterOpCode.EqualInteger) return CoflowRegisterKind.Integer;
        if (code is CoflowRegisterOpCode.AddFloat or CoflowRegisterOpCode.SubtractFloat or
            CoflowRegisterOpCode.MultiplyFloat or CoflowRegisterOpCode.DivideFloat or
            CoflowRegisterOpCode.PowerFloat or CoflowRegisterOpCode.LessFloat or
            CoflowRegisterOpCode.LessOrEqualFloat or CoflowRegisterOpCode.GreaterFloat or
            CoflowRegisterOpCode.GreaterOrEqualFloat or CoflowRegisterOpCode.EqualFloat)
            return CoflowRegisterKind.Float;
        if (code is CoflowRegisterOpCode.AddString or CoflowRegisterOpCode.LessString or
            CoflowRegisterOpCode.LessOrEqualString or CoflowRegisterOpCode.GreaterString or
            CoflowRegisterOpCode.GreaterOrEqualString or CoflowRegisterOpCode.EqualReference)
            return CoflowRegisterKind.Reference;
        return null;
    }

    private static void VisitOperation(object? operation, Action<CoflowValueRegister> visit)
    {
        switch (operation)
        {
            case CoflowRegisterValueTransfer value: visit(value.Source); visit(value.Target); break;
            case CoflowRegisterCollectionSite value:
                foreach (var collectionItem in value.First) visit(collectionItem);
                if (value.Second is { } second)
                    foreach (var collectionItem in second) visit(collectionItem);
                visit(value.Target); break;
            case CoflowRegisterArrayIndexSite value:
                visit(value.Collection); visit(value.Index); visit(value.Target); break;
            case CoflowRegisterCollectionReadSite value:
                visit(value.Collection);
                if (value.Index is { } itemIndex) visit(itemIndex);
                visit(value.Target); break;
            case CoflowRegisterCollectionProjectionSite value:
                visit(value.Source); visit(value.Target); break;
            case CoflowRegisterCollectionBuiltinSite value:
                visit(value.Receiver);
                if (value.Argument is { } builtinArgument) visit(builtinArgument);
                visit(value.Target); break;
            case CoflowRegisterArrayBuilderSite value:
                visit(value.CollectionOrCapacity);
                if (value.Item is { } appendItem) visit(appendItem);
                visit(value.Target); break;
            case CoflowRegisterConstantSite value: visit(value.Target); break;
            case CoflowRegisterFieldValueSite value: visit(value.Target); break;
            case CoflowRegisterTargetSite value: visit(value.Target); break;
            case CoflowRegisterPropagateSite value:
                visit(value.Source); visit(value.Payload); visit(value.ReturnValue); break;
            case CoflowRegisterClosureSite value:
                foreach (var capture in value.Captures) visit(capture);
                visit(value.Target); break;
            case CoflowRegisterCallSite value:
                for (var index = 0; index < value.SourceArguments.Length; index++)
                    if (value.CopyArguments[index]) visit(value.SourceArguments[index]);
                foreach (var argument in value.Arguments) visit(argument);
                visit(value.Result); break;
            case CoflowRegisterIndirectCallSite value:
                visit(value.Callable);
                foreach (var argument in value.Arguments) visit(argument);
                visit(value.Result); break;
            case CoflowNativeCallSite value:
                foreach (var argument in value.Arguments) visit(argument);
                visit(value.Result); break;
        }
    }

    private static object? MapOperation(
        object? operation,
        Func<CoflowValueRegister, CoflowValueRegister> map) => operation switch
        {
            CoflowRegisterValueTransfer value => new CoflowRegisterValueTransfer(
                map(value.Source), map(value.Target)),
            CoflowRegisterCollectionSite value => new CoflowRegisterCollectionSite(
                value.First.Select(map).ToArray(), value.Second?.Select(map).ToArray(), map(value.Target)),
            CoflowRegisterArrayIndexSite value => new CoflowRegisterArrayIndexSite(
                map(value.Collection), map(value.Index), map(value.Target)),
            CoflowRegisterCollectionReadSite value => new CoflowRegisterCollectionReadSite(
                map(value.Collection), value.Index is { } index ? map(index) : null, map(value.Target)),
            CoflowRegisterCollectionProjectionSite value => new CoflowRegisterCollectionProjectionSite(
                map(value.Source), map(value.Target)),
            CoflowRegisterCollectionBuiltinSite value => new CoflowRegisterCollectionBuiltinSite(
                value.Builtin, map(value.Receiver),
                value.Argument is { } builtinArgument ? map(builtinArgument) : null,
                map(value.Target)),
            CoflowRegisterArrayBuilderSite value => new CoflowRegisterArrayBuilderSite(
                map(value.CollectionOrCapacity), value.Item is { } item ? map(item) : null,
                map(value.Target)),
            CoflowRegisterConstantSite value => new CoflowRegisterConstantSite(value.Value, map(value.Target)),
            CoflowRegisterFieldValueSite value => new CoflowRegisterFieldValueSite(
                value.Access, map(value.Target)),
            CoflowRegisterTargetSite value => new CoflowRegisterTargetSite(map(value.Target)),
            CoflowRegisterPropagateSite value => new CoflowRegisterPropagateSite(
                map(value.Source), map(value.Payload), map(value.ReturnValue)),
            CoflowRegisterClosureSite value => new CoflowRegisterClosureSite(
                value.Template, value.Captures.Select(map).ToArray(), map(value.Target)),
            CoflowRegisterCallSite value => MapCallSite(value, map),
            CoflowRegisterIndirectCallSite value => new CoflowRegisterIndirectCallSite(
                map(value.Callable), value.Arguments.Select(map).ToArray(), map(value.Result), value.ResultType),
            CoflowNativeCallSite value => new CoflowNativeCallSite(
                value.Call, value.Arguments.Select(map).ToArray(), map(value.Result)),
            _ => operation,
        };

    private static CoflowRegisterCallSite MapCallSite(
        CoflowRegisterCallSite value,
        Func<CoflowValueRegister, CoflowValueRegister> map)
    {
        var arguments = value.Arguments.Select(map).ToArray();
        var sourceArguments = value.SourceArguments.Select((argument, index) =>
            value.CopyArguments[index] ? map(argument) : arguments[index]).ToArray();
        return new CoflowRegisterCallSite(
            value.ProgramIndex,
            value.Signature,
            sourceArguments,
            arguments,
            value.CopyArguments.ToArray(),
            map(value.Result),
            arguments.Where(argument => argument.Shape.IntegerCount != 0)
                .Select(argument => argument.IntegerBase).DefaultIfEmpty(-1).Min(),
            arguments.Where(argument => argument.Shape.FloatCount != 0)
                .Select(argument => argument.FloatBase).DefaultIfEmpty(-1).Min(),
            arguments.Where(argument => argument.Shape.ReferenceCount != 0)
                .Select(argument => argument.ReferenceBase).DefaultIfEmpty(-1).Min());
    }

    private static CoflowValueRegister Allocate(
        Type type,
        ref int integer,
        ref int floating,
        ref int reference)
    {
        var result = new CoflowValueRegister(CoflowValueShape.Of(type), integer, floating, reference);
        integer += result.Shape.IntegerCount;
        floating += result.Shape.FloatCount;
        reference += result.Shape.ReferenceCount;
        return result;
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

    private static (
        CoflowRegisterInstruction[] Instructions,
        CfdSpan?[] InstructionSpans,
        long[] Immediates,
        CoflowRegisterOperations Operations) LowerInstructions(
        CoflowLoweringInput program,
        Type[][] states,
        CoflowValueRegister[] parameters,
        CoflowValueRegister[] locals,
        CoflowValueRegister returnValue,
        CoflowValueRegister[] temporaries,
        int outgoingIntegerBase,
        int outgoingFloatBase,
        int outgoingReferenceBase)
    {
        var result = new CoflowLoweredInstruction[program.Instructions.Length];
        for (var pc = 0; pc < result.Length; pc++)
        {
            var source = program.Instructions[pc];
            var stack = states[pc];
            var depth = stack.Length;

            CoflowValueRegister Temporary(Type type, int index)
            {
                var storage = temporaries[index];
                return new CoflowValueRegister(
                    CoflowValueShape.Of(type),
                    storage.IntegerBase,
                    storage.FloatBase,
                    storage.ReferenceBase);
            }
            CoflowValueRegister Stack(int index) => Temporary(stack[index], index);
            CoflowValueRegister Top(int offset = 1) => Stack(depth - offset);
            CoflowValueRegister Output(Type type, int consumed) => Temporary(type, depth - consumed);
            object Operation() => program.Operations[source.Operand]
                ?? throw Invalid(program, $"instruction {pc} has no operation descriptor");
            CoflowValueRegister[] Arguments(int count)
            {
                var start = depth - count;
                var arguments = new CoflowValueRegister[count];
                for (var index = 0; index < count; index++)
                    arguments[index] = Stack(start + index);
                return arguments;
            }
            CoflowLoweredInstruction LowerCollectionBuiltin(CoflowBuiltin builtin, Type resultType)
            {
                var consumed = builtin.HasCollectionArgument ? 2 : 1;
                return new CoflowLoweredInstruction(CoflowRegisterOpCode.CollectionBuiltin,
                    Operation: new CoflowRegisterCollectionBuiltinSite(
                        builtin,
                        builtin.HasCollectionArgument ? Top(2) : Top(),
                        builtin.HasCollectionArgument ? Top() : null,
                        Output(resultType, consumed)));
            }

            var valueType = source.Code switch
            {
                CoflowOpCode.Constant => program.EncodedConstants[source.Operand]?.Shape.Type ?? typeof(object),
                CoflowOpCode.Argument => program.ParameterTypes[source.Operand],
                CoflowOpCode.Local => locals[source.Operand].Shape.Type,
                CoflowOpCode.LoadField => ((CoflowFieldAccess)Operation()).RuntimeType,
                CoflowOpCode.Native => ((CoflowNativeCall)Operation()).ResultType,
                CoflowOpCode.Call => ((CoflowCallSite)Operation()).Signature.ResultType,
                CoflowOpCode.TailCall or CoflowOpCode.TailCallIndirect => program.ReturnType,
                _ => source.ValueType ?? typeof(object),
            };
            result[pc] = source.Code switch
            {
                CoflowOpCode.Constant => Constant(
                    program.EncodedConstants[source.Operand]
                        ?? throw Invalid(program, $"instruction {pc} has no encoded constant"),
                    Output(valueType, 0)),
                CoflowOpCode.Argument => Move(parameters[source.Operand], Output(valueType, 0)),
                CoflowOpCode.Local => Move(locals[source.Operand], Output(valueType, 0)),
                CoflowOpCode.StoreLocal => Move(Top(), locals[source.Operand]),
                CoflowOpCode.LoadField => Field(
                    (CoflowFieldAccess)Operation(), Top(), Output(valueType, 1)),
                CoflowOpCode.Native => Native(
                    (CoflowNativeCall)Operation(),
                    Arguments(((CoflowNativeCall)Operation()).ArgumentCount),
                    Output(valueType, ((CoflowNativeCall)Operation()).ArgumentCount)),
                CoflowOpCode.MakeArray => Collection(
                    CoflowRegisterOpCode.MakeArray, Arguments(source.Operand), null,
                    Output(valueType, source.Operand)),
                CoflowOpCode.MakeDictionary => Dictionary(
                    Arguments(checked(source.Operand * 2)),
                    Output(valueType, checked(source.Operand * 2))),
                CoflowOpCode.ArrayIndex or CoflowOpCode.DictionaryIndex => new(
                    source.Code == CoflowOpCode.ArrayIndex
                        ? CoflowRegisterOpCode.ArrayIndex : CoflowRegisterOpCode.DictionaryIndex,
                    Operation: new CoflowRegisterArrayIndexSite(
                        Top(2), Top(), Output(valueType, 2))),
                CoflowOpCode.CollectionCount => CollectionRead(
                    CoflowRegisterOpCode.CollectionCount, Top(), null, Output(valueType, 1)),
                CoflowOpCode.ArrayItem => CollectionRead(
                    CoflowRegisterOpCode.ArrayItem, Top(2), Top(), Output(valueType, 2)),
                CoflowOpCode.DictionaryKey => CollectionRead(
                    CoflowRegisterOpCode.DictionaryKey, Top(2), Top(), Output(valueType, 2)),
                CoflowOpCode.DictionaryValue => CollectionRead(
                    CoflowRegisterOpCode.DictionaryValue, Top(2), Top(), Output(valueType, 2)),
                CoflowOpCode.DictionaryKeys or CoflowOpCode.DictionaryValues => new(
                    source.Code == CoflowOpCode.DictionaryKeys
                        ? CoflowRegisterOpCode.DictionaryKeys : CoflowRegisterOpCode.DictionaryValues,
                    Operation: new CoflowRegisterCollectionProjectionSite(
                        Top(), Output(valueType, 1))),
                CoflowOpCode.CollectionBuiltin => LowerCollectionBuiltin(
                    (CoflowBuiltin)Operation(), valueType),
                CoflowOpCode.BeginArrayBuilder => ArrayBuilder(
                    CoflowRegisterOpCode.BeginArrayBuilder, Top(), null, Output(valueType, 1)),
                CoflowOpCode.AppendArrayBuilder => ArrayBuilder(
                    CoflowRegisterOpCode.AppendArrayBuilder, Top(2), Top(), Output(valueType, 2)),
                CoflowOpCode.MakeOptionNone => new(
                    CoflowRegisterOpCode.MakeOptionNone,
                    Operation: new CoflowRegisterTargetSite(Output(valueType, 0))),
                CoflowOpCode.MakeOptionSome => Transfer(
                    CoflowRegisterOpCode.MakeOptionSome, Top(), Output(valueType, 1)),
                CoflowOpCode.MakeResultOk => Transfer(
                    CoflowRegisterOpCode.MakeResultOk, Top(), Output(valueType, 1)),
                CoflowOpCode.MakeResultErr => Transfer(
                    CoflowRegisterOpCode.MakeResultErr, Top(), Output(valueType, 1)),
                CoflowOpCode.ReadValueTag => new(
                    CoflowRegisterOpCode.ReadValueTag,
                    Output(typeof(bool), 1).IntegerBase,
                    Top().IntegerBase),
                CoflowOpCode.ReadFirstPayload => Transfer(
                    CoflowRegisterOpCode.ReadFirstPayload, Top().First, Output(valueType, 1)),
                CoflowOpCode.ReadSecondPayload => Transfer(
                    CoflowRegisterOpCode.ReadSecondPayload, Top().Second, Output(valueType, 1)),
                CoflowOpCode.Propagate => new(
                    CoflowRegisterOpCode.Propagate,
                    Operation: new CoflowRegisterPropagateSite(
                        Top(), Output(valueType, 1), returnValue)),
                CoflowOpCode.MakeClosure => Closure(
                    (CoflowClosureTemplate)Operation(),
                    Arguments(((CoflowClosureTemplate)Operation()).CaptureCount),
                    Output(valueType, ((CoflowClosureTemplate)Operation()).CaptureCount)),
                CoflowOpCode.Pop => new(CoflowRegisterOpCode.Nop),
                CoflowOpCode.Reinterpret => Transfer(
                    CoflowRegisterOpCode.MoveValue, Top(), Output(valueType, 1)),
                CoflowOpCode.ConvertIntToFloat => new(
                    CoflowRegisterOpCode.ConvertIntToFloat,
                    Output(typeof(double), 1).FloatBase,
                    Top().IntegerBase),
                CoflowOpCode.ConvertFloatToInt => new(
                    CoflowRegisterOpCode.ConvertFloatToInt,
                    Output(typeof(long), 1).IntegerBase,
                    Top().FloatBase),
                CoflowOpCode.IsType => new(
                    Top().Shape.Kind == CoflowValueShapeKind.Record
                        ? CoflowRegisterOpCode.IsArenaType : CoflowRegisterOpCode.IsType,
                    Output(typeof(bool), 1).IntegerBase,
                    Top().Shape.Kind == CoflowValueShapeKind.Record
                        ? Top().IntegerBase : Top().ReferenceBase,
                    Operation: (Type)Operation()),
                CoflowOpCode.NegateInt or CoflowOpCode.Not or CoflowOpCode.BitNot => IntegerUnary(
                    source.Code, Top().IntegerBase, Output(valueType, 1).IntegerBase),
                CoflowOpCode.NegateFloat => new(
                    CoflowRegisterOpCode.NegateFloat,
                    Output(valueType, 1).FloatBase,
                    Top().FloatBase),
                CoflowOpCode.AddInt or CoflowOpCode.SubtractInt or CoflowOpCode.MultiplyInt or
                CoflowOpCode.DivideInt or CoflowOpCode.IntegerDivide or CoflowOpCode.Remainder or
                CoflowOpCode.PowerInt or CoflowOpCode.ShiftLeft or CoflowOpCode.ShiftRight or
                CoflowOpCode.BitAnd or CoflowOpCode.BitXor or CoflowOpCode.BitOr => IntegerBinary(
                    source.Code,
                    Output(valueType, 2).IntegerBase,
                    Top(2).IntegerBase,
                    Top().IntegerBase),
                CoflowOpCode.AddFloat or CoflowOpCode.SubtractFloat or CoflowOpCode.MultiplyFloat or
                CoflowOpCode.DivideFloat or CoflowOpCode.PowerFloat => FloatBinary(
                    source.Code,
                    Output(valueType, 2).FloatBase,
                    Top(2).FloatBase,
                    Top().FloatBase),
                CoflowOpCode.AddString => new(
                    CoflowRegisterOpCode.AddString,
                    Output(typeof(string), 2).ReferenceBase,
                    Top(2).ReferenceBase,
                    Top().ReferenceBase),
                CoflowOpCode.LessInt or CoflowOpCode.LessOrEqualInt or CoflowOpCode.GreaterInt or
                CoflowOpCode.GreaterOrEqualInt or CoflowOpCode.EqualInteger => IntegerComparison(
                    source.Code,
                    Output(typeof(bool), 2).IntegerBase,
                    Top(2).IntegerBase,
                    Top().IntegerBase),
                CoflowOpCode.LessFloat or CoflowOpCode.LessOrEqualFloat or CoflowOpCode.GreaterFloat or
                CoflowOpCode.GreaterOrEqualFloat or CoflowOpCode.EqualFloat => FloatComparison(
                    source.Code,
                    Output(typeof(bool), 2).IntegerBase,
                    Top(2).FloatBase,
                    Top().FloatBase),
                CoflowOpCode.LessString or CoflowOpCode.LessOrEqualString or CoflowOpCode.GreaterString or
                CoflowOpCode.GreaterOrEqualString => StringComparison(
                    source.Code,
                    Output(typeof(bool), 2).IntegerBase,
                    Top(2).ReferenceBase,
                    Top().ReferenceBase),
                CoflowOpCode.EqualReference => new(
                    CoflowRegisterOpCode.EqualReference,
                    Output(typeof(bool), 2).IntegerBase,
                    Top(2).ReferenceBase,
                    Top().ReferenceBase),
                CoflowOpCode.JumpIfFalseKeep or CoflowOpCode.JumpIfFalse => new(
                    CoflowRegisterOpCode.JumpIfFalse, Top().IntegerBase, source.Operand),
                CoflowOpCode.JumpIfTrueKeep => new(
                    CoflowRegisterOpCode.JumpIfTrue, Top().IntegerBase, source.Operand),
                CoflowOpCode.Jump => new(CoflowRegisterOpCode.Jump, source.Operand),
                CoflowOpCode.Call => DirectCall(
                    CoflowRegisterOpCode.Call,
                    (CoflowCallSite)Operation(),
                    ResolveProgramIndex(program, (CoflowCallSite)Operation(), pc),
                    Arguments(((CoflowCallSite)Operation()).ArgumentCount),
                    Output(((CoflowCallSite)Operation()).Signature.ResultType,
                        ((CoflowCallSite)Operation()).ArgumentCount),
                    outgoingIntegerBase,
                    outgoingFloatBase,
                    outgoingReferenceBase),
                CoflowOpCode.CallIndirect => IndirectCall(
                    CoflowRegisterOpCode.CallIndirect,
                    Stack(depth - source.Operand - 1),
                    Arguments(source.Operand),
                    Output(valueType, source.Operand + 1),
                    valueType),
                CoflowOpCode.TailCall => DirectCall(
                    CoflowRegisterOpCode.TailCall,
                    (CoflowCallSite)Operation(),
                    ResolveProgramIndex(program, (CoflowCallSite)Operation(), pc),
                    Arguments(((CoflowCallSite)Operation()).ArgumentCount),
                    returnValue,
                    outgoingIntegerBase,
                    outgoingFloatBase,
                    outgoingReferenceBase),
                CoflowOpCode.TailCallIndirect => IndirectCall(
                    CoflowRegisterOpCode.TailCallIndirect,
                    Stack(depth - source.Operand - 1),
                    Arguments(source.Operand),
                    returnValue,
                    program.ReturnType),
                CoflowOpCode.Return => new(
                    CoflowRegisterOpCode.Return,
                    Operation: new CoflowRegisterTargetSite(Top())),
                _ => throw Invalid(program, $"unknown opcode `{source.Code}`"),
            };
        }
        PlaceDirectCallArguments(result);
        OptimizeScalarMoves(result);
        return Compact(result, program.InstructionSpans);
    }

    private static void PlaceDirectCallArguments(CoflowLoweredInstruction[] instructions)
    {
        for (var callIndex = 0; callIndex < instructions.Length; callIndex++)
        {
            if (instructions[callIndex].Operation is not CoflowRegisterCallSite site) continue;
            for (var argumentIndex = 0; argumentIndex < site.SourceArguments.Length; argumentIndex++)
            {
                var source = site.SourceArguments[argumentIndex];
                var target = site.Arguments[argumentIndex];
                if (source.Shape.Kind != CoflowValueShapeKind.Scalar) continue;
                var kind = source.Shape.ScalarKind!.Value;
                var sourceIndex = kind switch
                {
                    CoflowRegisterKind.Integer => source.IntegerBase,
                    CoflowRegisterKind.Float => source.FloatBase,
                    _ => source.ReferenceBase,
                };
                for (var scan = callIndex - 1; scan >= 0; scan--)
                {
                    var instruction = instructions[scan];
                    if (IsControlBoundary(instruction.Code) || instruction.Operation is not null) break;
                    if (WrittenRegister(instruction.Code, kind, instruction.A) == sourceIndex)
                    {
                        var targetIndex = kind switch
                        {
                            CoflowRegisterKind.Integer => target.IntegerBase,
                            CoflowRegisterKind.Float => target.FloatBase,
                            _ => target.ReferenceBase,
                        };
                        instructions[scan] = instruction with { A = targetIndex };
                        site.CopyArguments[argumentIndex] = false;
                        break;
                    }
                    if (ReadsRegister(instruction, kind, sourceIndex)) break;
                }
            }
        }
    }

    private static bool ReadsRegister(
        CoflowLoweredInstruction instruction,
        CoflowRegisterKind kind,
        int register) => RewriteReads(instruction, kind, register, int.MinValue) != instruction;

    private static void OptimizeScalarMoves(CoflowLoweredInstruction[] instructions)
    {
        bool changed;
        do
        {
            changed = false;
            for (var index = 0; index < instructions.Length; index++)
            {
                var move = instructions[index];
                var kind = move.Code switch
                {
                    CoflowRegisterOpCode.MoveInteger => CoflowRegisterKind.Integer,
                    CoflowRegisterOpCode.MoveFloat => CoflowRegisterKind.Float,
                    CoflowRegisterOpCode.MoveReference => CoflowRegisterKind.Reference,
                    _ => (CoflowRegisterKind?)null,
                };
                if (kind is null) continue;
                if (move.A == move.B)
                {
                    instructions[index] = new(CoflowRegisterOpCode.Nop);
                    changed = true;
                    continue;
                }

                var rewrites = new List<(int Index, CoflowLoweredInstruction Instruction)>();
                var eliminated = false;
                for (var scan = index + 1; scan < instructions.Length; scan++)
                {
                    var instruction = instructions[scan];
                    if (instruction.Operation is not null || IsControlBoundary(instruction.Code)) break;
                    var rewritten = RewriteReads(instruction, kind.Value, move.A, move.B);
                    if (rewritten != instruction) rewrites.Add((scan, rewritten));

                    var written = WrittenRegister(instruction.Code, kind.Value, instruction.A);
                    if (written == move.B) break;
                    if (written != move.A) continue;
                    eliminated = true;
                    break;
                }
                if (!eliminated) continue;
                foreach (var rewrite in rewrites) instructions[rewrite.Index] = rewrite.Instruction;
                instructions[index] = new(CoflowRegisterOpCode.Nop);
                changed = true;
            }
        } while (changed);
    }

    private static bool IsControlBoundary(CoflowRegisterOpCode code) => code is
        CoflowRegisterOpCode.JumpIfFalse or CoflowRegisterOpCode.JumpIfTrue or
        CoflowRegisterOpCode.Jump or CoflowRegisterOpCode.Call or
        CoflowRegisterOpCode.CallIndirect or CoflowRegisterOpCode.TailCall or
        CoflowRegisterOpCode.TailCallIndirect or CoflowRegisterOpCode.Return;

    private static int WrittenRegister(
        CoflowRegisterOpCode code,
        CoflowRegisterKind kind,
        int target) => kind switch
        {
            CoflowRegisterKind.Integer when code is
                CoflowRegisterOpCode.ConstantInteger or CoflowRegisterOpCode.MoveInteger or
                CoflowRegisterOpCode.LoadHostFieldInteger or CoflowRegisterOpCode.LoadArenaFieldInteger or
                CoflowRegisterOpCode.LoadArenaFieldValue or CoflowRegisterOpCode.ReadValueTag or
                CoflowRegisterOpCode.ConvertFloatToInt or CoflowRegisterOpCode.IsType or CoflowRegisterOpCode.IsArenaType or
                CoflowRegisterOpCode.NegateInt or CoflowRegisterOpCode.Not or CoflowRegisterOpCode.BitNot or
                CoflowRegisterOpCode.AddInt or CoflowRegisterOpCode.SubtractInt or
                CoflowRegisterOpCode.MultiplyInt or CoflowRegisterOpCode.DivideInt or
                CoflowRegisterOpCode.IntegerDivide or CoflowRegisterOpCode.Remainder or
                CoflowRegisterOpCode.PowerInt or CoflowRegisterOpCode.ShiftLeft or
                CoflowRegisterOpCode.ShiftRight or CoflowRegisterOpCode.BitAnd or
                CoflowRegisterOpCode.BitXor or CoflowRegisterOpCode.BitOr or
                CoflowRegisterOpCode.LessInt or CoflowRegisterOpCode.LessOrEqualInt or
                CoflowRegisterOpCode.GreaterInt or CoflowRegisterOpCode.GreaterOrEqualInt or
                CoflowRegisterOpCode.EqualInteger or CoflowRegisterOpCode.LessFloat or
                CoflowRegisterOpCode.LessOrEqualFloat or CoflowRegisterOpCode.GreaterFloat or
                CoflowRegisterOpCode.GreaterOrEqualFloat or CoflowRegisterOpCode.EqualFloat or
                CoflowRegisterOpCode.LessString or CoflowRegisterOpCode.LessOrEqualString or
                CoflowRegisterOpCode.GreaterString or CoflowRegisterOpCode.GreaterOrEqualString or
                CoflowRegisterOpCode.EqualReference => target,
            CoflowRegisterKind.Float when code is
                CoflowRegisterOpCode.ConstantFloat or CoflowRegisterOpCode.MoveFloat or
                CoflowRegisterOpCode.LoadHostFieldFloat or CoflowRegisterOpCode.LoadArenaFieldFloat or CoflowRegisterOpCode.ConvertIntToFloat or
                CoflowRegisterOpCode.NegateFloat or CoflowRegisterOpCode.AddFloat or
                CoflowRegisterOpCode.SubtractFloat or CoflowRegisterOpCode.MultiplyFloat or
                CoflowRegisterOpCode.DivideFloat or CoflowRegisterOpCode.PowerFloat => target,
            CoflowRegisterKind.Reference when code is
                CoflowRegisterOpCode.ConstantReference or CoflowRegisterOpCode.MoveReference or
                CoflowRegisterOpCode.LoadHostFieldReference or CoflowRegisterOpCode.LoadArenaFieldReference or CoflowRegisterOpCode.AddString => target,
            _ => -1,
        };

    private static CoflowLoweredInstruction RewriteReads(
        CoflowLoweredInstruction instruction,
        CoflowRegisterKind kind,
        int target,
        int source)
    {
        var readB = kind switch
        {
            CoflowRegisterKind.Integer => instruction.Code is
                CoflowRegisterOpCode.MoveInteger or CoflowRegisterOpCode.LoadArenaFieldInteger or
                CoflowRegisterOpCode.LoadArenaFieldFloat or CoflowRegisterOpCode.LoadArenaFieldReference or
                CoflowRegisterOpCode.IsArenaType or CoflowRegisterOpCode.ReadValueTag or
                CoflowRegisterOpCode.ConvertIntToFloat or CoflowRegisterOpCode.NegateInt or
                CoflowRegisterOpCode.Not or CoflowRegisterOpCode.BitNot or
                CoflowRegisterOpCode.AddInt or CoflowRegisterOpCode.SubtractInt or
                CoflowRegisterOpCode.MultiplyInt or CoflowRegisterOpCode.DivideInt or
                CoflowRegisterOpCode.IntegerDivide or CoflowRegisterOpCode.Remainder or
                CoflowRegisterOpCode.PowerInt or CoflowRegisterOpCode.ShiftLeft or
                CoflowRegisterOpCode.ShiftRight or CoflowRegisterOpCode.BitAnd or
                CoflowRegisterOpCode.BitXor or CoflowRegisterOpCode.BitOr or
                CoflowRegisterOpCode.LessInt or CoflowRegisterOpCode.LessOrEqualInt or
                CoflowRegisterOpCode.GreaterInt or CoflowRegisterOpCode.GreaterOrEqualInt or
                CoflowRegisterOpCode.EqualInteger,
            CoflowRegisterKind.Float => instruction.Code is
                CoflowRegisterOpCode.MoveFloat or CoflowRegisterOpCode.ConvertFloatToInt or
                CoflowRegisterOpCode.NegateFloat or CoflowRegisterOpCode.AddFloat or
                CoflowRegisterOpCode.SubtractFloat or CoflowRegisterOpCode.MultiplyFloat or
                CoflowRegisterOpCode.DivideFloat or CoflowRegisterOpCode.PowerFloat or
                CoflowRegisterOpCode.LessFloat or CoflowRegisterOpCode.LessOrEqualFloat or
                CoflowRegisterOpCode.GreaterFloat or CoflowRegisterOpCode.GreaterOrEqualFloat or
                CoflowRegisterOpCode.EqualFloat,
            _ => instruction.Code is
                CoflowRegisterOpCode.MoveReference or CoflowRegisterOpCode.LoadHostFieldInteger or
                CoflowRegisterOpCode.LoadHostFieldFloat or CoflowRegisterOpCode.LoadHostFieldReference or
                CoflowRegisterOpCode.IsType or CoflowRegisterOpCode.AddString or
                CoflowRegisterOpCode.LessString or CoflowRegisterOpCode.LessOrEqualString or
                CoflowRegisterOpCode.GreaterString or CoflowRegisterOpCode.GreaterOrEqualString or
                CoflowRegisterOpCode.EqualReference,
        };
        var readC = kind switch
        {
            CoflowRegisterKind.Integer => instruction.Code is
                CoflowRegisterOpCode.AddInt or CoflowRegisterOpCode.SubtractInt or
                CoflowRegisterOpCode.MultiplyInt or CoflowRegisterOpCode.DivideInt or
                CoflowRegisterOpCode.IntegerDivide or CoflowRegisterOpCode.Remainder or
                CoflowRegisterOpCode.PowerInt or CoflowRegisterOpCode.ShiftLeft or
                CoflowRegisterOpCode.ShiftRight or CoflowRegisterOpCode.BitAnd or
                CoflowRegisterOpCode.BitXor or CoflowRegisterOpCode.BitOr or
                CoflowRegisterOpCode.LessInt or CoflowRegisterOpCode.LessOrEqualInt or
                CoflowRegisterOpCode.GreaterInt or CoflowRegisterOpCode.GreaterOrEqualInt or
                CoflowRegisterOpCode.EqualInteger,
            CoflowRegisterKind.Float => instruction.Code is
                CoflowRegisterOpCode.AddFloat or CoflowRegisterOpCode.SubtractFloat or
                CoflowRegisterOpCode.MultiplyFloat or CoflowRegisterOpCode.DivideFloat or
                CoflowRegisterOpCode.PowerFloat or CoflowRegisterOpCode.LessFloat or
                CoflowRegisterOpCode.LessOrEqualFloat or CoflowRegisterOpCode.GreaterFloat or
                CoflowRegisterOpCode.GreaterOrEqualFloat or CoflowRegisterOpCode.EqualFloat,
            _ => instruction.Code is
                CoflowRegisterOpCode.AddString or CoflowRegisterOpCode.LessString or
                CoflowRegisterOpCode.LessOrEqualString or CoflowRegisterOpCode.GreaterString or
                CoflowRegisterOpCode.GreaterOrEqualString or CoflowRegisterOpCode.EqualReference,
        };
        return instruction with
        {
            B = readB && instruction.B == target ? source : instruction.B,
            C = readC && instruction.C == target ? source : instruction.C,
        };
    }

    private static (
        CoflowRegisterInstruction[] Instructions,
        CfdSpan?[] InstructionSpans,
        long[] Immediates,
        CoflowRegisterOperations Operations) Compact(
        CoflowLoweredInstruction[] source,
        CfdSpan?[] sourceSpans)
    {
        var targets = new int[source.Length + 1];
        var count = 0;
        for (var index = 0; index < source.Length; index++)
        {
            targets[index] = count;
            if (source[index].Code != CoflowRegisterOpCode.Nop) count++;
        }
        targets[source.Length] = count;

        var instructions = new CoflowRegisterInstruction[count];
        var instructionSpans = new CfdSpan?[count];
        var immediates = new List<long>();
        var operations = new CoflowRegisterOperations.Builder();
        var target = 0;
        for (var index = 0; index < source.Length; index++)
        {
            var instruction = source[index];
            if (instruction.Code == CoflowRegisterOpCode.Nop) continue;
            instruction = instruction.Code switch
            {
                CoflowRegisterOpCode.Jump => instruction with { A = targets[instruction.A] },
                CoflowRegisterOpCode.JumpIfFalse or CoflowRegisterOpCode.JumpIfTrue =>
                    instruction with { B = targets[instruction.B] },
                _ => instruction,
            };
            var immediate = 0;
            if (instruction.Code is CoflowRegisterOpCode.ConstantInteger or CoflowRegisterOpCode.ConstantFloat)
            {
                immediate = immediates.Count;
                immediates.Add(instruction.Immediate);
            }
            var hasOperation = HasOperation(instruction.Code);
            var operation = 0;
            if (hasOperation)
            {
                operation = operations.Add(instruction.Code, instruction.Operation);
            }
            instructions[target] = new CoflowRegisterInstruction(
                instruction.Code,
                instruction.A,
                instruction.Code is CoflowRegisterOpCode.ConstantInteger or CoflowRegisterOpCode.ConstantFloat
                    ? immediate : instruction.B,
                hasOperation ? operation : instruction.C);
            instructionSpans[target] = sourceSpans[index];
            target++;
        }
        return (instructions, instructionSpans, immediates.ToArray(), operations.Build());
    }

    private static bool HasOperation(CoflowRegisterOpCode code) => code is
        CoflowRegisterOpCode.ConstantReference or CoflowRegisterOpCode.ConstantValue or
        CoflowRegisterOpCode.MoveValue or CoflowRegisterOpCode.LoadHostFieldInteger or
        CoflowRegisterOpCode.LoadHostFieldFloat or CoflowRegisterOpCode.LoadHostFieldReference or
        CoflowRegisterOpCode.LoadHostFieldValue or CoflowRegisterOpCode.LoadArenaFieldInteger or
        CoflowRegisterOpCode.LoadArenaFieldFloat or CoflowRegisterOpCode.LoadArenaFieldReference or
        CoflowRegisterOpCode.LoadArenaFieldValue or
        CoflowRegisterOpCode.Native or CoflowRegisterOpCode.MakeArray or
        CoflowRegisterOpCode.MakeDictionary or CoflowRegisterOpCode.ArrayIndex or
        CoflowRegisterOpCode.DictionaryIndex or
        CoflowRegisterOpCode.CollectionCount or CoflowRegisterOpCode.ArrayItem or
        CoflowRegisterOpCode.DictionaryKey or CoflowRegisterOpCode.DictionaryValue or
        CoflowRegisterOpCode.DictionaryKeys or CoflowRegisterOpCode.DictionaryValues or
        CoflowRegisterOpCode.CollectionBuiltin or
        CoflowRegisterOpCode.BeginArrayBuilder or CoflowRegisterOpCode.AppendArrayBuilder or
        CoflowRegisterOpCode.MakeOptionNone or
        CoflowRegisterOpCode.MakeOptionSome or CoflowRegisterOpCode.MakeResultOk or
        CoflowRegisterOpCode.MakeResultErr or CoflowRegisterOpCode.ReadFirstPayload or
        CoflowRegisterOpCode.ReadSecondPayload or CoflowRegisterOpCode.Propagate or
        CoflowRegisterOpCode.MakeClosure or CoflowRegisterOpCode.IsType or CoflowRegisterOpCode.IsArenaType or
        CoflowRegisterOpCode.Call or CoflowRegisterOpCode.CallIndirect or
        CoflowRegisterOpCode.TailCall or CoflowRegisterOpCode.TailCallIndirect or
        CoflowRegisterOpCode.Return;

    private static CoflowLoweredInstruction Constant(
        CoflowEncodedValue value,
        CoflowValueRegister target) => value.Shape.Kind switch
        {
            CoflowValueShapeKind.Unit => new(CoflowRegisterOpCode.Nop),
            CoflowValueShapeKind.Scalar when value.Shape.ScalarKind == CoflowRegisterKind.Integer =>
                new(CoflowRegisterOpCode.ConstantInteger, target.IntegerBase, Immediate: value.Integers[0]),
            CoflowValueShapeKind.Scalar when value.Shape.ScalarKind == CoflowRegisterKind.Float =>
                new(CoflowRegisterOpCode.ConstantFloat, target.FloatBase,
                    Immediate: BitConverter.DoubleToInt64Bits(value.Floats[0])),
            CoflowValueShapeKind.Scalar =>
                new(CoflowRegisterOpCode.ConstantReference, target.ReferenceBase, Operation: value.References[0]),
            _ => new(CoflowRegisterOpCode.ConstantValue,
                Operation: new CoflowRegisterConstantSite(value, target)),
        };

    private static CoflowLoweredInstruction Move(
        CoflowValueRegister source,
        CoflowValueRegister target)
    {
        if (source.Shape.Kind == CoflowValueShapeKind.Unit)
            return new(CoflowRegisterOpCode.Nop);
        if (source.Shape.Kind is not (CoflowValueShapeKind.Scalar or CoflowValueShapeKind.Collection))
            return Transfer(CoflowRegisterOpCode.MoveValue, source, target);
        return source.Shape.ScalarKind switch
        {
            CoflowRegisterKind.Integer => new(
                CoflowRegisterOpCode.MoveInteger, target.IntegerBase, source.IntegerBase),
            CoflowRegisterKind.Float => new(
                CoflowRegisterOpCode.MoveFloat, target.FloatBase, source.FloatBase),
            _ => new(CoflowRegisterOpCode.MoveReference, target.ReferenceBase, source.ReferenceBase),
        };
    }

    private static CoflowLoweredInstruction Transfer(
        CoflowRegisterOpCode code,
        CoflowValueRegister source,
        CoflowValueRegister target) => new(
            code,
            Operation: new CoflowRegisterValueTransfer(source, target));

    private static CoflowLoweredInstruction Field(
        CoflowFieldAccess access,
        CoflowValueRegister receiver,
        CoflowValueRegister target)
    {
        if (access.ReceiverIsStruct)
        {
            if (receiver.Shape.Kind != CoflowValueShapeKind.Struct)
                throw new InvalidOperationException($"field `{access.Name}` requires a struct receiver layout");
            if (access.IsFunction)
                return Native(access.Call, new[] { receiver }, target);
            var source = new CoflowValueRegister(
                target.Shape,
                receiver.IntegerBase + access.IntegerOffset,
                receiver.FloatBase + access.FloatOffset,
                receiver.ReferenceBase + access.ReferenceOffset);
            return Move(source, target);
        }
        if (!access.IsHost)
        {
            if (receiver.Shape.Kind != CoflowValueShapeKind.Record)
                throw new InvalidOperationException($"field `{access.Name}` requires a schema ValueId receiver");
            if (access.IsFunction)
                return Native(access.Call, new[] { receiver }, target);
            if (target.Shape.Kind is CoflowValueShapeKind.Scalar or CoflowValueShapeKind.Collection or
                CoflowValueShapeKind.Record)
            {
                return target.Shape.ScalarKind switch
                {
                    CoflowRegisterKind.Integer => new(CoflowRegisterOpCode.LoadArenaFieldInteger,
                        target.IntegerBase, receiver.IntegerBase, Operation: access),
                    CoflowRegisterKind.Float => new(CoflowRegisterOpCode.LoadArenaFieldFloat,
                        target.FloatBase, receiver.IntegerBase, Operation: access),
                    _ => new(CoflowRegisterOpCode.LoadArenaFieldReference,
                        target.ReferenceBase, receiver.IntegerBase, Operation: access),
                };
            }
            return new(CoflowRegisterOpCode.LoadArenaFieldValue, receiver.IntegerBase,
                Operation: new CoflowRegisterFieldValueSite(access, target));
        }
        if (access.ReadInteger is not null)
            return new(CoflowRegisterOpCode.LoadHostFieldInteger,
                target.IntegerBase, receiver.ReferenceBase, Operation: access);
        if (access.ReadFloat is not null)
            return new(CoflowRegisterOpCode.LoadHostFieldFloat,
                target.FloatBase, receiver.ReferenceBase, Operation: access);
        if (access.ReadReference is not null)
            return new(CoflowRegisterOpCode.LoadHostFieldReference,
                target.ReferenceBase, receiver.ReferenceBase, Operation: access);
        if (access.ReadValue is not null)
        {
            return new(CoflowRegisterOpCode.LoadHostFieldValue,
                receiver.ReferenceBase, Operation: new CoflowRegisterFieldValueSite(access, target));
        }
        return Native(access.Call, new[] { receiver }, target);
    }

    private static CoflowLoweredInstruction Native(
        CoflowNativeCall call,
        CoflowValueRegister[] arguments,
        CoflowValueRegister result) => new(
            CoflowRegisterOpCode.Native,
            Operation: new CoflowNativeCallSite(call, arguments, result));

    private static CoflowLoweredInstruction Collection(
        CoflowRegisterOpCode code,
        CoflowValueRegister[] first,
        CoflowValueRegister[]? second,
        CoflowValueRegister target) => new(code,
            Operation: new CoflowRegisterCollectionSite(first, second, target));

    private static CoflowLoweredInstruction Dictionary(
        CoflowValueRegister[] entries,
        CoflowValueRegister target)
    {
        var keys = new CoflowValueRegister[entries.Length / 2];
        var values = new CoflowValueRegister[keys.Length];
        for (var index = 0; index < keys.Length; index++)
        {
            keys[index] = entries[index * 2];
            values[index] = entries[index * 2 + 1];
        }
        return Collection(CoflowRegisterOpCode.MakeDictionary, keys, values, target);
    }

    private static CoflowLoweredInstruction CollectionRead(
        CoflowRegisterOpCode code,
        CoflowValueRegister collection,
        CoflowValueRegister? index,
        CoflowValueRegister target) => new(code,
            Operation: new CoflowRegisterCollectionReadSite(collection, index, target));

    private static CoflowLoweredInstruction ArrayBuilder(
        CoflowRegisterOpCode code,
        CoflowValueRegister collectionOrCapacity,
        CoflowValueRegister? item,
        CoflowValueRegister target) => new(code,
            Operation: new CoflowRegisterArrayBuilderSite(collectionOrCapacity, item, target));

    private static CoflowLoweredInstruction Closure(
        CoflowClosureTemplate template,
        CoflowValueRegister[] captures,
        CoflowValueRegister target) => new(
            CoflowRegisterOpCode.MakeClosure,
            Operation: new CoflowRegisterClosureSite(template, captures, target));

    private static CoflowLoweredInstruction DirectCall(
        CoflowRegisterOpCode code,
        CoflowCallSite call,
        int programIndex,
        CoflowValueRegister[] arguments,
        CoflowValueRegister result,
        int integerWindowBase,
        int floatWindowBase,
        int referenceWindowBase)
    {
        var window = AllocateAt(
            call.VmParameterTypes,
            integerWindowBase,
            floatWindowBase,
            referenceWindowBase);
        return new(code, Operation: new CoflowRegisterCallSite(
            programIndex,
            call.Signature,
            arguments,
            window,
            Enumerable.Repeat(true, arguments.Length).ToArray(),
            result,
            integerWindowBase,
            floatWindowBase,
            referenceWindowBase));
    }

    private static int ResolveProgramIndex(
        CoflowLoweringInput program,
        CoflowCallSite call,
        int pc)
    {
        if (!program.FunctionIndexes.TryGetValue(call.Identity, out var programIndex))
            throw new CoflowProgramLinkException($"unknown function `{call.Identity}`");
        return programIndex;
    }

    private static CoflowValueRegister[] AllocateAt(
        IReadOnlyList<Type> types,
        int integer,
        int floating,
        int reference)
    {
        var result = new CoflowValueRegister[types.Count];
        for (var index = 0; index < types.Count; index++)
            result[index] = Allocate(types[index], ref integer, ref floating, ref reference);
        return result;
    }

    private static CoflowLoweredInstruction IndirectCall(
        CoflowRegisterOpCode code,
        CoflowValueRegister callable,
        CoflowValueRegister[] arguments,
        CoflowValueRegister result,
        Type resultType) => new(
            code,
            Operation: new CoflowRegisterIndirectCallSite(
                callable, arguments, result, resultType));

    private static CoflowLoweredInstruction IntegerUnary(
        CoflowOpCode code,
        int source,
        int target) => new(code switch
        {
            CoflowOpCode.NegateInt => CoflowRegisterOpCode.NegateInt,
            CoflowOpCode.Not => CoflowRegisterOpCode.Not,
            _ => CoflowRegisterOpCode.BitNot,
        }, target, source);

    private static CoflowLoweredInstruction IntegerBinary(
        CoflowOpCode code,
        int target,
        int left,
        int right) => new(code switch
        {
            CoflowOpCode.AddInt => CoflowRegisterOpCode.AddInt,
            CoflowOpCode.SubtractInt => CoflowRegisterOpCode.SubtractInt,
            CoflowOpCode.MultiplyInt => CoflowRegisterOpCode.MultiplyInt,
            CoflowOpCode.DivideInt => CoflowRegisterOpCode.DivideInt,
            CoflowOpCode.IntegerDivide => CoflowRegisterOpCode.IntegerDivide,
            CoflowOpCode.Remainder => CoflowRegisterOpCode.Remainder,
            CoflowOpCode.PowerInt => CoflowRegisterOpCode.PowerInt,
            CoflowOpCode.ShiftLeft => CoflowRegisterOpCode.ShiftLeft,
            CoflowOpCode.ShiftRight => CoflowRegisterOpCode.ShiftRight,
            CoflowOpCode.BitAnd => CoflowRegisterOpCode.BitAnd,
            CoflowOpCode.BitXor => CoflowRegisterOpCode.BitXor,
            _ => CoflowRegisterOpCode.BitOr,
        }, target, left, right);

    private static CoflowLoweredInstruction FloatBinary(
        CoflowOpCode code,
        int target,
        int left,
        int right) => new(code switch
        {
            CoflowOpCode.AddFloat => CoflowRegisterOpCode.AddFloat,
            CoflowOpCode.SubtractFloat => CoflowRegisterOpCode.SubtractFloat,
            CoflowOpCode.MultiplyFloat => CoflowRegisterOpCode.MultiplyFloat,
            CoflowOpCode.DivideFloat => CoflowRegisterOpCode.DivideFloat,
            _ => CoflowRegisterOpCode.PowerFloat,
        }, target, left, right);

    private static CoflowLoweredInstruction IntegerComparison(
        CoflowOpCode code,
        int target,
        int left,
        int right) => new(code switch
        {
            CoflowOpCode.LessInt => CoflowRegisterOpCode.LessInt,
            CoflowOpCode.LessOrEqualInt => CoflowRegisterOpCode.LessOrEqualInt,
            CoflowOpCode.GreaterInt => CoflowRegisterOpCode.GreaterInt,
            CoflowOpCode.GreaterOrEqualInt => CoflowRegisterOpCode.GreaterOrEqualInt,
            _ => CoflowRegisterOpCode.EqualInteger,
        }, target, left, right);

    private static CoflowLoweredInstruction FloatComparison(
        CoflowOpCode code,
        int target,
        int left,
        int right) => new(code switch
        {
            CoflowOpCode.LessFloat => CoflowRegisterOpCode.LessFloat,
            CoflowOpCode.LessOrEqualFloat => CoflowRegisterOpCode.LessOrEqualFloat,
            CoflowOpCode.GreaterFloat => CoflowRegisterOpCode.GreaterFloat,
            CoflowOpCode.GreaterOrEqualFloat => CoflowRegisterOpCode.GreaterOrEqualFloat,
            _ => CoflowRegisterOpCode.EqualFloat,
        }, target, left, right);

    private static CoflowLoweredInstruction StringComparison(
        CoflowOpCode code,
        int target,
        int left,
        int right) => new(code switch
        {
            CoflowOpCode.LessString => CoflowRegisterOpCode.LessString,
            CoflowOpCode.LessOrEqualString => CoflowRegisterOpCode.LessOrEqualString,
            CoflowOpCode.GreaterString => CoflowRegisterOpCode.GreaterString,
            _ => CoflowRegisterOpCode.GreaterOrEqualString,
        }, target, left, right);

}

