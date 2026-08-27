namespace CoflowRuntime;

using System.Buffers;

internal enum CoflowOpCode : byte
{
    Constant,
    Argument,
    Local,
    StoreLocal,
    LocalInt,
    StoreLocalInt,
    LoadField,
    Construct,
    Native,
    Propagate,
    MakeClosure,
    HigherOrder,
    Pop,
    NegateInt,
    NegateFloat,
    Not,
    BitNot,
    AddInt,
    AddFloat,
    AddString,
    SubtractInt,
    SubtractFloat,
    MultiplyInt,
    MultiplyFloat,
    DivideInt,
    DivideFloat,
    IntegerDivide,
    Remainder,
    PowerInt,
    PowerFloat,
    ShiftLeft,
    ShiftRight,
    BitAnd,
    BitXor,
    BitOr,
    LessInt,
    LessFloat,
    LessString,
    LessOrEqualInt,
    LessOrEqualFloat,
    LessOrEqualString,
    GreaterInt,
    GreaterFloat,
    GreaterString,
    GreaterOrEqualInt,
    GreaterOrEqualFloat,
    GreaterOrEqualString,
    JumpIfFalseKeep,
    JumpIfTrueKeep,
    JumpIfFalse,
    Jump,
    Call,
    CallIndirect,
    TailCall,
    TailCallIndirect,
    Return,
    JumpIfLocalIntNotLessArgument,
    AddLocalIntsAndStore,
    AddLocalIntConstantAndStore,
    AddIntegerFieldChainAndStore,
    RunIntegerLoop,
    RunTailIntegerCountdown,
}

internal readonly record struct CoflowInstruction(
    CoflowOpCode Code,
    int Operand = 0,
    int Operand2 = 0,
    int Operand3 = 0);
internal readonly record struct CoflowCallSite(CoflowFunctionSlot Slot, int ArgumentCount);
internal readonly record struct CoflowIntegerFieldChain(
    int SourceLocal,
    int TargetLocal,
    object Receiver,
    CoflowFieldAccess[] Accesses,
    int InstructionCount);
internal enum CoflowIntegerLoopOperationKind : byte
{
    AddLocals,
    AddConstant,
    AddFieldChain,
}
internal readonly record struct CoflowIntegerLoopOperation(
    CoflowIntegerLoopOperationKind Kind,
    int SourceLocal,
    int TargetLocal,
    int RightLocal,
    long Constant,
    object? Receiver,
    CoflowFieldAccess[]? Accesses,
    int InstructionCount,
    int SourcePc);
internal readonly record struct CoflowIntegerLoop(
    int ConditionLocal,
    int LimitArgument,
    int StartPc,
    int EndPc,
    CoflowIntegerLoopOperation[] Operations);
internal readonly record struct CoflowTailIntegerCountdown(
    int Argument,
    long Threshold,
    long Step,
    bool Subtract,
    object? Result,
    int ComparisonPc,
    int SubtractPc,
    int ReturnPc);
internal readonly record struct CoflowSimpleIntegerFunction(
    int Argument,
    long Constant,
    CoflowOpCode Operation,
    int OperationPc);
internal readonly record struct CoflowNativeCall(Func<object?[], object?> Invoke, int ArgumentCount);
internal readonly record struct CoflowLoopAccess(
    Func<object?, object?> Prepare,
    Func<object?, object?> Count,
    Func<object?[], object?> First,
    Func<object?[], object?>? Second);
internal readonly record struct CoflowPropagationResult(bool Success, object? Value);
internal readonly record struct CoflowRange(long Start, long End, bool Inclusive)
{
    internal long Count => End <= Start
        ? Inclusive && End == Start ? 1 : 0
        : checked(End - Start + (Inclusive ? 1 : 0));
}
internal readonly record struct CoflowClosureTemplate(CoflowProgram Program, int CaptureCount);
internal sealed record CoflowClosure(CoflowProgram Program, object?[] Captures)
{
    public TResult Invoke<TResult>(object?[] arguments) =>
        CoflowFunctionDelegates.Adapt<TResult>(CoflowVm.Execute(
            Program, Append(arguments, Captures)));

    public void InvokeVoid(object?[] arguments) =>
        CoflowVm.Execute(Program, Append(arguments, Captures));

    internal static object?[] Append(object?[] arguments, object?[] captures)
    {
        var combined = new object?[arguments.Length + captures.Length];
        Array.Copy(arguments, combined, arguments.Length);
        Array.Copy(captures, 0, combined, arguments.Length, captures.Length);
        return combined;
    }
}
internal readonly record struct CoflowHigherOrderOperation(
    string Name,
    Type ResultType,
    Func<object?, int> Count,
    Func<object?, int, object?> Item,
    Func<object?[], object?> CreateArray,
    Func<object?, object?> CreateSome,
    object? None);

internal sealed class CoflowProgram
{
    internal CoflowProgram(
        CoflowFunctionIdentity identity,
        string sourcePath,
        CfdSpan? sourceSpan,
        IReadOnlyList<CoflowInstruction> instructions,
        IReadOnlyList<CfdSpan?> instructionSpans,
        IReadOnlyList<object?> constants,
        int parameterCount,
        int localCount)
    {
        Identity = identity;
        SourcePath = sourcePath;
        SourceSpan = sourceSpan;
        var frozenInstructions = instructions as CoflowInstruction[] ?? instructions.ToArray();
        var frozenSpans = instructionSpans as CfdSpan?[] ?? instructionSpans.ToArray();
        var frozenConstants = constants as object?[] ?? constants.ToArray();
        (Instructions, Constants) = Optimize(
            identity, frozenInstructions, frozenSpans, frozenConstants);
        InstructionSpans = frozenSpans;
        ParameterCount = parameterCount;
        LocalCount = localCount;
        IntegerLocalCount = Instructions.Any(instruction => instruction.Code is
            CoflowOpCode.LocalInt or CoflowOpCode.StoreLocalInt or
            CoflowOpCode.JumpIfLocalIntNotLessArgument or
            CoflowOpCode.AddLocalIntsAndStore or CoflowOpCode.AddLocalIntConstantAndStore or
            CoflowOpCode.AddIntegerFieldChainAndStore or CoflowOpCode.RunIntegerLoop)
            ? localCount
            : 0;
        SimpleIntegerFunction = TryCreateSimpleIntegerFunction(
            frozenInstructions, frozenConstants, out var simpleIntegerFunction)
            ? simpleIntegerFunction
            : null;
    }

    internal CoflowFunctionIdentity Identity { get; }
    internal string SourcePath { get; }
    internal CfdSpan? SourceSpan { get; }
    internal CoflowInstruction[] Instructions { get; }
    internal CfdSpan?[] InstructionSpans { get; }
    internal object?[] Constants { get; }
    internal int ParameterCount { get; }
    internal int LocalCount { get; }
    internal int IntegerLocalCount { get; }
    internal CoflowSimpleIntegerFunction? SimpleIntegerFunction { get; }

