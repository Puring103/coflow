namespace CoflowRuntime.Generated;

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

internal enum CoflowRegisterOpCode : byte
{
    Nop,
    ConstantInteger, ConstantFloat, ConstantReference, ConstantValue,
    MoveInteger, MoveFloat, MoveReference, MoveValue,
    LoadFieldInteger, LoadFieldFloat, LoadFieldReference, Native,
    MakeOptionNone, MakeOptionSome, MakeResultOk, MakeResultErr,
    ReadValueTag, ReadFirstPayload, ReadSecondPayload, Propagate,
    MakeClosure,
    ConvertIntToFloat, ConvertFloatToInt, IsType,
    NegateInt, NegateFloat, Not, BitNot,
    AddInt, AddFloat, AddString, SubtractInt, SubtractFloat, MultiplyInt, MultiplyFloat,
    DivideInt, DivideFloat, IntegerDivide, Remainder, PowerInt, PowerFloat,
    ShiftLeft, ShiftRight, BitAnd, BitXor, BitOr,
    LessInt, LessFloat, LessString, LessOrEqualInt, LessOrEqualFloat, LessOrEqualString,
    GreaterInt, GreaterFloat, GreaterString,
    GreaterOrEqualInt, GreaterOrEqualFloat, GreaterOrEqualString,
    EqualInteger, EqualFloat, EqualReference,
    JumpIfFalse, JumpIfTrue, Jump,
    Call, CallIndirect, TailCall, TailCallIndirect, Return,
}

internal readonly record struct CoflowRegisterInstruction(
    CoflowRegisterOpCode Code,
    int A = 0,
    int B = 0,
    int C = 0,
    long Immediate = 0,
    object? Operation = null);

internal sealed record CoflowRegisterValueTransfer(
    CoflowValueRegister Source,
    CoflowValueRegister Target);

internal sealed record CoflowRegisterConstantSite(
    CoflowEncodedValue Value,
    CoflowValueRegister Target);

internal sealed record CoflowRegisterTargetSite(CoflowValueRegister Target);

internal sealed record CoflowRegisterPropagateSite(
    CoflowValueRegister Source,
    CoflowValueRegister Payload,
    CoflowValueRegister ReturnValue);

internal sealed record CoflowRegisterClosureSite(
    CoflowClosureTemplate Template,
    CoflowValueRegister[] Captures,
    CoflowValueRegister Target);

internal sealed record CoflowRegisterCallSite(
    CoflowFunctionEntry Entry,
    CoflowValueRegister[] Arguments,
    CoflowValueRegister Result);

internal sealed record CoflowRegisterIndirectCallSite(
    CoflowValueRegister Callable,
    CoflowValueRegister[] Arguments,
    CoflowValueRegister Result,
    Type ResultType);

internal sealed record CoflowLoweringInput(
    CoflowFunctionIdentity Identity,
    CoflowInstruction[] Instructions,
    CfdSpan?[] InstructionSpans,
    object?[] Operations,
    CoflowEncodedValue?[] EncodedConstants,
    Type[] ParameterTypes,
    Type ReturnType,
    int LocalCount);

internal sealed class CoflowRegisterProgram
{
    internal CoflowRegisterProgram(
        CoflowValueRegister[] parameters,
        CoflowRegisterInstruction[] instructions,
        CfdSpan?[] instructionSpans,
        int integerRegisterCount,
        int floatRegisterCount,
        int referenceRegisterCount)
    {
        Parameters = parameters;
        Instructions = instructions;
        InstructionSpans = instructionSpans;
        ParameterIntegerCount = parameters.Sum(value => value.Shape.IntegerCount);
        ParameterFloatCount = parameters.Sum(value => value.Shape.FloatCount);
        ParameterReferenceCount = parameters.Sum(value => value.Shape.ReferenceCount);
        IntegerRegisterCount = integerRegisterCount;
        FloatRegisterCount = floatRegisterCount;
        ReferenceRegisterCount = referenceRegisterCount;
    }

    internal CoflowValueRegister[] Parameters { get; }
    internal CoflowRegisterInstruction[] Instructions { get; }
    internal CfdSpan?[] InstructionSpans { get; }
    internal int IntegerRegisterCount { get; }
    internal int FloatRegisterCount { get; }
    internal int ReferenceRegisterCount { get; }
    internal int ParameterIntegerCount { get; }
    internal int ParameterFloatCount { get; }
    internal int ParameterReferenceCount { get; }
}

