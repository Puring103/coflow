namespace Coflow.Runtime.CompilerServices;

using System.Buffers;
using System.Runtime.CompilerServices;

internal readonly record struct CoflowRegisterWindow(
    int IntegerBase,
    int FloatBase,
    int ReferenceBase,
    int IntegerTop,
    int FloatTop,
    int ReferenceTop);

/// <summary>唯一拥有寄存器数组、当前窗口、codec 临时区和池化清理协议。</summary>
internal sealed class CoflowRegisterStorage
{
    private long[] _integers = ArrayPool<long>.Shared.Rent(32);
    private double[] _floats = ArrayPool<double>.Shared.Rent(16);
    private object?[] _references = RentCleared<object>(32);
    private readonly int _baselineIntegerCapacity;
    private readonly int _baselineFloatCapacity;
    private readonly int _baselineReferenceCapacity;
    private int _integerBase;
    private int _floatBase;
    private int _referenceBase;
    private int _integerTop;
    private int _floatTop;
    private int _referenceTop;
    private int _referenceHighWater;
    private int _codecIntegerTop;
    private int _codecFloatTop;
    private int _codecReferenceTop;

    internal CoflowRegisterStorage()
    {
        _baselineIntegerCapacity = _integers.Length;
        _baselineFloatCapacity = _floats.Length;
        _baselineReferenceCapacity = _references.Length;
    }

    internal int IntegerBase => _integerBase;
    internal int FloatBase => _floatBase;
    internal int ReferenceBase => _referenceBase;

    internal CoflowValueView View(CoflowValueRegister register) =>
        new(_integers, _floats, _references, register.IntegerBase, register.FloatBase, register.ReferenceBase);

    internal CoflowRegisterWindow Window => new(
        _integerBase, _floatBase, _referenceBase,
        _integerTop, _floatTop, _referenceTop);

    internal void Reset()
    {
        _integerBase = 0;
        _floatBase = 0;
        _referenceBase = 0;
        _integerTop = 0;
        _floatTop = 0;
        _referenceTop = 0;
        _referenceHighWater = 0;
        _codecIntegerTop = 0;
        _codecFloatTop = 0;
        _codecReferenceTop = 0;
    }

    internal CoflowValueRegister Offset(CoflowValueRegister register) => register with
    {
        IntegerBase = register.IntegerBase + _integerBase,
        FloatBase = register.FloatBase + _floatBase,
        ReferenceBase = register.ReferenceBase + _referenceBase
    };

    internal static CoflowValueRegister Absolute(CoflowValueRegister register, CoflowRegisterWindow window) =>
        register with
        {
            IntegerBase = register.IntegerBase + window.IntegerBase,
            FloatBase = register.FloatBase + window.FloatBase,
            ReferenceBase = register.ReferenceBase + window.ReferenceBase
        };

    internal long ReadInteger(CoflowRegister register) => _integers[register.Index];
    internal double ReadFloat(CoflowRegister register) => _floats[register.Index];
    internal object? ReadReference(CoflowRegister register) => _references[register.Index];
    internal void WriteInteger(CoflowRegister register, long value) => _integers[register.Index] = value;
    internal void WriteFloat(CoflowRegister register, double value) => _floats[register.Index] = value;
    internal void WriteReference(CoflowRegister register, object? value) => _references[register.Index] = value;
    internal long ReadIntegerRelative(int index) => _integers[_integerBase + index];
    internal double ReadFloatRelative(int index) => _floats[_floatBase + index];
    internal object? ReadReferenceRelative(int index) => _references[_referenceBase + index];
    internal void WriteIntegerRelative(int index, long value) => _integers[_integerBase + index] = value;
    internal void WriteFloatRelative(int index, double value) => _floats[_floatBase + index] = value;
    internal void WriteReferenceRelative(int index, object? value) => _references[_referenceBase + index] = value;

    internal void Copy(CoflowValueRegister source, CoflowValueRegister target)
    {
        RequireSamePhysicalLayout(source.Shape, target.Shape);
        CopyBank(_integers, source.IntegerBase, target.IntegerBase, source.Shape.IntegerCount);
        CopyBank(_floats, source.FloatBase, target.FloatBase, source.Shape.FloatCount);
        CopyBank(_references, source.ReferenceBase, target.ReferenceBase, source.Shape.ReferenceCount);
    }