    private static bool TryCreateSimpleIntegerFunction(
        CoflowInstruction[] source,
        object?[] constants,
        out CoflowSimpleIntegerFunction function)
    {
        function = default;
        if (source.Length != 4 ||
            source[0].Code != CoflowOpCode.Argument ||
            source[1].Code != CoflowOpCode.Constant ||
            constants[source[1].Operand] is not long constant ||
            source[2].Code is not (CoflowOpCode.AddInt or CoflowOpCode.SubtractInt or
                CoflowOpCode.MultiplyInt) ||
            source[3].Code != CoflowOpCode.Return)
            return false;
        function = new CoflowSimpleIntegerFunction(
            source[0].Operand, constant, source[2].Code, OperationPc: 2);
        return true;
    }

    private static (CoflowInstruction[] Instructions, object?[] Constants) Optimize(
        CoflowFunctionIdentity identity,
        CoflowInstruction[] source,
        CfdSpan?[] spans,
        object?[] constants)
    {
        var instructions = (CoflowInstruction[])source.Clone();
        var jumpTargets = source
            .Where(instruction => instruction.Code is CoflowOpCode.Jump
                or CoflowOpCode.JumpIfFalse
                or CoflowOpCode.JumpIfFalseKeep
                or CoflowOpCode.JumpIfTrueKeep)
            .Select(instruction => instruction.Operand)
            .ToHashSet();
        var integerLocals = new HashSet<int>();

        for (var index = 0; index + 3 < source.Length; index++)
        {
            if (jumpTargets.Contains(index + 1) || jumpTargets.Contains(index + 2) ||
                jumpTargets.Contains(index + 3))
                continue;

            var first = source[index];
            var second = source[index + 1];
            var third = source[index + 2];
            var fourth = source[index + 3];
            if (first.Code == CoflowOpCode.Local && second.Code == CoflowOpCode.Argument &&
                third.Code == CoflowOpCode.LessInt && fourth.Code == CoflowOpCode.JumpIfFalse)
            {
                integerLocals.Add(first.Operand);
                instructions[index] = new CoflowInstruction(
                    CoflowOpCode.JumpIfLocalIntNotLessArgument,
                    first.Operand, second.Operand, fourth.Operand);
                index += 3;
                continue;
            }
            if (first.Code == CoflowOpCode.Local && second.Code == CoflowOpCode.Local &&
                third.Code == CoflowOpCode.AddInt && fourth.Code == CoflowOpCode.StoreLocal)
            {
                integerLocals.Add(first.Operand);
                integerLocals.Add(second.Operand);
                integerLocals.Add(fourth.Operand);
                instructions[index] = new CoflowInstruction(
                    CoflowOpCode.AddLocalIntsAndStore,
                    first.Operand, second.Operand, fourth.Operand);
                spans[index] = spans[index + 2];
                index += 3;
                continue;
            }
            if (first.Code == CoflowOpCode.Local && second.Code == CoflowOpCode.Constant &&
                constants[second.Operand] is long && third.Code == CoflowOpCode.AddInt &&
                fourth.Code == CoflowOpCode.StoreLocal)
            {
                integerLocals.Add(first.Operand);
                integerLocals.Add(fourth.Operand);
                instructions[index] = new CoflowInstruction(
                    CoflowOpCode.AddLocalIntConstantAndStore,
                    first.Operand, second.Operand, fourth.Operand);
                spans[index] = spans[index + 2];
                index += 3;
            }
        }

        var optimizedConstants = constants.ToList();
        for (var index = 0; index + 4 < source.Length; index++)
        {
            if (source[index].Code != CoflowOpCode.Local ||
                source[index + 1].Code != CoflowOpCode.Constant)
                continue;
            var cursor = index + 2;
            var accesses = new List<CoflowFieldAccess>();
            while (cursor < source.Length && source[cursor].Code == CoflowOpCode.LoadField)
            {
                accesses.Add((CoflowFieldAccess)constants[source[cursor].Operand]!);
                cursor++;
            }
            if (accesses.Count == 0 || accesses[^1].RuntimeType != typeof(long) ||
                cursor + 1 >= source.Length || source[cursor].Code != CoflowOpCode.AddInt ||
                source[cursor + 1].Code != CoflowOpCode.StoreLocal ||
                Enumerable.Range(index + 1, cursor - index + 1).Any(jumpTargets.Contains))
                continue;

            var instructionCount = cursor - index + 2;
            var chain = new CoflowIntegerFieldChain(
                source[index].Operand,
                source[cursor + 1].Operand,
                constants[source[index + 1].Operand]!,
                accesses.ToArray(),
                instructionCount);
            var constantIndex = optimizedConstants.Count;
            optimizedConstants.Add(chain);
            integerLocals.Add(chain.SourceLocal);
            integerLocals.Add(chain.TargetLocal);
            instructions[index] = new CoflowInstruction(
                CoflowOpCode.AddIntegerFieldChainAndStore, constantIndex);
            spans[index] = spans[cursor];
            index = cursor + 1;
        }