internal static class CoflowRegisterLowering
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
        var (executable, instructionSpans) = LowerInstructions(
            program,
            states.Take(instructions.Length).Select(value => value!).ToArray(),
            parameters,
            localRegisters,
            returnValue,
            temporaries);
        return new CoflowRegisterProgram(
            parameters,
            executable,
            instructionSpans,
            integer,
            floating,
            reference);
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

    private static (CoflowRegisterInstruction[] Instructions, CfdSpan?[] InstructionSpans) LowerInstructions(
        CoflowLoweringInput program,
        Type[][] states,
        CoflowValueRegister[] parameters,
        CoflowValueRegister[] locals,
        CoflowValueRegister returnValue,
        CoflowValueRegister[] temporaries)
    {
        var result = new CoflowRegisterInstruction[program.Instructions.Length];
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

            var valueType = source.Code switch
            {
                CoflowOpCode.Constant => program.EncodedConstants[source.Operand]?.Shape.Type ?? typeof(object),
                CoflowOpCode.Argument => program.ParameterTypes[source.Operand],
                CoflowOpCode.Local => locals[source.Operand].Shape.Type,
                CoflowOpCode.LoadField => ((CoflowFieldAccess)Operation()).RuntimeType,
                CoflowOpCode.Native => ((CoflowNativeCall)Operation()).ResultType,
                CoflowOpCode.Call => ((CoflowCallSite)Operation()).Entry.Signature.ResultType,
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
                    CoflowRegisterOpCode.IsType,
                    Output(typeof(bool), 1).IntegerBase,
                    Top().ReferenceBase,
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
                    Arguments(((CoflowCallSite)Operation()).ArgumentCount),
                    Output(((CoflowCallSite)Operation()).Entry.Signature.ResultType,
                        ((CoflowCallSite)Operation()).ArgumentCount)),
                CoflowOpCode.CallIndirect => IndirectCall(
                    CoflowRegisterOpCode.CallIndirect,
                    Stack(depth - source.Operand - 1),
                    Arguments(source.Operand),
                    Output(valueType, source.Operand + 1),
                    valueType),
                CoflowOpCode.TailCall => DirectCall(
                    CoflowRegisterOpCode.TailCall,
                    (CoflowCallSite)Operation(),
                    Arguments(((CoflowCallSite)Operation()).ArgumentCount),
                    returnValue),
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
        return Compact(result, program.InstructionSpans);
    }

    private static (CoflowRegisterInstruction[] Instructions, CfdSpan?[] InstructionSpans) Compact(
        CoflowRegisterInstruction[] source,
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
            instructions[target] = instruction;
            instructionSpans[target] = sourceSpans[index];
            target++;
        }
        return (instructions, instructionSpans);
    }

    private static CoflowRegisterInstruction Constant(
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

    private static CoflowRegisterInstruction Move(
        CoflowValueRegister source,
        CoflowValueRegister target)
    {
        if (source.Shape.Kind == CoflowValueShapeKind.Unit)
            return new(CoflowRegisterOpCode.Nop);
        if (source.Shape.Kind != CoflowValueShapeKind.Scalar)
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

    private static CoflowRegisterInstruction Transfer(
        CoflowRegisterOpCode code,
        CoflowValueRegister source,
        CoflowValueRegister target) => new(
            code,
            Operation: new CoflowRegisterValueTransfer(source, target));

    private static CoflowRegisterInstruction Field(
        CoflowFieldAccess access,
        CoflowValueRegister receiver,
        CoflowValueRegister target)
    {
        if (access.ReadInteger is not null)
            return new(CoflowRegisterOpCode.LoadFieldInteger,
                target.IntegerBase, receiver.ReferenceBase, Operation: access);
        if (access.ReadFloat is not null)
            return new(CoflowRegisterOpCode.LoadFieldFloat,
                target.FloatBase, receiver.ReferenceBase, Operation: access);
        if (access.ReadReference is not null)
            return new(CoflowRegisterOpCode.LoadFieldReference,
                target.ReferenceBase, receiver.ReferenceBase, Operation: access);
        return Native(access.Call, new[] { receiver }, target);
    }

    private static CoflowRegisterInstruction Native(
        CoflowNativeCall call,
        CoflowValueRegister[] arguments,
        CoflowValueRegister result) => new(
            CoflowRegisterOpCode.Native,
            Operation: new CoflowNativeCallSite(call, arguments, result));

    private static CoflowRegisterInstruction Closure(
        CoflowClosureTemplate template,
        CoflowValueRegister[] captures,
        CoflowValueRegister target) => new(
            CoflowRegisterOpCode.MakeClosure,
            Operation: new CoflowRegisterClosureSite(template, captures, target));

    private static CoflowRegisterInstruction DirectCall(
        CoflowRegisterOpCode code,
        CoflowCallSite call,
        CoflowValueRegister[] arguments,
        CoflowValueRegister result) => new(
            code,
            Operation: new CoflowRegisterCallSite(call.Entry, arguments, result));

    private static CoflowRegisterInstruction IndirectCall(
        CoflowRegisterOpCode code,
        CoflowValueRegister callable,
        CoflowValueRegister[] arguments,
        CoflowValueRegister result,
        Type resultType) => new(
            code,
            Operation: new CoflowRegisterIndirectCallSite(
                callable, arguments, result, resultType));

    private static CoflowRegisterInstruction IntegerUnary(
        CoflowOpCode code,
        int source,
        int target) => new(code switch
        {
            CoflowOpCode.NegateInt => CoflowRegisterOpCode.NegateInt,
            CoflowOpCode.Not => CoflowRegisterOpCode.Not,
            _ => CoflowRegisterOpCode.BitNot,
        }, target, source);

    private static CoflowRegisterInstruction IntegerBinary(
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

    private static CoflowRegisterInstruction FloatBinary(
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

    private static CoflowRegisterInstruction IntegerComparison(
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

    private static CoflowRegisterInstruction FloatComparison(
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

    private static CoflowRegisterInstruction StringComparison(
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
    private static InvalidOperationException Invalid(CoflowLoweringInput program, string message) =>
        new($"invalid Coflow program `{program.Identity}`: {message}");
}