    internal void CopyRelative(CoflowValueRegister source, CoflowValueRegister target)
    {
        RequireSamePhysicalLayout(source.Shape, target.Shape);
        CopyBank(_integers, _integerBase + source.IntegerBase, _integerBase + target.IntegerBase, source.Shape.IntegerCount);
        CopyBank(_floats, _floatBase + source.FloatBase, _floatBase + target.FloatBase, source.Shape.FloatCount);
        CopyBank(_references, _referenceBase + source.ReferenceBase, _referenceBase + target.ReferenceBase, source.Shape.ReferenceCount);
    }

    internal void WriteEncodedRelative(CoflowEncodedValue source, CoflowValueRegister target)
    {
        if (source.Integers.Length != 0)
            Array.Copy(source.Integers, 0, _integers, _integerBase + target.IntegerBase, source.Integers.Length);
        if (source.Floats.Length != 0)
            Array.Copy(source.Floats, 0, _floats, _floatBase + target.FloatBase, source.Floats.Length);
        if (source.References.Length != 0)
            Array.Copy(source.References, 0, _references, _referenceBase + target.ReferenceBase, source.References.Length);
    }

    internal void EnterDirect(CoflowRegisterCallSite site, CoflowRegisterWindow previous)
    {
        _integerBase = site.IntegerWindowBase < 0 ? previous.IntegerTop : previous.IntegerBase + site.IntegerWindowBase;
        _floatBase = site.FloatWindowBase < 0 ? previous.FloatTop : previous.FloatBase + site.FloatWindowBase;
        _referenceBase = site.ReferenceWindowBase < 0 ? previous.ReferenceTop : previous.ReferenceBase + site.ReferenceWindowBase;
    }

    internal void EnterAfter(CoflowRegisterWindow previous)
    {
        _integerBase = previous.IntegerTop;
        _floatBase = previous.FloatTop;
        _referenceBase = previous.ReferenceTop;
    }

    internal void Reserve(CoflowRegisterProgram program, CoflowExecutionBudgetLease budget)
    {
        checked
        {
            _integerTop = _integerBase + program.IntegerRegisterCount;
            _floatTop = _floatBase + program.FloatRegisterCount;
            _referenceTop = _referenceBase + program.ReferenceRegisterCount;
            budget.AcquireRegisters(_integerTop, _floatTop, _referenceTop);
            Ensure(ref _integers, _integerTop);
            Ensure(ref _floats, _floatTop);
            Ensure(ref _references, _referenceTop);
            _referenceHighWater = Math.Max(_referenceHighWater, _referenceTop);
        }
    }

    internal void CompactTailWindow(CoflowRegisterProgram program, CoflowRegisterWindow previous)
    {
        if (program.ParameterIntegerCount != 0)
            Array.Copy(_integers, _integerBase, _integers, previous.IntegerBase, program.ParameterIntegerCount);
        if (program.ParameterFloatCount != 0)
            Array.Copy(_floats, _floatBase, _floats, previous.FloatBase, program.ParameterFloatCount);
        if (program.ParameterReferenceCount != 0)
            Array.Copy(_references, _referenceBase, _references, previous.ReferenceBase, program.ParameterReferenceCount);

        int retainedReferenceTop = previous.ReferenceBase + program.ParameterReferenceCount;
        int clearReferenceTop = Math.Max(previous.ReferenceTop,
            checked(previous.ReferenceBase + program.ReferenceRegisterCount));
        if (clearReferenceTop > retainedReferenceTop)
            Array.Clear(_references, retainedReferenceTop, clearReferenceTop - retainedReferenceTop);

        _integerBase = previous.IntegerBase;
        _floatBase = previous.FloatBase;
        _referenceBase = previous.ReferenceBase;
        checked
        {
            _integerTop = _integerBase + program.IntegerRegisterCount;
            _floatTop = _floatBase + program.FloatRegisterCount;
            _referenceTop = _referenceBase + program.ReferenceRegisterCount;
        }
    }

    internal void Restore(CoflowFrame frame, CoflowRegisterProgram program)
    {
        _integerBase = frame.IntegerBase;
        _floatBase = frame.FloatBase;
        _referenceBase = frame.ReferenceBase;
        checked
        {
            _integerTop = _integerBase + program.IntegerRegisterCount;
            _floatTop = _floatBase + program.FloatRegisterCount;
            _referenceTop = _referenceBase + program.ReferenceRegisterCount;
        }
    }