        for (var index = 0; index + 4 < source.Length; index++)
        {
            if (source[index].Code != CoflowOpCode.Local ||
                source[index + 1].Code != CoflowOpCode.Argument ||
                source[index + 2].Code != CoflowOpCode.LessInt ||
                source[index + 3].Code != CoflowOpCode.JumpIfFalse ||
                jumpTargets.Contains(index + 1) || jumpTargets.Contains(index + 2) ||
                jumpTargets.Contains(index + 3))
                continue;
            var end = source[index + 3].Operand;
            if (end <= index + 4 || end > source.Length ||
                source[end - 1].Code != CoflowOpCode.Jump ||
                source[end - 1].Operand != index)
                continue;

            var operations = new List<CoflowIntegerLoopOperation>();
            var cursor = index + 4;
            var valid = true;
            while (cursor < end - 1)
            {
                if (operations.Count != 0 && cursor + 1 < end &&
                    source[cursor].Code == CoflowOpCode.Constant &&
                    constants[source[cursor].Operand] is Unit &&
                    source[cursor + 1].Code == CoflowOpCode.Pop)
                {
                    operations[^1] = operations[^1] with
                    {
                        InstructionCount = operations[^1].InstructionCount + 2,
                    };
                    cursor += 2;
                    continue;
                }
                var operationStart = cursor;
                CoflowIntegerLoopOperation operation;
                if (cursor + 3 < end && source[cursor].Code == CoflowOpCode.Local &&
                    source[cursor + 1].Code == CoflowOpCode.Local &&
                    source[cursor + 2].Code == CoflowOpCode.AddInt &&
                    source[cursor + 3].Code == CoflowOpCode.StoreLocal)
                {
                    operation = new CoflowIntegerLoopOperation(
                        CoflowIntegerLoopOperationKind.AddLocals,
                        source[cursor].Operand,
                        source[cursor + 3].Operand,
                        source[cursor + 1].Operand,
                        0, null, null, 4, cursor + 2);
                    cursor += 4;
                }
                else if (cursor + 3 < end && source[cursor].Code == CoflowOpCode.Local &&
                    source[cursor + 1].Code == CoflowOpCode.Constant &&
                    constants[source[cursor + 1].Operand] is long constant &&
                    source[cursor + 2].Code == CoflowOpCode.AddInt &&
                    source[cursor + 3].Code == CoflowOpCode.StoreLocal)
                {
                    operation = new CoflowIntegerLoopOperation(
                        CoflowIntegerLoopOperationKind.AddConstant,
                        source[cursor].Operand,
                        source[cursor + 3].Operand,
                        0, constant, null, null, 4, cursor + 2);
                    cursor += 4;
                }
                else if (cursor + 4 < end && source[cursor].Code == CoflowOpCode.Local &&
                    source[cursor + 1].Code == CoflowOpCode.Constant)
                {
                    var accessCursor = cursor + 2;
                    var accesses = new List<CoflowFieldAccess>();
                    while (accessCursor < end && source[accessCursor].Code == CoflowOpCode.LoadField)
                    {
                        accesses.Add((CoflowFieldAccess)constants[source[accessCursor].Operand]!);
                        accessCursor++;
                    }
                    if (accesses.Count == 0 || accesses[^1].RuntimeType != typeof(long) ||
                        accessCursor + 1 >= end || source[accessCursor].Code != CoflowOpCode.AddInt ||
                        source[accessCursor + 1].Code != CoflowOpCode.StoreLocal)
                    {
                        valid = false;
                        break;
                    }
                    var receiver = constants[source[cursor + 1].Operand]!;
                    if (accesses.All(access => !access.IsHost))
                    {
                        for (var accessIndex = 0; accessIndex < accesses.Count - 1; accessIndex++)
                            receiver = accesses[accessIndex].Read(receiver)!;
                        operation = new CoflowIntegerLoopOperation(
                            CoflowIntegerLoopOperationKind.AddConstant,
                            source[cursor].Operand,
                            source[accessCursor + 1].Operand,
                            0,
                            accesses[^1].ReadInteger(receiver),
                            null, null,
                            accessCursor - cursor + 2,
                            accessCursor);
                    }
                    else
                    {
                        operation = new CoflowIntegerLoopOperation(
                            CoflowIntegerLoopOperationKind.AddFieldChain,
                            source[cursor].Operand,
                            source[accessCursor + 1].Operand,
                            0, 0,
                            receiver,
                            accesses.ToArray(),
                            accessCursor - cursor + 2,
                            accessCursor);
                    }
                    cursor = accessCursor + 2;
                }
                else
                {
                    valid = false;
                    break;
                }

                if (cursor + 1 < end && source[cursor].Code == CoflowOpCode.Constant &&
                    constants[source[cursor].Operand] is Unit &&
                    source[cursor + 1].Code == CoflowOpCode.Pop)
                {
                    cursor += 2;
                    operation = operation with
                    {
                        InstructionCount = operation.InstructionCount + 2,
                    };
                }
                if (jumpTargets.Any(target => target > operationStart && target < cursor))
                {
                    valid = false;
                    break;
                }
                operations.Add(operation);
            }
            if (!valid || operations.Count == 0 || cursor != end - 1) continue;

            integerLocals.Add(source[index].Operand);
            foreach (var operation in operations)
            {
                integerLocals.Add(operation.SourceLocal);
                integerLocals.Add(operation.TargetLocal);
                if (operation.Kind == CoflowIntegerLoopOperationKind.AddLocals)
                    integerLocals.Add(operation.RightLocal);
            }
            var loop = new CoflowIntegerLoop(
                source[index].Operand,
                source[index + 1].Operand,
                index,
                end,
                operations.ToArray());
            var constantIndex = optimizedConstants.Count;
            optimizedConstants.Add(loop);
            instructions[index] = new CoflowInstruction(CoflowOpCode.RunIntegerLoop, constantIndex);
            spans[index] = spans[index + 2];
            index = end - 1;
        }

        for (var index = 0; index < instructions.Length; index++)
        {
            var instruction = instructions[index];
            if (!integerLocals.Contains(instruction.Operand)) continue;
            if (instruction.Code == CoflowOpCode.Local)
                instructions[index] = instruction with { Code = CoflowOpCode.LocalInt };
            else if (instruction.Code == CoflowOpCode.StoreLocal)
                instructions[index] = instruction with { Code = CoflowOpCode.StoreLocalInt };
        }

        if (TryCreateTailIntegerCountdown(identity, source, constants, out var countdown))
        {
            var constantIndex = optimizedConstants.Count;
            optimizedConstants.Add(countdown);
            instructions[0] = new CoflowInstruction(
                CoflowOpCode.RunTailIntegerCountdown, constantIndex);
            spans[0] = spans[countdown.ComparisonPc];
        }
        return (instructions, optimizedConstants.ToArray());
    }

    private static bool TryCreateTailIntegerCountdown(
        CoflowFunctionIdentity identity,
        CoflowInstruction[] source,
        object?[] constants,
        out CoflowTailIntegerCountdown countdown)
    {
        countdown = default;
        if (source.Length != 10 ||
            source[0].Code != CoflowOpCode.Argument ||
            source[1].Code != CoflowOpCode.Constant ||
            constants[source[1].Operand] is not long threshold ||
            source[2].Code != CoflowOpCode.LessOrEqualInt ||
            source[3].Code != CoflowOpCode.JumpIfFalse || source[3].Operand != 6 ||
            source[4].Code != CoflowOpCode.Constant ||
            source[5].Code != CoflowOpCode.Return ||
            source[6].Code != CoflowOpCode.Argument ||
            source[6].Operand != source[0].Operand ||
            source[7].Code != CoflowOpCode.Constant ||
            constants[source[7].Operand] is not long decrement ||
            source[8].Code is not (CoflowOpCode.AddInt or CoflowOpCode.SubtractInt) ||
            source[9].Code != CoflowOpCode.TailCall ||
            constants[source[9].Operand] is not CoflowCallSite { ArgumentCount: 1 } call ||
            call.Slot.Identity != identity)
            return false;

        countdown = new CoflowTailIntegerCountdown(
            source[0].Operand,
            threshold,
            decrement,
            source[8].Code == CoflowOpCode.SubtractInt,
            constants[source[4].Operand],
            ComparisonPc: 2,
            SubtractPc: 8,
            ReturnPc: 5);
        return true;
    }
}

public sealed class CoflowFaultException : Exception
{
    internal CoflowFaultException(
        CoflowFunctionIdentity function,
        string sourcePath,
        CfdSpan? sourceSpan,
        IReadOnlyList<CoflowFunctionIdentity> callStack,
        string message,
        Exception? inner = null,
        bool preserveSourceLocation = false)
        : base(message, inner)
    {
        Function = function;
        SourcePath = sourcePath;
        SourceSpan = sourceSpan;
        CallStack = callStack;
        PreserveSourceLocation = preserveSourceLocation;
    }

    public CoflowFunctionIdentity Function { get; }
    public string SourcePath { get; }
    public CfdSpan? SourceSpan { get; }
    public IReadOnlyList<CoflowFunctionIdentity> CallStack { get; }
    internal bool PreserveSourceLocation { get; }

    internal CoflowFaultException WithCallers(
        IEnumerable<CoflowFunctionIdentity> callers,
        string? callerSourcePath = null,
        CfdSpan? callerSourceSpan = null)
    {
        var stack = CallStack.Concat(callers)
            .Distinct()
            .Take(32)
            .ToArray();
        return new CoflowFaultException(
            Function,
            callerSourcePath ?? SourcePath,
            callerSourceSpan ?? SourceSpan,
            stack,
            Message,
            InnerException,
            PreserveSourceLocation);
    }
}

