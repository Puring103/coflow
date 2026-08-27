namespace CoflowRuntime;

using System.Buffers;

internal enum CoflowOpCode : byte
{
    Constant,
    Argument,
    Local,
    StoreLocal,
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
}

internal readonly record struct CoflowInstruction(CoflowOpCode Code, int Operand = 0);
internal readonly record struct CoflowCallSite(CoflowFunctionSlot Slot, int ArgumentCount);
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
        Instructions = instructions as CoflowInstruction[] ?? instructions.ToArray();
        InstructionSpans = instructionSpans as CfdSpan?[] ?? instructionSpans.ToArray();
        Constants = constants as object?[] ?? constants.ToArray();
        ParameterCount = parameterCount;
        LocalCount = localCount;
    }

    internal CoflowFunctionIdentity Identity { get; }
    internal string SourcePath { get; }
    internal CfdSpan? SourceSpan { get; }
    internal CoflowInstruction[] Instructions { get; }
    internal CfdSpan?[] InstructionSpans { get; }
    internal object?[] Constants { get; }
    internal int ParameterCount { get; }
    internal int LocalCount { get; }
}

public sealed class CoflowFaultException : Exception
{
    internal CoflowFaultException(
        CoflowFunctionIdentity function,
        string sourcePath,
        CfdSpan? sourceSpan,
        IReadOnlyList<CoflowFunctionIdentity> callStack,
        string message,
        Exception? inner = null)
        : base(message, inner)
    {
        Function = function;
        SourcePath = sourcePath;
        SourceSpan = sourceSpan;
        CallStack = callStack;
    }

    public CoflowFunctionIdentity Function { get; }
    public string SourcePath { get; }
    public CfdSpan? SourceSpan { get; }
    public IReadOnlyList<CoflowFunctionIdentity> CallStack { get; }

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
            InnerException);
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
        var execution = previousExecution ?? new CoflowExecutionContext(
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
                        var callArguments = new object?[call.ArgumentCount];
                        for (var index = callArguments.Length - 1; index >= 0; index--)
                            callArguments[index] = Pop();
                        var target = call.Slot.CompiledProgram;
                        if (target is not null)
                        {
                            execution.EnterFrame(target.Identity);
                            frames.Push(new Frame(target, callArguments, stackCount, ownsArguments: true));
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
                        var callArguments = frame.ReusableArguments(call.ArgumentCount)
                            ?? new object?[call.ArgumentCount];
                        for (var index = callArguments.Length - 1; index >= 0; index--)
                            callArguments[index] = Pop();
                        var target = call.Slot.CompiledProgram;
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
                caller?.Program.SourcePath,
                callerSpan);
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
            frames.Push(new Frame(target, targetArguments, stackCount, continuation, ownsArguments: true));
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

    private static CoflowFaultException Fault(
        CoflowProgram program,
        string message,
        Exception? inner = null,
        IEnumerable<CoflowFunctionIdentity>? callStack = null,
        CfdSpan? sourceSpan = null) =>
        new(
            program.Identity,
            program.SourcePath,
            sourceSpan ?? program.SourceSpan,
            (callStack ?? new[] { program.Identity }).Take(32).ToArray(),
            message,
            inner);

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
            StackBase = stackBase;
            Continuation = continuation;
            OwnsArguments = ownsArguments;
        }

        internal CoflowProgram Program { get; private set; }
        internal object?[] Arguments { get; private set; }
        internal object?[] Locals { get; private set; }
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
            if (Locals.Length == program.LocalCount)
                Array.Clear(Locals, 0, Locals.Length);
            else
                Locals = new object?[program.LocalCount];
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

    private sealed class CoflowExecutionContext(long instructionLimit)
    {
        private readonly List<CoflowFunctionIdentity> _callStack = new();
        private readonly long _instructionLimit = instructionLimit;
        private long _instructions;
        private int _stackValues;

        internal IReadOnlyList<CoflowFunctionIdentity> CallStack =>
            _callStack.AsEnumerable().Reverse().Distinct().Take(32).ToArray();

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