    internal T DecodeEncoded<T>(CoflowExecutionSession session, CoflowEncodedValue source)
    {
        CoflowValueShape shape = CoflowValueShape.Of(typeof(T));
        RequireSamePhysicalLayout(source.Shape, shape);
        int integerBase = Math.Max(_integerTop, _codecIntegerTop);
        int floatBase = Math.Max(_floatTop, _codecFloatTop);
        int referenceBase = Math.Max(_referenceTop, _codecReferenceTop);
        int oldIntegerTop = _codecIntegerTop;
        int oldFloatTop = _codecFloatTop;
        int oldReferenceTop = _codecReferenceTop;
        checked
        {
            Ensure(ref _integers, integerBase + shape.IntegerCount);
            Ensure(ref _floats, floatBase + shape.FloatCount);
            Ensure(ref _references, referenceBase + shape.ReferenceCount);
            _codecIntegerTop = integerBase + shape.IntegerCount;
            _codecFloatTop = floatBase + shape.FloatCount;
            _codecReferenceTop = referenceBase + shape.ReferenceCount;
        }
        try
        {
            Array.Copy(source.Integers, 0, _integers, integerBase, source.Integers.Length);
            Array.Copy(source.Floats, 0, _floats, floatBase, source.Floats.Length);
            Array.Copy(source.References, 0, _references, referenceBase, source.References.Length);
            return CoflowBoundaryCodec<T>.Read(session,
                new CoflowValueRegister(shape, integerBase, floatBase, referenceBase));
        }
        finally
        {
            if (shape.ReferenceCount != 0) Array.Clear(_references, referenceBase, shape.ReferenceCount);
            _codecIntegerTop = oldIntegerTop;
            _codecFloatTop = oldFloatTop;
            _codecReferenceTop = oldReferenceTop;
        }
    }

    internal void ClearCurrentReferences()
    {
        if (_referenceTop > _referenceBase)
            Array.Clear(_references, _referenceBase, _referenceTop - _referenceBase);
    }

    internal void ClearAndTrim()
    {
        if (_referenceHighWater != 0) Array.Clear(_references, 0, _referenceHighWater);
        Reset();
        TrimToBaseline(ref _integers, _baselineIntegerCapacity);
        TrimToBaseline(ref _floats, _baselineFloatCapacity);
        TrimToBaseline(ref _references, _baselineReferenceCapacity);
    }

    private static void RequireSamePhysicalLayout(CoflowValueShape actual, CoflowValueShape expected)
    {
        if (actual.IntegerCount != expected.IntegerCount || actual.FloatCount != expected.FloatCount ||
            actual.ReferenceCount != expected.ReferenceCount)
            throw new InvalidOperationException($"register value layout mismatch: `{actual.Type}` to `{expected.Type}`");
    }

    private static void CopyBank<T>(T[] values, int source, int target, int count)
    {
        if (count == 0 || source == target) return;
        if (count == 1) values[target] = values[source];
        else Array.Copy(values, source, values, target, count);
    }

    private static void Ensure<T>(ref T[] values, int count)
    {
        if (count <= values.Length) return;
        T[] expanded = ArrayPool<T>.Shared.Rent(Math.Max(count, checked(values.Length * 2)));
        bool clearReferences = RuntimeHelpers.IsReferenceOrContainsReferences<T>();
        if (clearReferences) Array.Clear(expanded, 0, expanded.Length);
        Array.Copy(values, expanded, values.Length);
        if (clearReferences) Array.Clear(values, 0, values.Length);
        ArrayPool<T>.Shared.Return(values);
        values = expanded;
    }

    private static T[] RentCleared<T>(int count)
    {
        T[] values = ArrayPool<T>.Shared.Rent(count);
        Array.Clear(values, 0, values.Length);
        return values;
    }

    private static void TrimToBaseline<T>(ref T[] values, int baselineCapacity)
    {
        if (values.Length <= baselineCapacity) return;
        T[] expanded = values;
        values = RentCleared<T>(baselineCapacity);
        ArrayPool<T>.Shared.Return(expanded);
    }
}