internal static class CoflowVm
{
    private const long MaximumInstructions = 10_000_000;
    private const int MaximumFrames = 4096;
    private const int MaximumStackValues = 1_000_000;
    [ThreadStatic]
    private static CoflowExecutionContext? _currentExecution;
    [ThreadStatic]
    private static CoflowExecutionContext? _pooledExecution;
    [ThreadStatic]
    private static long? _instructionLimitOverride;

    internal static IDisposable OverrideInstructionLimitForCurrentThread(long limit)
    {
        if (limit <= 0) throw new ArgumentOutOfRangeException(nameof(limit));
        var previous = _instructionLimitOverride;
        _instructionLimitOverride = limit;
        return new InstructionLimitScope(previous);
    }

    internal static object? Execute(CoflowProgram program, object?[] arguments)
    {
        if (arguments.Length != program.ParameterCount)
            throw Fault(program,
                $"function expected {program.ParameterCount} arguments but received {arguments.Length}");

        var previousExecution = _currentExecution;
        var execution = previousExecution ?? RentExecutionContext(
            _instructionLimitOverride ?? MaximumInstructions);
        var initialStackSize = Math.Min(MaximumStackValues,
            Math.Max(16, program.Instructions.Length / 2));
        var stack = ArrayPool<object?>.Shared.Rent(initialStackSize);
        var frames = new Stack<Frame>();
        _currentExecution = execution;
        var stackCount = 0;
        void Push(object? value)
        {
            execution.PushValue();
            if (stackCount == stack.Length)
            {
                var replacement = ArrayPool<object?>.Shared.Rent(
                    Math.Min(MaximumStackValues, checked(stack.Length * 2)));
                Array.Copy(stack, replacement, stackCount);
                Array.Clear(stack, 0, stackCount);
                ArrayPool<object?>.Shared.Return(stack);
                stack = replacement;
            }
            stack[stackCount++] = value;
        }
        object? Pop()
        {
            if (stackCount == 0) throw new InvalidOperationException("VM stack underflow.");
            var value = stack[--stackCount];
            stack[stackCount] = null;
            execution.PopValue();
            return value;
        }

        try
        {
            var initialFrame = new Frame(program, arguments, stackBase: 0);
            execution.EnterFrame(program.Identity);
            frames.Push(initialFrame);
            while (frames.Count != 0)
            {
                var frame = frames.Peek();
                execution.ChargeInstruction();
                if (frame.Pc >= frame.Program.Instructions.Length)
                    throw new InvalidOperationException("Coflow function ended without a return instruction.");
                var instruction = frame.Program.Instructions[frame.Pc++];
                switch (instruction.Code)
                {
                    case CoflowOpCode.Constant:
                        Push(frame.Program.Constants[instruction.Operand]);
                        break;
                    case CoflowOpCode.Argument:
                        Push(frame.Arguments[instruction.Operand]);
                        break;
                    case CoflowOpCode.Local:
                        Push(frame.Locals[instruction.Operand]);
                        break;
                    case CoflowOpCode.StoreLocal:
                        frame.Locals[instruction.Operand] = Pop();
                        break;
                    case CoflowOpCode.LocalInt:
                        Push(frame.IntegerLocals[instruction.Operand]);
                        break;
                    case CoflowOpCode.StoreLocalInt:
                        frame.IntegerLocals[instruction.Operand] = (long)Pop()!;
                        break;
                    case CoflowOpCode.LoadField:
                        Push(((CoflowFieldAccess)frame.Program.Constants[instruction.Operand]!).Read(Pop()!));
                        break;
                    case CoflowOpCode.Construct:
                        Push(((Func<object?, object?>)frame.Program.Constants[instruction.Operand]!)(Pop()));
                        break;
                    case CoflowOpCode.Native:
                    {
                        var call = (CoflowNativeCall)frame.Program.Constants[instruction.Operand]!;
                        var nativeArguments = new object?[call.ArgumentCount];
                        for (var index = nativeArguments.Length - 1; index >= 0; index--)
                            nativeArguments[index] = Pop();
                        Push(call.Invoke(nativeArguments));
                        break;
                    }
                    case CoflowOpCode.Propagate:
                    {
                        var propagate = (Func<object?, CoflowPropagationResult>)
                            frame.Program.Constants[instruction.Operand]!;
                        var result = propagate(Pop());
                        if (result.Success)
                        {
                            Push(result.Value);
                            break;
                        }
                        if (CompleteFrame(result.Value, out var propagated)) return propagated;
                        break;
                    }
                    case CoflowOpCode.MakeClosure:
                    {
                        var template = (CoflowClosureTemplate)frame.Program.Constants[instruction.Operand]!;
                        var captures = new object?[template.CaptureCount];
                        for (var index = captures.Length - 1; index >= 0; index--)
                            captures[index] = Pop();
                        Push(new CoflowClosure(template.Program, captures));
                        break;
                    }
                    case CoflowOpCode.HigherOrder:
                    {
                        var operation = (CoflowHigherOrderOperation)frame.Program.Constants[instruction.Operand]!;
                        object? callable;
                        object? accumulator = null;
                        if (operation.Name == "fold")
                        {
                            callable = Pop();
                            accumulator = Pop();
                        }
                        else
                        {
                            callable = Pop();
                        }
                        var items = Pop();
                        RunHigherOrder(new HigherOrderState(operation, items, callable!, accumulator), null, false);
                        break;
                    }
                    case CoflowOpCode.Pop:
                        Pop();
                        break;
                    case CoflowOpCode.NegateInt:
                        Push(checked(-(long)Pop()!));
                        break;
                    case CoflowOpCode.NegateFloat:
                        Push(-(double)Pop()!);
                        break;
                    case CoflowOpCode.Not:
                        Push(!(bool)Pop()!);
                        break;
                    case CoflowOpCode.BitNot:
                        Push(~(long)Pop()!);
                        break;
                    case CoflowOpCode.AddInt:
                        BinaryLong((left, right) => checked(left + right));
                        break;
                    case CoflowOpCode.SubtractInt:
                        BinaryLong((left, right) => checked(left - right));
                        break;
                    case CoflowOpCode.MultiplyInt:
                        BinaryLong((left, right) => checked(left * right));
                        break;
                    case CoflowOpCode.DivideInt:
                    case CoflowOpCode.IntegerDivide:
                        BinaryLong((left, right) => checked(left / right));
                        break;
                    case CoflowOpCode.Remainder:
                        BinaryLong((left, right) => checked(left % right));
                        break;
                    case CoflowOpCode.PowerInt:
                        BinaryLong(PowerInt);
                        break;
                    case CoflowOpCode.PowerFloat:
                        BinaryDouble(Math.Pow);
                        break;
                    case CoflowOpCode.ShiftLeft:
                        BinaryLong((left, right) => checked(left << checked((int)right)));
                        break;
                    case CoflowOpCode.ShiftRight:
                        BinaryLong((left, right) => left >> checked((int)right));
                        break;
                    case CoflowOpCode.BitAnd: BinaryLong((left, right) => left & right); break;
                    case CoflowOpCode.BitXor: BinaryLong((left, right) => left ^ right); break;
                    case CoflowOpCode.BitOr: BinaryLong((left, right) => left | right); break;
                    case CoflowOpCode.AddFloat:
                        BinaryDouble((left, right) => left + right);
                        break;
                    case CoflowOpCode.SubtractFloat:
                        BinaryDouble((left, right) => left - right);
                        break;
                    case CoflowOpCode.MultiplyFloat:
                        BinaryDouble((left, right) => left * right);
                        break;
                    case CoflowOpCode.DivideFloat:
                        BinaryDouble((left, right) => left / right);
                        break;
                    case CoflowOpCode.AddString:
                    {
                        var right = (string)Pop()!;
                        var left = (string)Pop()!;
                        Push(left + right);
                        break;
                    }
                    case CoflowOpCode.LessInt: CompareLong((left, right) => left < right); break;
                    case CoflowOpCode.LessOrEqualInt: CompareLong((left, right) => left <= right); break;
                    case CoflowOpCode.GreaterInt: CompareLong((left, right) => left > right); break;
                    case CoflowOpCode.GreaterOrEqualInt: CompareLong((left, right) => left >= right); break;
                    case CoflowOpCode.LessFloat: CompareDouble((left, right) => left < right); break;
                    case CoflowOpCode.LessOrEqualFloat: CompareDouble((left, right) => left <= right); break;
                    case CoflowOpCode.GreaterFloat: CompareDouble((left, right) => left > right); break;
                    case CoflowOpCode.GreaterOrEqualFloat: CompareDouble((left, right) => left >= right); break;
                    case CoflowOpCode.LessString: CompareString(value => value < 0); break;
                    case CoflowOpCode.LessOrEqualString: CompareString(value => value <= 0); break;
                    case CoflowOpCode.GreaterString: CompareString(value => value > 0); break;
                    case CoflowOpCode.GreaterOrEqualString: CompareString(value => value >= 0); break;
                    case CoflowOpCode.JumpIfFalseKeep:
                        if (!(bool)stack[stackCount - 1]!) frame.Pc = instruction.Operand;
                        else Pop();
                        break;
                    case CoflowOpCode.JumpIfTrueKeep:
                        if ((bool)stack[stackCount - 1]!) frame.Pc = instruction.Operand;
                        else Pop();
                        break;
                    case CoflowOpCode.JumpIfFalse:
                        if (!(bool)Pop()!) frame.Pc = instruction.Operand;
                        break;
                    case CoflowOpCode.Jump:
                        frame.Pc = instruction.Operand;
                        break;
                    case CoflowOpCode.Call:
                    {
                        var call = (CoflowCallSite)frame.Program.Constants[instruction.Operand]!;
                        var target = call.Slot.CompiledProgram;
                        if (call.ArgumentCount == 1 &&
                            target?.SimpleIntegerFunction is { } simpleIntegerFunction)
                        {
                            var value = ExecuteSimpleIntegerFunction(
                                target, simpleIntegerFunction, (long)Pop()!, execution, tailCall: false);
                            var completedTailCall = false;
                            while (frame.Pc < frame.Program.Instructions.Length)
                            {
                                var nextInstruction = frame.Program.Instructions[frame.Pc];
                                if (nextInstruction.Code is not (CoflowOpCode.Call or CoflowOpCode.TailCall))
                                    break;
                                var nextCall = (CoflowCallSite)
                                    frame.Program.Constants[nextInstruction.Operand]!;
                                var nextTarget = nextCall.Slot.CompiledProgram;
                                if (nextCall.ArgumentCount != 1 ||
                                    nextTarget?.SimpleIntegerFunction is not { } nextFunction)
                                    break;

                                execution.ChargeInstruction();
                                frame.Pc++;
                                var tailCall = nextInstruction.Code == CoflowOpCode.TailCall;
                                value = ExecuteSimpleIntegerFunction(
                                    nextTarget, nextFunction, value, execution, tailCall);
                                if (!tailCall) continue;
                                if (CompleteFrame(value, out var returned)) return returned;
                                completedTailCall = true;
                                break;
                            }
                            if (!completedTailCall) Push(value);
                            break;
                        }
                        var callArguments = new object?[call.ArgumentCount];
                        for (var index = callArguments.Length - 1; index >= 0; index--)
                            callArguments[index] = Pop();
                        if (target is not null)
                        {
                            execution.EnterFrame(target.Identity);
                            frames.Push(new Frame(
                                target, callArguments, stackCount, ownsArguments: true));
                        }
                        else
                            Push(call.Slot.InvokeBoundFromVm(callArguments));
                        break;
                    }
                    case CoflowOpCode.CallIndirect:
                    {
                        var callArguments = new object?[instruction.Operand];
                        for (var index = callArguments.Length - 1; index >= 0; index--)
                            callArguments[index] = Pop();
                        var callable = Pop();
                        if (!TryScheduleCall(callable, callArguments, null, out var immediate))
                            Push(immediate);
                        break;
                    }
                    case CoflowOpCode.TailCall:
                    {
                        var call = (CoflowCallSite)frame.Program.Constants[instruction.Operand]!;
                        var target = call.Slot.CompiledProgram;
                        if (call.ArgumentCount == 1 &&
                            target?.SimpleIntegerFunction is { } simpleIntegerFunction)
                        {
                            var result = ExecuteSimpleIntegerFunction(
                                target, simpleIntegerFunction, (long)Pop()!, execution, tailCall: true);
                            if (CompleteFrame(result, out var returned)) return returned;
                            break;
                        }
                        var callArguments = frame.ReusableArguments(call.ArgumentCount)
                            ?? new object?[call.ArgumentCount];
                        for (var index = callArguments.Length - 1; index >= 0; index--)
                            callArguments[index] = Pop();
                        if (target is not null)
                        {
                            ReplaceFrame(target, callArguments);
                        }
                        else if (CompleteFrame(call.Slot.InvokeBoundFromVm(callArguments), out var returned))
                        {
                            return returned;
                        }
                        break;
                    }
                    case CoflowOpCode.TailCallIndirect:
                    {
                        var callArguments = new object?[instruction.Operand];
                        for (var index = callArguments.Length - 1; index >= 0; index--)
                            callArguments[index] = Pop();
                        var callable = Pop();
                        if (TryReplaceFrame(callable, callArguments, out var immediate)) break;
                        if (CompleteFrame(immediate, out var returned)) return returned;
                        break;
                    }
                    case CoflowOpCode.Return:
                    {
                        var result = Pop();
                        if (CompleteFrame(result, out var returned)) return returned;
                        break;
                    }
                    case CoflowOpCode.JumpIfLocalIntNotLessArgument:
                        execution.ChargeInstructions(3);
                        if (frame.IntegerLocals[instruction.Operand] >=
                            (long)frame.Arguments[instruction.Operand2]!)
                            frame.Pc = instruction.Operand3;
                        else
                            frame.Pc += 3;
                        break;
                    case CoflowOpCode.AddLocalIntsAndStore:
                        execution.ChargeInstructions(3);
                        frame.IntegerLocals[instruction.Operand3] = checked(
                            frame.IntegerLocals[instruction.Operand] +
                            frame.IntegerLocals[instruction.Operand2]);
                        frame.Pc += 3;
                        break;
                    case CoflowOpCode.AddLocalIntConstantAndStore:
                        execution.ChargeInstructions(3);
                        frame.IntegerLocals[instruction.Operand3] = checked(
                            frame.IntegerLocals[instruction.Operand] +
                            (long)frame.Program.Constants[instruction.Operand2]!);
                        frame.Pc += 3;
                        break;
                    case CoflowOpCode.AddIntegerFieldChainAndStore:
                        ExecuteIntegerFieldChain(frame, execution, instruction.Operand);
                        break;
                    case CoflowOpCode.RunIntegerLoop:
                        ExecuteIntegerLoop(frame, execution, instruction.Operand);
                        break;
                    case CoflowOpCode.RunTailIntegerCountdown:
                    {
                        var result = ExecuteTailIntegerCountdown(
                            frame, execution, instruction.Operand);
                        if (CompleteFrame(result, out var returned)) return returned;
                        break;
                    }
                    default:
                        throw new InvalidOperationException($"Unknown Coflow opcode `{instruction.Code}`.");
                }
            }
            throw new InvalidOperationException("Coflow VM stopped without a result.");
        }
        catch (CoflowFaultException error)
        {
            var caller = frames.TryPeek(out var frame) ? frame : null;
            var callerSpan = caller is not null && caller.Pc > 0 &&
                caller.Pc <= caller.Program.InstructionSpans.Length
                ? caller.Program.InstructionSpans[caller.Pc - 1]
                : null;
            throw error.WithCallers(
                frames.Select(item => item.Program.Identity),
                error.PreserveSourceLocation ? null : caller?.Program.SourcePath,
                error.PreserveSourceLocation ? null : callerSpan);
        }
        catch (Exception error)
        {
            var failed = frames.TryPeek(out var frame) ? frame.Program : program;
            var instructionSpan = frames.TryPeek(out frame) && frame.Pc > 0 &&
                frame.Pc <= frame.Program.InstructionSpans.Length
                ? frame.Program.InstructionSpans[frame.Pc - 1]
                : null;
            throw Fault(
                failed,
                error is System.Reflection.TargetInvocationException { InnerException: { } inner }
                    ? inner.Message
                    : error.Message,
                error is System.Reflection.TargetInvocationException { InnerException: { } target }
                    ? target
                    : error,
                execution.CallStack,
                instructionSpan);
        }
        finally
        {
            try
            {
                while (frames.Count != 0)
                {
                    frames.Pop();
                    execution.ExitFrame();
                }
                while (stackCount != 0) Pop();
            }
            finally
            {
                if (stackCount != 0) Array.Clear(stack, 0, stackCount);
                ArrayPool<object?>.Shared.Return(stack);
                _currentExecution = previousExecution;
                if (previousExecution is null) ReturnExecutionContext(execution);
            }
        }

        void BinaryLong(Func<long, long, long> operation)
        {
            var right = (long)Pop()!;
            var left = (long)Pop()!;
            Push(operation(left, right));
        }
        static long PowerInt(long value, long exponent)
        {
            if (exponent < 0) throw new InvalidOperationException("integer exponent must be non-negative");
            var result = 1L;
            var factor = value;
            while (exponent != 0)
            {
                if ((exponent & 1) != 0) result = checked(result * factor);
                exponent >>= 1;
                if (exponent != 0) factor = checked(factor * factor);
            }
            return result;
        }
        void BinaryDouble(Func<double, double, double> operation)
        {
            var right = (double)Pop()!;
            var left = (double)Pop()!;
            Push(operation(left, right));
        }
        void CompareLong(Func<long, long, bool> operation)
        {
            var right = (long)Pop()!;
            var left = (long)Pop()!;
            Push(operation(left, right));
        }
        void CompareDouble(Func<double, double, bool> operation)
        {
            var right = (double)Pop()!;
            var left = (double)Pop()!;
            Push(operation(left, right));
        }
        void CompareString(Func<int, bool> operation)
        {
            var right = (string)Pop()!;
            var left = (string)Pop()!;
            Push(operation(string.CompareOrdinal(left, right)));
        }

        bool CompleteFrame(object? result, out object? rootResult)
        {
            var completed = frames.Pop();
            execution.ExitFrame();
            while (stackCount > completed.StackBase) Pop();
            if (frames.Count == 0)
            {
                rootResult = result;
                return true;
            }
            if (completed.Continuation is { } continuation) continuation(result);
            else Push(result);
            rootResult = null;
            return false;
        }

        bool TryScheduleCall(
            object? callable,
            object?[] callArguments,
            Action<object?>? continuation,
            out object? immediate)
        {
            CoflowProgram? target = null;
            object?[] targetArguments = callArguments;
            if (callable is CoflowFunctionSlot slot)
            {
                target = slot.CompiledProgram;
                if (target is null)
                {
                    immediate = slot.InvokeBoundFromVm(callArguments);
                    return false;
                }
            }
            else if (callable is CoflowClosure closure)
            {
                target = closure.Program;
                targetArguments = CoflowClosure.Append(callArguments, closure.Captures);
            }
            else if (callable is Delegate implementation)
            {
                if (CoflowFunctionDelegates.TryGetSlot(implementation, out var delegateSlot))
                {
                    target = delegateSlot.CompiledProgram;
                    if (target is not null)
                        goto Schedule;
                    immediate = delegateSlot.InvokeBoundFromVm(callArguments);
                    return false;
                }
                if (CoflowFunctionDelegates.TryGetClosure(implementation, out var delegateClosure))
                {
                    target = delegateClosure.Program;
                    targetArguments = CoflowClosure.Append(callArguments, delegateClosure.Captures);
                    goto Schedule;
                }
                immediate = CoflowFunctionDelegates.InvokeAdapted(implementation, callArguments) ?? Unit.Value;
                return false;
            }
            else
            {
                throw new InvalidOperationException("Coflow indirect call target is not callable.");
            }
        Schedule:
            execution.EnterFrame(target.Identity);
            frames.Push(new Frame(
                target, targetArguments, stackCount, continuation, ownsArguments: true));
            immediate = null;
            return true;
        }

        bool TryReplaceFrame(object? callable, object?[] callArguments, out object? immediate)
        {
            CoflowProgram? target = null;
            object?[] targetArguments = callArguments;
            if (callable is CoflowFunctionSlot slot)
            {
                target = slot.CompiledProgram;
                if (target is null)
                {
                    immediate = slot.InvokeBoundFromVm(callArguments);
                    return false;
                }
            }
            else if (callable is CoflowClosure closure)
            {
                target = closure.Program;
                targetArguments = CoflowClosure.Append(callArguments, closure.Captures);
            }
            else if (callable is Delegate implementation)
            {
                if (CoflowFunctionDelegates.TryGetSlot(implementation, out var delegateSlot))
                {
                    target = delegateSlot.CompiledProgram;
                    if (target is not null)
                        goto Replace;
                    immediate = delegateSlot.InvokeBoundFromVm(callArguments);
                    return false;
                }
                if (CoflowFunctionDelegates.TryGetClosure(implementation, out var delegateClosure))
                {
                    target = delegateClosure.Program;
                    targetArguments = CoflowClosure.Append(callArguments, delegateClosure.Captures);
                    goto Replace;
                }
                immediate = CoflowFunctionDelegates.InvokeAdapted(implementation, callArguments) ?? Unit.Value;
                return false;
            }
            else
            {
                throw new InvalidOperationException("Coflow indirect call target is not callable.");
            }
        Replace:
            ReplaceFrame(target, targetArguments);
            immediate = null;
            return true;
        }

        void ReplaceFrame(CoflowProgram target, object?[] targetArguments)
        {
            var replaced = frames.Pop();
            while (stackCount > replaced.StackBase) Pop();
            execution.ReplaceFrame(target.Identity);
            replaced.Reset(target, targetArguments);
            frames.Push(replaced);
        }

        void RunHigherOrder(HigherOrderState state, object? callbackResult, bool hasResult)
        {
            while (true)
            {
                if (hasResult)
                {
                    var item = state.Operation.Item(state.Items, state.Index - 1);
                    switch (state.Operation.Name)
                    {
                        case "map": state.Output!.Add(callbackResult); break;
                        case "filter": if ((bool)callbackResult!) state.Output!.Add(item); break;
                        case "fold": state.Accumulator = callbackResult; break;
                        case "find":
                            if ((bool)callbackResult!)
                            {
                                state.ClearArguments();
                                Push(state.Operation.CreateSome(item));
                                return;
                            }
                            break;
                        case "any":
                            if ((bool)callbackResult!)
                            {
                                state.ClearArguments();
                                Push(true);
                                return;
                            }
                            break;
                        case "all":
                            if (!(bool)callbackResult!)
                            {
                                state.ClearArguments();
                                Push(false);
                                return;
                            }
                            break;
                    }
                }
                if (state.Index >= state.Count)
                {
                    var result = state.Operation.Name switch
                    {
                        "map" or "filter" => state.Operation.CreateArray(state.Output!.ToArray()),
                        "fold" => state.Accumulator,
                        "find" => state.Operation.None,
                        "any" => false,
                        "all" => true,
                        _ => throw new InvalidOperationException("unknown higher-order operation"),
                    };
                    state.ClearArguments();
                    Push(result);
                    return;
                }
                var current = state.Operation.Item(state.Items, state.Index++);
                execution.ChargeInstruction();
                state.SetArguments(current);
                if (TryScheduleCall(state.Callable, state.CallbackArguments,
                        result => RunHigherOrder(state, result, true), out var immediate))
                    return;
                callbackResult = immediate;
                hasResult = true;
            }
        }
    }

    [System.Runtime.CompilerServices.MethodImpl(
        System.Runtime.CompilerServices.MethodImplOptions.NoInlining)]
    private static void ExecuteIntegerFieldChain(
        Frame frame,
        CoflowExecutionContext execution,
        int constantIndex)
    {
        var chain = (CoflowIntegerFieldChain)frame.Program.Constants[constantIndex]!;
        execution.ChargeInstructions(chain.InstructionCount - 1);
        var receiver = chain.Receiver;
        for (var index = 0; index < chain.Accesses.Length - 1; index++)
            receiver = chain.Accesses[index].Read(receiver)!;
        frame.IntegerLocals[chain.TargetLocal] = checked(
            frame.IntegerLocals[chain.SourceLocal] +
            chain.Accesses[^1].ReadInteger(receiver));
        frame.Pc += chain.InstructionCount - 1;
    }

    [System.Runtime.CompilerServices.MethodImpl(
        System.Runtime.CompilerServices.MethodImplOptions.NoInlining)]
    private static void ExecuteIntegerLoop(
        Frame frame,
        CoflowExecutionContext execution,
        int constantIndex)
    {
        var loop = (CoflowIntegerLoop)frame.Program.Constants[constantIndex]!;
        var limit = (long)frame.Arguments[loop.LimitArgument]!;
        var conditionCost = 3L;
        while (true)
        {
            execution.ChargeInstructions(conditionCost);
            conditionCost = 4;
            if (frame.IntegerLocals[loop.ConditionLocal] >= limit) break;
            foreach (var operation in loop.Operations)
            {
                execution.ChargeInstructions(operation.InstructionCount);
                frame.Pc = operation.SourcePc + 1;
                switch (operation.Kind)
                {
                    case CoflowIntegerLoopOperationKind.AddLocals:
                        frame.IntegerLocals[operation.TargetLocal] = checked(
                            frame.IntegerLocals[operation.SourceLocal] +
                            frame.IntegerLocals[operation.RightLocal]);
                        break;
                    case CoflowIntegerLoopOperationKind.AddConstant:
                        frame.IntegerLocals[operation.TargetLocal] = checked(
                            frame.IntegerLocals[operation.SourceLocal] + operation.Constant);
                        break;
                    case CoflowIntegerLoopOperationKind.AddFieldChain:
                    {
                        var receiver = operation.Receiver!;
                        var accesses = operation.Accesses!;
                        for (var index = 0; index < accesses.Length - 1; index++)
                            receiver = accesses[index].Read(receiver)!;
                        frame.IntegerLocals[operation.TargetLocal] = checked(
                            frame.IntegerLocals[operation.SourceLocal] +
                            accesses[^1].ReadInteger(receiver));
                        break;
                    }
                    default:
                        throw new InvalidOperationException("Unknown integer loop operation.");
                }
            }
            execution.ChargeInstruction();
            frame.Pc = loop.StartPc + 1;
        }
        frame.Pc = loop.EndPc;
    }

    [System.Runtime.CompilerServices.MethodImpl(
        System.Runtime.CompilerServices.MethodImplOptions.NoInlining)]
    private static object? ExecuteTailIntegerCountdown(
        Frame frame,
        CoflowExecutionContext execution,
        int constantIndex)
    {
        var countdown = (CoflowTailIntegerCountdown)frame.Program.Constants[constantIndex]!;
        var value = (long)frame.Arguments[countdown.Argument]!;
        var conditionCost = 3L;
        while (true)
        {
            frame.Pc = countdown.ComparisonPc + 1;
            execution.ChargeInstructions(conditionCost);
            conditionCost = 4;
            if (value <= countdown.Threshold)
            {
                frame.Pc = countdown.ReturnPc + 1;
                execution.ChargeInstructions(2);
                return countdown.Result;
            }

            frame.Pc = countdown.SubtractPc + 1;
            execution.ChargeInstructions(4);
            value = countdown.Subtract
                ? checked(value - countdown.Step)
                : checked(value + countdown.Step);
        }
    }

    [System.Runtime.CompilerServices.MethodImpl(
        System.Runtime.CompilerServices.MethodImplOptions.NoInlining)]
    private static long ExecuteSimpleIntegerFunction(
        CoflowProgram program,
        CoflowSimpleIntegerFunction function,
        long argument,
        CoflowExecutionContext execution,
        bool tailCall)
    {
        if (tailCall) execution.ReplaceFrame(program.Identity);
        else execution.EnterFrame(program.Identity);
        try
        {
            execution.EnsureStackCapacity(2);
            execution.ChargeInstructions(4);
            return function.Operation switch
            {
                CoflowOpCode.AddInt => checked(argument + function.Constant),
                CoflowOpCode.SubtractInt => checked(argument - function.Constant),
                CoflowOpCode.MultiplyInt => checked(argument * function.Constant),
                _ => throw new InvalidOperationException("Unknown simple integer operation."),
            };
        }
        catch (OverflowException error)
        {
            throw Fault(
                program,
                error.Message,
                error,
                execution.CallStack,
                program.InstructionSpans[function.OperationPc],
                preserveSourceLocation: true);
        }
        catch (Exception error)
        {
            throw Fault(
                program,
                error.Message,
                error,
                execution.CallStack,
                preserveSourceLocation: true);
        }
        finally
        {
            if (!tailCall) execution.ExitFrame();
        }
    }

    private static CoflowFaultException Fault(
        CoflowProgram program,
        string message,
        Exception? inner = null,
        IEnumerable<CoflowFunctionIdentity>? callStack = null,
        CfdSpan? sourceSpan = null,
        bool preserveSourceLocation = false) =>
        new(
            program.Identity,
            program.SourcePath,
            sourceSpan ?? program.SourceSpan,
            (callStack ?? new[] { program.Identity }).Take(32).ToArray(),
            message,
            inner,
            preserveSourceLocation);

    private static CoflowExecutionContext RentExecutionContext(long instructionLimit)
    {
        var execution = _pooledExecution ?? new CoflowExecutionContext();
        _pooledExecution = null;
        execution.Reset(instructionLimit);
        return execution;
    }

    private static void ReturnExecutionContext(CoflowExecutionContext execution)
    {
        execution.Clear();
        _pooledExecution = execution;
    }

    private sealed class Frame
    {
        internal Frame(
            CoflowProgram program,
            object?[] arguments,
            int stackBase,
            Action<object?>? continuation = null,
            bool ownsArguments = false)
        {
            Program = program;
            Arguments = arguments;
            Locals = new object?[program.LocalCount];
            IntegerLocals = program.IntegerLocalCount == 0
                ? Array.Empty<long>()
                : new long[program.IntegerLocalCount];
            StackBase = stackBase;
            Continuation = continuation;
            OwnsArguments = ownsArguments;
        }

        internal CoflowProgram Program { get; private set; }
        internal object?[] Arguments { get; private set; }
        internal object?[] Locals { get; private set; }
        internal long[] IntegerLocals { get; private set; }
        internal int StackBase { get; }
        internal Action<object?>? Continuation { get; }
        internal bool OwnsArguments { get; private set; }
        internal int Pc { get; set; }

        internal object?[]? ReusableArguments(int count) =>
            OwnsArguments && Arguments.Length == count ? Arguments : null;

        internal void Reset(CoflowProgram program, object?[] arguments)
        {
            Program = program;
            Arguments = arguments;
            OwnsArguments = true;
            if (Locals.Length == program.LocalCount &&
                IntegerLocals.Length == program.IntegerLocalCount)
            {
                Array.Clear(Locals, 0, Locals.Length);
                Array.Clear(IntegerLocals, 0, IntegerLocals.Length);
            }
            else
            {
                Locals = new object?[program.LocalCount];
                IntegerLocals = program.IntegerLocalCount == 0
                    ? Array.Empty<long>()
                    : new long[program.IntegerLocalCount];
            }
            Pc = 0;
        }

    }

    private sealed class HigherOrderState(
        CoflowHigherOrderOperation operation,
        object? items,
        object callable,
        object? accumulator)
    {
        internal CoflowHigherOrderOperation Operation { get; } = operation;
        internal object? Items { get; } = items;
        internal int Count { get; } = operation.Count(items);
        internal object Callable { get; } = callable;
        internal object? Accumulator { get; set; } = accumulator;
        internal List<object?>? Output { get; } = operation.Name is "map" or "filter"
            ? new List<object?>(operation.Count(items))
            : null;
        internal object?[] CallbackArguments { get; } = new object?[operation.Name == "fold" ? 2 : 1];
        internal int Index { get; set; }

        internal void SetArguments(object? current)
        {
            if (CallbackArguments.Length == 2) CallbackArguments[0] = Accumulator;
            CallbackArguments[^1] = current;
        }

        internal void ClearArguments() => Array.Clear(CallbackArguments, 0, CallbackArguments.Length);
    }

    private sealed class CoflowExecutionContext
    {
        private readonly List<CoflowFunctionIdentity> _callStack = new();
        private long _instructionLimit;
        private long _instructions;
        private int _stackValues;

        internal IReadOnlyList<CoflowFunctionIdentity> CallStack =>
            _callStack.AsEnumerable().Reverse().Distinct().Take(32).ToArray();

        internal void Reset(long instructionLimit)
        {
            _instructionLimit = instructionLimit;
            _instructions = 0;
            _stackValues = 0;
            _callStack.Clear();
        }

        internal void Clear()
        {
            _instructionLimit = 0;
            _instructions = 0;
            _stackValues = 0;
            _callStack.Clear();
        }

        internal void ChargeInstruction()
        {
            ChargeInstructions(1);
        }

        internal void ChargeInstructions(long count)
        {
            if (count < 0 || _instructions > _instructionLimit - count)
                throw new InvalidOperationException("Coflow VM instruction budget exceeded.");
            _instructions += count;
        }

        internal void EnterFrame(CoflowFunctionIdentity identity)
        {
            if (_callStack.Count >= MaximumFrames)
                throw new InvalidOperationException("Coflow VM call depth budget exceeded.");
            _callStack.Add(identity);
        }

        internal void ExitFrame()
        {
            if (_callStack.Count == 0)
                throw new InvalidOperationException("Coflow VM frame budget underflow.");
            _callStack.RemoveAt(_callStack.Count - 1);
        }

        internal void ReplaceFrame(CoflowFunctionIdentity identity)
        {
            if (_callStack.Count == 0)
                throw new InvalidOperationException("Coflow VM frame budget underflow.");
            _callStack[^1] = identity;
        }

        internal void PushValue()
        {
            if (_stackValues >= MaximumStackValues)
                throw new InvalidOperationException("Coflow VM value stack budget exceeded.");
            _stackValues++;
        }

        internal void EnsureStackCapacity(int additional)
        {
            if (additional < 0 || _stackValues > MaximumStackValues - additional)
                throw new InvalidOperationException("Coflow VM value stack budget exceeded.");
        }

        internal void PopValue()
        {
            if (_stackValues == 0)
                throw new InvalidOperationException("Coflow VM value stack budget underflow.");
            _stackValues--;
        }
    }

    private sealed class InstructionLimitScope(long? previous) : IDisposable
    {
        private bool _disposed;

        public void Dispose()
        {
            if (_disposed) return;
            _instructionLimitOverride = previous;
            _disposed = true;
        }
    }

    internal static void ChargeWork(long units)
    {
        if (units > 0) _currentExecution?.ChargeInstructions(units);
    }
}
