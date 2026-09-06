using System;
using System.Buffers;
using System.Collections.Generic;
using System.Linq;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

namespace Coflow.Runtime.CompilerServices;

internal static class CoflowVm
{
    internal interface ICoflowArguments
    {
        int Count { get; }

        void Write(CoflowExecutionContext context);
    }

    [StructLayout(LayoutKind.Sequential, Size = 1)]
    private readonly record struct Arguments0 : ICoflowArguments
    {
        public int Count => 0;

        public void Write(CoflowExecutionContext context)
        {
        }
    }

    private readonly record struct Arguments1<T1>(T1 Arg1) : ICoflowArguments
    {
        public int Count => 1;

        public void Write(CoflowExecutionContext context)
        {
            context.Write(context.Parameter(0), Arg1);
        }
    }

    private readonly record struct Arguments2<T1, T2>(T1 Arg1, T2 Arg2) : ICoflowArguments
    {
        public int Count => 2;

        public void Write(CoflowExecutionContext context)
        {
            context.Write(context.Parameter(0), Arg1);
            context.Write(context.Parameter(1), Arg2);
        }
    }

    private readonly record struct Arguments3<T1, T2, T3>(T1 Arg1, T2 Arg2, T3 Arg3) : ICoflowArguments
    {
        public int Count => 3;

        public void Write(CoflowExecutionContext context)
        {
            context.Write(context.Parameter(0), Arg1);
            context.Write(context.Parameter(1), Arg2);
            context.Write(context.Parameter(2), Arg3);
        }
    }

    private readonly record struct Arguments4<T1, T2, T3, T4>(T1 Arg1, T2 Arg2, T3 Arg3, T4 Arg4) : ICoflowArguments
    {
        public int Count => 4;

        public void Write(CoflowExecutionContext context)
        {
            context.Write(context.Parameter(0), Arg1);
            context.Write(context.Parameter(1), Arg2);
            context.Write(context.Parameter(2), Arg3);
            context.Write(context.Parameter(3), Arg4);
        }
    }

    private readonly record struct Arguments5<T1, T2, T3, T4, T5>(T1 Arg1, T2 Arg2, T3 Arg3, T4 Arg4, T5 Arg5) : ICoflowArguments
    {
        public int Count => 5;

        public void Write(CoflowExecutionContext context)
        {
            context.Write(context.Parameter(0), Arg1);
            context.Write(context.Parameter(1), Arg2);
            context.Write(context.Parameter(2), Arg3);
            context.Write(context.Parameter(3), Arg4);
            context.Write(context.Parameter(4), Arg5);
        }
    }

    private readonly record struct Arguments6<T1, T2, T3, T4, T5, T6>(T1 Arg1, T2 Arg2, T3 Arg3, T4 Arg4, T5 Arg5, T6 Arg6) : ICoflowArguments
    {
        public int Count => 6;

        public void Write(CoflowExecutionContext context)
        {
            context.Write(context.Parameter(0), Arg1);
            context.Write(context.Parameter(1), Arg2);
            context.Write(context.Parameter(2), Arg3);
            context.Write(context.Parameter(3), Arg4);
            context.Write(context.Parameter(4), Arg5);
            context.Write(context.Parameter(5), Arg6);
        }
    }

    private readonly record struct Arguments7<T1, T2, T3, T4, T5, T6, T7>(T1 Arg1, T2 Arg2, T3 Arg3, T4 Arg4, T5 Arg5, T6 Arg6, T7 Arg7) : ICoflowArguments
    {
        public int Count => 7;

        public void Write(CoflowExecutionContext context)
        {
            context.Write(context.Parameter(0), Arg1);
            context.Write(context.Parameter(1), Arg2);
            context.Write(context.Parameter(2), Arg3);
            context.Write(context.Parameter(3), Arg4);
            context.Write(context.Parameter(4), Arg5);
            context.Write(context.Parameter(5), Arg6);
            context.Write(context.Parameter(6), Arg7);
        }
    }

    private readonly record struct Arguments8<T1, T2, T3, T4, T5, T6, T7, T8>(T1 Arg1, T2 Arg2, T3 Arg3, T4 Arg4, T5 Arg5, T6 Arg6, T7 Arg7, T8 Arg8) : ICoflowArguments
    {
        public int Count => 8;

        public void Write(CoflowExecutionContext context)
        {
            context.Write(context.Parameter(0), Arg1);
            context.Write(context.Parameter(1), Arg2);
            context.Write(context.Parameter(2), Arg3);
            context.Write(context.Parameter(3), Arg4);
            context.Write(context.Parameter(4), Arg5);
            context.Write(context.Parameter(5), Arg6);
            context.Write(context.Parameter(6), Arg7);
            context.Write(context.Parameter(7), Arg8);
        }
    }

    private readonly record struct RawArguments1<T1>(T1 Arg1) : ICoflowArguments
    {
        public int Count => 1;

        public void Write(CoflowExecutionContext context)
        {
            CoflowBoundaryCodec<T1>.WriteRelative(context, context.Program.RegisterProgram.Parameters[0], Arg1);
        }
    }

    private readonly record struct ReceiverArguments<TReceiver, TArguments>(TReceiver Receiver, TArguments Arguments) : ICoflowArguments where TArguments : struct, ICoflowArguments
    {
        public int Count => Arguments.Count + 1;

        public void Write(CoflowExecutionContext context)
        {
            Arguments.Write(context);
            context.Write(context.Parameter(Arguments.Count), Receiver);
        }
    }

    private readonly record struct BoxedReceiverArguments<TArguments>(object Receiver, TArguments Arguments) : ICoflowArguments where TArguments : struct, ICoflowArguments
    {
        public int Count => Arguments.Count + 1;

        public void Write(CoflowExecutionContext context)
        {
            Arguments.Write(context);
            CoflowValueRegister target = context.Parameter(Arguments.Count);
            context.WriteBoxed(target, Receiver);
        }
    }

    private readonly record struct ClosureArguments<TArguments>(CoflowClosure Closure, TArguments Arguments) : ICoflowArguments where TArguments : struct, ICoflowArguments
    {
        public int Count => Arguments.Count + Closure.Captures.Count;

        public void Write(CoflowExecutionContext context)
        {
            Arguments.Write(context);
            context.WriteCaptures(Closure, Arguments.Count);
        }
    }

    private struct CoflowFrame
    {
        internal CoflowProgram Program;

        internal int ReturnPc;

        internal int IntegerBase;

        internal int FloatBase;

        internal int ReferenceBase;

        internal CoflowValueRegister ReturnTarget;
    }

    internal sealed class CoflowExecutionContext : IDisposable
    {
        private long[] _integers = ArrayPool<long>.Shared.Rent(32);

        private double[] _floats = ArrayPool<double>.Shared.Rent(16);

        private object?[] _references = RentCleared<object>(32);

        private CoflowFrame[] _frames = RentCleared<CoflowFrame>(16);

        private readonly CoflowCollectionArena _collections = new CoflowCollectionArena();

        private IReadOnlyList<CoflowCollectionArena> _capturedCollections = Array.Empty<CoflowCollectionArena>();

        private int _frameCount;

        private int _frameHighWater;

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

        private CoflowExecutionBudget _budget = null!;
        private int _budgetFrames;
        private int _budgetIntegerRegisters;
        private int _budgetFloatRegisters;
        private int _budgetReferenceRegisters;

        internal CoflowExecutionContext? NextPooled { get; set; }

        internal int IntegerBase => _integerBase;

        internal int FloatBase => _floatBase;

        internal int ReferenceBase => _referenceBase;

        internal CoflowCollectionArena Collections => _collections;

        internal CoflowExecutionBudget Budget => _budget;

        internal CoflowProgram Program { get; private set; } = null!;

        internal int Pc { get; set; }

        internal IEnumerable<CoflowFunctionIdentity> CallStack => (from value in _frames.Take(_frameCount)
                                                                   select value.Program.Identity).Append(Program.Identity).Reverse();

        internal void Reset()
        {
            _frameCount = 0;
            _frameHighWater = 0;
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
            Pc = 0;
            Program = null!;
            _capturedCollections = Array.Empty<CoflowCollectionArena>();
            _budget = null!;
            _budgetFrames = 0;
            _budgetIntegerRegisters = 0;
            _budgetFloatRegisters = 0;
            _budgetReferenceRegisters = 0;
        }

        internal void Start<TArguments>(CoflowProgram program, TArguments arguments,
            uint? standaloneGeneration = null, CoflowClosure? closure = null) where TArguments : struct, ICoflowArguments
        {
            Program = program;
            _budget = standaloneGeneration.HasValue
                ? CoflowExecutionBudget.CreateUnbounded()
                : CoflowInvocationContext.Budget;
            _budget.EnterFrame();
            _budgetFrames = 1;
            uint generation = standaloneGeneration ?? CoflowInvocationContext.Generation;
            uint firstIndex = ((!standaloneGeneration.HasValue) ? checked((uint)CoflowInvocationContext.PublishedCollectionCount) : 0u);
            if (closure is not null)
            {
                _capturedCollections = closure.Collections;
                foreach (var arena in _capturedCollections)
                    firstIndex = Math.Max(firstIndex, arena.LastIndex);
            }
            // 正常执行由快照分配全局唯一索引；独立程序仍使用 Arena 内的连续索引。
            _collections.Reset(generation, firstIndex,
                standaloneGeneration.HasValue ? null : CoflowInvocationContext.AllocateCollectionIndex,
                standaloneGeneration.HasValue ? null : _budget);
            Reserve(program.RegisterProgram);
            arguments.Write(this);
        }

        internal CoflowValueRegister Parameter(int index)
        {
            return Offset(Program.RegisterProgram.Parameters[index]);
        }

        internal CoflowValueRegister OffsetRelative(CoflowValueRegister register)
        {
            return Offset(register);
        }

        private CoflowValueRegister Offset(CoflowValueRegister register)
        {
            return register with
            {
                IntegerBase = register.IntegerBase + _integerBase,
                FloatBase = register.FloatBase + _floatBase,
                ReferenceBase = register.ReferenceBase + _referenceBase
            };
        }

        private static CoflowValueRegister Absolute(CoflowValueRegister register, int integerBase, int floatBase, int referenceBase)
        {
            return register with
            {
                IntegerBase = register.IntegerBase + integerBase,
                FloatBase = register.FloatBase + floatBase,
                ReferenceBase = register.ReferenceBase + referenceBase
            };
        }

        internal long ReadInteger(CoflowRegister register)
        {
            return _integers[register.Index];
        }

        internal double ReadFloat(CoflowRegister register)
        {
            return _floats[register.Index];
        }

        internal object? ReadReference(CoflowRegister register)
        {
            return _references[register.Index];
        }

        internal void WriteInteger(CoflowRegister register, long value)
        {
            _integers[register.Index] = value;
        }

        internal void WriteFloat(CoflowRegister register, double value)
        {
            _floats[register.Index] = value;
        }

        internal void WriteReference(CoflowRegister register, object? value)
        {
            _references[register.Index] = value;
        }

        internal long ReadIntegerRelative(int index)
        {
            return _integers[_integerBase + index];
        }

        internal double ReadFloatRelative(int index)
        {
            return _floats[_floatBase + index];
        }

        internal object? ReadReferenceRelative(int index)
        {
            return _references[_referenceBase + index];
        }

        internal void WriteIntegerRelative(int index, long value)
        {
            _integers[_integerBase + index] = value;
        }

        internal void WriteBooleanRelative(int index, bool value)
        {
            _integers[_integerBase + index] = (value ? 1 : 0);
        }

        internal void WriteFloatRelative(int index, double value)
        {
            _floats[_floatBase + index] = value;
        }

        internal void WriteReferenceRelative(int index, object? value)
        {
            _references[_referenceBase + index] = value;
        }

        internal void Write<T>(CoflowValueRegister register, T value)
        {
            CoflowBoundaryCodec<T>.WriteImported(this, register, value);
        }

        internal void Copy(CoflowValueRegister source, CoflowValueRegister target)
        {
            RequireSamePhysicalLayout(source, target);
            CopyBank(_integers, source.IntegerBase, target.IntegerBase, source.Shape.IntegerCount);
            CopyBank(_floats, source.FloatBase, target.FloatBase, source.Shape.FloatCount);
            CopyBank(_references, source.ReferenceBase, target.ReferenceBase, source.Shape.ReferenceCount);
        }

        internal void CopyRelative(CoflowValueRegister source, CoflowValueRegister target)
        {
            RequireSamePhysicalLayout(source, target);
            CopyBank(_integers, _integerBase + source.IntegerBase, _integerBase + target.IntegerBase, source.Shape.IntegerCount);
            CopyBank(_floats, _floatBase + source.FloatBase, _floatBase + target.FloatBase, source.Shape.FloatCount);
            CopyBank(_references, _referenceBase + source.ReferenceBase, _referenceBase + target.ReferenceBase, source.Shape.ReferenceCount);
        }

        private static void RequireSamePhysicalLayout(CoflowValueRegister source, CoflowValueRegister target)
        {
            if (source.Shape.IntegerCount != target.Shape.IntegerCount || source.Shape.FloatCount != target.Shape.FloatCount || source.Shape.ReferenceCount != target.Shape.ReferenceCount)
            {
                throw new InvalidOperationException($"register value layout mismatch: `{source.Shape.Type}` to `{target.Shape.Type}`");
            }
        }

        private static void CopyBank<T>(T[] values, int source, int target, int count)
        {
            if (count != 0 && source != target)
            {
                if (count == 1)
                {
                    values[target] = values[source];
                }
                else
                {
                    Array.Copy(values, source, values, target, count);
                }
            }
        }

        internal void WriteEncodedRelative(CoflowEncodedValue source, CoflowValueRegister target)
        {
            if (source.Integers.Length == 1 && source.Floats.Length == 0 && source.References.Length == 0)
            {
                WriteIntegerRelative(target.IntegerBase, source.Integers[0]);
                return;
            }
            if (source.Floats.Length == 1 && source.Integers.Length == 0 && source.References.Length == 0)
            {
                WriteFloatRelative(target.FloatBase, source.Floats[0]);
                return;
            }
            if (source.References.Length == 1 && source.Integers.Length == 0 && source.Floats.Length == 0)
            {
                WriteReferenceRelative(target.ReferenceBase, source.References[0]);
                return;
            }
            if (source.Integers.Length != 0)
            {
                Array.Copy(source.Integers, 0, _integers, _integerBase + target.IntegerBase, source.Integers.Length);
            }
            if (source.Floats.Length != 0)
            {
                Array.Copy(source.Floats, 0, _floats, _floatBase + target.FloatBase, source.Floats.Length);
            }
            if (source.References.Length != 0)
            {
                Array.Copy(source.References, 0, _references, _referenceBase + target.ReferenceBase, source.References.Length);
            }
        }

        internal bool Call(CoflowRegisterCallSite site, CoflowProgram? target, bool tail)
        {
            PrepareCallArguments(site);
            if (target == null)
            {
                return false;
            }
            if (tail && target == Program)
            {
                for (int i = 0; i < site.Arguments.Length; i++)
                {
                    CopyRelative(site.Arguments[i], Program.RegisterProgram.Parameters[i]);
                }
                Pc = 0;
                return true;
            }
            EnterDirect(target, site, tail, Offset(site.Result));
            return true;
        }

        private void PrepareCallArguments(CoflowRegisterCallSite site)
        {
            for (int i = 0; i < site.Arguments.Length; i++)
            {
                if (site.CopyArguments[i])
                {
                    CopyRelative(site.SourceArguments[i], site.Arguments[i]);
                }
            }
        }

        private void EnterDirect(CoflowProgram target, CoflowRegisterCallSite site, bool tail, CoflowValueRegister returnTarget)
        {
            int integerBase = _integerBase;
            int floatBase = _floatBase;
            int referenceBase = _referenceBase;
            int integerTop = _integerTop;
            int floatTop = _floatTop;
            int referenceTop = _referenceTop;
            if (!tail)
            {
                PushFrame(returnTarget);
            }
            _integerBase = ((site.IntegerWindowBase < 0) ? integerTop : (integerBase + site.IntegerWindowBase));
            _floatBase = ((site.FloatWindowBase < 0) ? floatTop : (floatBase + site.FloatWindowBase));
            _referenceBase = ((site.ReferenceWindowBase < 0) ? referenceTop : (referenceBase + site.ReferenceWindowBase));
            Program = target;
            Pc = 0;
            Reserve(target.RegisterProgram);
            if (tail)
            {
                CompactTailWindow(target.RegisterProgram, integerBase, floatBase, referenceBase, referenceTop);
            }
        }

        private void EnterFromRegisters(CoflowProgram target, CoflowValueRegister[] arguments, bool tail, CoflowValueRegister returnTarget)
        {
            CoflowProgram program = Program;
            int integerBase = _integerBase;
            int floatBase = _floatBase;
            int referenceBase = _referenceBase;
            int integerTop = _integerTop;
            int floatTop = _floatTop;
            int referenceTop = _referenceTop;
            if (!tail)
            {
                PushFrame(returnTarget);
                _integerBase = _integerTop;
                _floatBase = _floatTop;
                _referenceBase = _referenceTop;
            }
            else
            {
                _integerBase = integerTop;
                _floatBase = floatTop;
                _referenceBase = referenceTop;
            }
            Program = target;
            Pc = 0;
            Reserve(target.RegisterProgram);
            for (int i = 0; i < arguments.Length; i++)
            {
                CoflowValueRegister source = Absolute(arguments[i], integerBase, floatBase, referenceBase);
                Copy(source, Parameter(i));
            }
            if (tail)
            {
                CompactTailWindow(target.RegisterProgram, integerBase, floatBase, referenceBase, referenceTop);
            }
        }

        internal bool CallIndirect<TResult>(CoflowRegisterIndirectCallSite site, bool tail, out TResult returned)
        {
            returned = default!;
            var functionId = CoflowFunctionId.FromPacked(unchecked((ulong)
                ReadIntegerRelative(site.Callable.IntegerBase)));
            var environmentId = CoflowValueId.FromPacked(unchecked((ulong)
                ReadIntegerRelative(site.Callable.IntegerBase + 1)));
            var callable = CoflowInvocationContext.Function(functionId, environmentId);
            if (callable.Closure is { } closure)
            {
                EnterClosureFromRegisters(closure, site.Arguments, tail, Offset(site.Result));
                return false;
            }
            var functionEntry = callable.Entry!;
            CoflowProgram? compiledProgram = functionEntry.CompiledProgram;
            if (compiledProgram != null)
            {
                EnterBoundFromRegisters(compiledProgram, callable.Receiver!, site.Arguments, tail, Offset(site.Result));
                return false;
            }
            Budget.HostCall(BoundaryLanes(site.Arguments, site.Result));
            functionEntry.InvokeBoundFromVm(new CoflowNativeFrame(this, site.Arguments, site.Result, site.ResultType));
            return tail && ReturnRegister<TResult>(site.Result, out returned);
        }

        internal T DecodeEncoded<T>(CoflowEncodedValue source)
        {
            CoflowValueShape shape = CoflowValueShape.Of(typeof(T));
            RequireSamePhysicalLayout(source.Shape, shape);
            int integerBase = Math.Max(_integerTop, _codecIntegerTop);
            int floatBase = Math.Max(_floatTop, _codecFloatTop);
            int referenceBase = Math.Max(_referenceTop, _codecReferenceTop);
            int codecIntegerTop;
            int codecFloatTop;
            int codecReferenceTop;
            checked
            {
                Ensure(ref _integers, integerBase + shape.IntegerCount);
                Ensure(ref _floats, floatBase + shape.FloatCount);
                Ensure(ref _references, referenceBase + shape.ReferenceCount);
                codecIntegerTop = _codecIntegerTop;
                codecFloatTop = _codecFloatTop;
                codecReferenceTop = _codecReferenceTop;
            }
            _codecIntegerTop = integerBase + shape.IntegerCount;
            _codecFloatTop = floatBase + shape.FloatCount;
            _codecReferenceTop = referenceBase + shape.ReferenceCount;
            try
            {
                Array.Copy(source.Integers, 0, _integers, integerBase, source.Integers.Length);
                Array.Copy(source.Floats, 0, _floats, floatBase, source.Floats.Length);
                Array.Copy(source.References, 0, _references, referenceBase, source.References.Length);
                return CoflowBoundaryCodec<T>.Read(this,
                    new CoflowValueRegister(shape, integerBase, floatBase, referenceBase));
            }
            finally
            {
                if (shape.ReferenceCount != 0)
                {
                    Array.Clear(_references, referenceBase, shape.ReferenceCount);
                }
                _codecIntegerTop = codecIntegerTop;
                _codecFloatTop = codecFloatTop;
                _codecReferenceTop = codecReferenceTop;
            }
        }

        internal CoflowCollectionKind CollectionKind(CoflowCollectionId id)
        {
            return ResolveCollectionArena(id).Kind(id);
        }

        internal int CollectionItemCount(CoflowCollectionId id)
        {
            return ResolveCollectionArena(id).ItemCount(id);
        }

        internal CoflowEncodedValue ReadArrayItem(CoflowCollectionId id, int index)
        {
            return ResolveCollectionArena(id).ReadArrayItem(id, index);
        }

        internal CoflowEncodedValue ReadDictionaryKey(CoflowCollectionId id, int index)
        {
            return ResolveCollectionArena(id).ReadDictionaryKey(id, index);
        }

        internal CoflowEncodedValue ReadDictionaryValue(CoflowCollectionId id, int index)
        {
            return ResolveCollectionArena(id).ReadDictionaryValue(id, index);
        }

        internal void MakeCollection(CoflowRegisterCollectionSite site, bool dictionary)
        {
            CoflowCollectionId collectionId;
            if (dictionary)
            {
                CoflowValueRegister[] values = site.Second ?? throw new InvalidOperationException("A dictionary site has no value registers.");
                Type[] genericArguments = site.Target.Shape.Type.GetGenericArguments();
                CoflowValueShape keyShape = ((site.First.Length == 0) ? CoflowValueShape.Of(genericArguments[0]) : site.First[0].Shape);
                CoflowValueShape valueShape = values.Length == 0 ? CoflowValueShape.Of(genericArguments[1]) : values[0].Shape;
                collectionId = _collections.AddDictionary(keyShape, valueShape, this, site.First, values);
            }
            else
            {
                CoflowValueShape elementShape = ((site.First.Length == 0) ? CoflowValueShape.Of(site.Target.Shape.Type.GetGenericArguments()[0]) : site.First[0].Shape);
                collectionId = _collections.AddArray(elementShape, this, site.First);
            }
            WriteIntegerRelative(site.Target.IntegerBase, (long)collectionId.Packed);
        }

        internal void ArrayIndex(CoflowRegisterArrayIndexSite site)
        {
            CoflowCollectionId id = CoflowCollectionId.FromPacked((ulong)ReadIntegerRelative(site.Collection.IntegerBase));
            long index = ReadIntegerRelative(site.Index.IntegerBase);
            if (index < 0 || index >= CollectionItemCount(id))
            {
                WriteIntegerRelative(site.Target.IntegerBase, 0L);
                return;
            }
            WriteIntegerRelative(site.Target.IntegerBase, 1L);
            CoflowValueRegister target = OffsetRelative(site.Target.First);
            checked
            {
                ResolveCollectionArena(id).CopyArrayItem(id, (int)index, this, target);
            }
        }

        internal void DictionaryIndex(CoflowRegisterArrayIndexSite site)
        {
            CoflowCollectionId id = CoflowCollectionId.FromPacked((ulong)ReadIntegerRelative(site.Collection.IntegerBase));
            int index = ResolveCollectionArena(id).FindDictionaryKey(id, this, site.Index);
            if (index < 0)
            {
                WriteIntegerRelative(site.Target.IntegerBase, 0L);
                return;
            }
            WriteIntegerRelative(site.Target.IntegerBase, 1L);
            CoflowValueRegister target = OffsetRelative(site.Target.First);
            ResolveCollectionArena(id).CopyDictionaryValue(id, index, this, target);
        }

        internal void ReadCollection(CoflowRegisterCollectionReadSite site, CoflowRegisterOpCode code)
        {
            CoflowCollectionId id = CoflowCollectionId.FromPacked((ulong)ReadIntegerRelative(site.Collection.IntegerBase));
            if (code == CoflowRegisterOpCode.CollectionCount)
            {
                WriteIntegerRelative(site.Target.IntegerBase, CollectionItemCount(id));
                return;
            }
            int index = checked((int)ReadIntegerRelative((site.Index ?? throw new InvalidOperationException("A collection item read has no index register.")).IntegerBase));
            CoflowValueRegister target = OffsetRelative(site.Target);
            var arena = ResolveCollectionArena(id);
            switch (code)
            {
                case CoflowRegisterOpCode.ArrayItem:
                    arena.CopyArrayItem(id, index, this, target);
                    break;
                case CoflowRegisterOpCode.DictionaryKey:
                    arena.CopyDictionaryKey(id, index, this, target);
                    break;
                default:
                    arena.CopyDictionaryValue(id, index, this, target);
                    break;
            }
        }

        internal void ProjectDictionary(CoflowRegisterCollectionProjectionSite site, bool values)
        {
            CoflowCollectionId sourceId = CoflowCollectionId.FromPacked((ulong)ReadIntegerRelative(site.Source.IntegerBase));
            CoflowCollectionId projectionId = _collections.AddDictionaryProjection(
                CollectionArena(sourceId), sourceId, values);
            WriteIntegerRelative(site.Target.IntegerBase, (long)projectionId.Packed);
        }

        internal void ExecuteCollectionBuiltin(CoflowRegisterCollectionBuiltinSite site)
        {
            CoflowBuiltinKind kind = site.Builtin.Kind;
            CoflowCollectionId collectionId = CollectionId(site.Receiver);
            CoflowCollectionArena arena = CollectionArena(collectionId);
            int count = CollectionItemCount(collectionId);
            Budget.CollectionWork(kind == CoflowBuiltinKind.CollectionUnique
                ? checked((long)count * Math.Max(0, count - 1) / 2)
                : count);
            bool second = kind == CoflowBuiltinKind.DictionaryContainsValue;
            if (kind - 4 <= CoflowBuiltinKind.DictionaryKeys)
            {
                bool value = false;
                for (int index = 0; index < count; index++)
                {
                    if (arena.ValueEqualsRegister(collectionId, index, second, this, site.Argument!.Value))
                    {
                        value = true;
                        break;
                    }
                }
                WriteBooleanRelative(site.Target.IntegerBase, value);
                return;
            }
            if (kind == CoflowBuiltinKind.CollectionUnique)
            {
                for (int left = 0; left < count; left++)
                {
                    for (int right = left + 1; right < count; right++)
                    {
                        if (arena.ValueEquals(collectionId, left, second: false,
                                arena, collectionId, right, otherSecond: false))
                        {
                            WriteBooleanRelative(site.Target.IntegerBase, value: false);
                            return;
                        }
                    }
                }
                WriteBooleanRelative(site.Target.IntegerBase, value: true);
                return;
            }
            if (kind is CoflowBuiltinKind.CollectionMin or CoflowBuiltinKind.CollectionMax)
            {
                if (count == 0)
                    throw new InvalidOperationException("aggregate requires a non-empty array");
                int selected = 0;
                for (int index = 1; index < count; index++)
                {
                    int comparison = arena.Compare(collectionId, index, selected);
                    if (kind == CoflowBuiltinKind.CollectionMin ? comparison < 0 : comparison > 0)
                        selected = index;
                }
                arena.CopyArrayItem(collectionId, selected, this, OffsetRelative(site.Target));
                return;
            }
            switch (kind)
            {
                case CoflowBuiltinKind.CollectionSumInteger:
                    {
                        long sum = 0L;
                        for (int index = 0; index < count; index++)
                            sum = checked(sum + arena.ReadInteger(collectionId, index));
                        WriteIntegerRelative(site.Target.IntegerBase, sum);
                        return;
                    }
                case CoflowBuiltinKind.CollectionSumFloat:
                    {
                        double sum = 0.0;
                        for (int index = 0; index < count; index++)
                            sum += arena.ReadFloat(collectionId, index);
                        WriteFloatRelative(site.Target.FloatBase, sum);
                        return;
                    }
                case CoflowBuiltinKind.CollectionSorted:
                case CoflowBuiltinKind.CollectionStrictlySorted:
                    bool strict = kind == CoflowBuiltinKind.CollectionStrictlySorted;
                    for (int index = 1; index < count; index++)
                    {
                        int comparison = arena.Compare(collectionId, index - 1, index);
                        if (strict ? comparison >= 0 : comparison > 0)
                        {
                            WriteBooleanRelative(site.Target.IntegerBase, value: false);
                            return;
                        }
                    }
                    WriteBooleanRelative(site.Target.IntegerBase, value: true);
                    return;
                default:
                    break;
            }
            CoflowCollectionId argumentId = CollectionId(site.Argument!.Value);
            var result = kind switch
            {
                CoflowBuiltinKind.CollectionIntersects => Overlaps(collectionId, argumentId),
                CoflowBuiltinKind.CollectionDisjoint => !Overlaps(collectionId, argumentId),
                CoflowBuiltinKind.CollectionSubset => IsSubset(collectionId, argumentId),
                CoflowBuiltinKind.CollectionSuperset => IsSubset(argumentId, collectionId),
                _ => throw new InvalidOperationException($"Unsupported collection builtin `{kind}`."),
            };
            WriteBooleanRelative(site.Target.IntegerBase, result);
        }

        private CoflowCollectionId CollectionId(CoflowValueRegister register)
        {
            return CoflowCollectionId.FromPacked((ulong)ReadIntegerRelative(register.IntegerBase));
        }

        private CoflowCollectionArena CollectionArena(CoflowCollectionId id)
        {
            return ResolveCollectionArena(id);
        }

        private CoflowCollectionArena ResolveCollectionArena(CoflowCollectionId id)
        {
            if (_collections.Contains(id)) return _collections;
            foreach (var arena in _capturedCollections)
                if (arena.Contains(id)) return arena;
            return CoflowInvocationContext.CollectionArena(id);
        }

        private bool Overlaps(CoflowCollectionId left, CoflowCollectionId right)
        {
            CoflowCollectionArena leftArena = CollectionArena(left);
            CoflowCollectionArena rightArena = CollectionArena(right);
            for (int leftIndex = 0; leftIndex < CollectionItemCount(left); leftIndex++)
            {
                for (int rightIndex = 0; rightIndex < CollectionItemCount(right); rightIndex++)
                {
                    if (leftArena.ValueEquals(left, leftIndex, second: false,
                            rightArena, right, rightIndex, otherSecond: false))
                    {
                        return true;
                    }
                }
            }
            return false;
        }

        private bool IsSubset(CoflowCollectionId left, CoflowCollectionId right)
        {
            CoflowCollectionArena leftArena = CollectionArena(left);
            CoflowCollectionArena rightArena = CollectionArena(right);
            for (int leftIndex = 0; leftIndex < CollectionItemCount(left); leftIndex++)
            {
                bool found = false;
                for (int rightIndex = 0; rightIndex < CollectionItemCount(right); rightIndex++)
                {
                    if (leftArena.ValueEquals(left, leftIndex, second: false,
                            rightArena, right, rightIndex, otherSecond: false))
                    {
                        found = true;
                        break;
                    }
                }
                if (!found)
                {
                    return false;
                }
            }
            return true;
        }

        internal void ArrayBuilder(CoflowRegisterArrayBuilderSite site, bool append)
        {
            if (!append)
            {
                int capacity = checked((int)ReadIntegerRelative(site.CollectionOrCapacity.IntegerBase));
                CoflowValueShape elementShape = CoflowValueShape.Of(site.Target.Shape.Type.GetGenericArguments()[0]);
                CoflowCollectionId collectionId = _collections.BeginArray(elementShape, capacity);
                WriteIntegerRelative(site.Target.IntegerBase, (long)collectionId.Packed);
                return;
            }
            CoflowValueRegister source = site.Item ?? throw new InvalidOperationException("An array append site has no item register.");
            CoflowCollectionId id = CoflowCollectionId.FromPacked((ulong)ReadIntegerRelative(site.CollectionOrCapacity.IntegerBase));
            if (!_collections.Contains(id))
            {
                throw new InvalidOperationException("Only an invocation array builder can be appended.");
            }
            _collections.AppendArray(id, this, source);
        }

        private static void RequireSamePhysicalLayout(CoflowValueShape actual, CoflowValueShape expected)
        {
            if (actual.IntegerCount != expected.IntegerCount || actual.FloatCount != expected.FloatCount || actual.ReferenceCount != expected.ReferenceCount)
            {
                throw new InvalidOperationException($"encoded value layout mismatch: `{actual.Type}` to `{expected.Type}`");
            }
        }

        internal void WriteBoxed(CoflowValueRegister target, object value)
        {
            if (target.Shape.Kind == CoflowValueShapeKind.Scalar && target.Shape.ScalarKind == CoflowRegisterKind.Reference)
            {
                WriteReference(new CoflowRegister(CoflowRegisterKind.Reference, target.ReferenceBase), value);
                return;
            }
            if (target.Shape.Kind == CoflowValueShapeKind.Struct &&
                CoflowStructCodecs.TryGet(target.Shape.Type, out CoflowStructDescriptor structDescriptor))
            {
                structDescriptor.WriteObject(this, target, value);
                return;
            }
            if (target.Shape.Kind == CoflowValueShapeKind.Record &&
                CoflowTypeCodecs.TryGet(target.Shape.Type, out CoflowTypeDescriptor typeDescriptor))
            {
                WriteInteger(new CoflowRegister(CoflowRegisterKind.Integer, target.IntegerBase),
                    (long)typeDescriptor.GetValueIdObject(value).Packed);
                return;
            }
            throw new InvalidOperationException($"Unsupported boxed receiver type `{target.Shape.Type}`.");
        }

        private void EnterBoundFromRegisters(CoflowProgram target, object receiver, CoflowValueRegister[] arguments, bool tail, CoflowValueRegister returnTarget)
        {
            int integerBase = _integerBase;
            int floatBase = _floatBase;
            int referenceBase = _referenceBase;
            int integerTop = _integerTop;
            int floatTop = _floatTop;
            int referenceTop = _referenceTop;
            if (!tail)
            {
                PushFrame(returnTarget);
                _integerBase = _integerTop;
                _floatBase = _floatTop;
                _referenceBase = _referenceTop;
            }
            else
            {
                _integerBase = integerTop;
                _floatBase = floatTop;
                _referenceBase = referenceTop;
            }
            Program = target;
            Pc = 0;
            Reserve(target.RegisterProgram);
            for (int index = 0; index < arguments.Length; index++)
            {
                CoflowValueRegister source = Absolute(arguments[index], integerBase, floatBase, referenceBase);
                Copy(source, Parameter(index));
            }
            WriteBoxed(Parameter(arguments.Length), receiver);
            if (tail)
            {
                CompactTailWindow(target.RegisterProgram, integerBase, floatBase, referenceBase, referenceTop);
            }
        }

        private void EnterClosureFromRegisters(CoflowClosure closure, CoflowValueRegister[] arguments, bool tail, CoflowValueRegister returnTarget)
        {
            if (closure.Collections.Count != 0)
                _capturedCollections = _capturedCollections.Concat(closure.Collections).Distinct().ToArray();
            int integerBase = _integerBase;
            int floatBase = _floatBase;
            int referenceBase = _referenceBase;
            int integerTop = _integerTop;
            int floatTop = _floatTop;
            int referenceTop = _referenceTop;
            if (!tail)
            {
                PushFrame(returnTarget);
                _integerBase = _integerTop;
                _floatBase = _floatTop;
                _referenceBase = _referenceTop;
            }
            else
            {
                _integerBase = integerTop;
                _floatBase = floatTop;
                _referenceBase = referenceTop;
            }
            Program = closure.Program;
            Pc = 0;
            Reserve(closure.Program.RegisterProgram);
            for (int index = 0; index < arguments.Length; index++)
            {
                CoflowValueRegister source = Absolute(arguments[index], integerBase, floatBase, referenceBase);
                Copy(source, Parameter(index));
            }
            WriteCaptures(closure, arguments.Length);
            if (tail)
            {
                CompactTailWindow(closure.Program.RegisterProgram, integerBase, floatBase, referenceBase, referenceTop);
            }
        }

        private void CompactTailWindow(CoflowRegisterProgram program, int integerBase, int floatBase, int referenceBase, int previousReferenceTop)
        {
            int sourceIntegerBase = _integerBase;
            int sourceFloatBase = _floatBase;
            int sourceReferenceBase = _referenceBase;
            if (program.ParameterIntegerCount != 0)
            {
                Array.Copy(_integers, sourceIntegerBase, _integers, integerBase, program.ParameterIntegerCount);
            }
            if (program.ParameterFloatCount != 0)
            {
                Array.Copy(_floats, sourceFloatBase, _floats, floatBase, program.ParameterFloatCount);
            }
            if (program.ParameterReferenceCount != 0)
            {
                Array.Copy(_references, sourceReferenceBase, _references, referenceBase, program.ParameterReferenceCount);
            }
            int retainedReferenceTop = referenceBase + program.ParameterReferenceCount;
            int clearReferenceTop = Math.Max(previousReferenceTop,
                checked(referenceBase + program.ReferenceRegisterCount));
            if (clearReferenceTop > retainedReferenceTop)
            {
                Array.Clear(_references, retainedReferenceTop, clearReferenceTop - retainedReferenceTop);
            }
            _integerBase = integerBase;
            _floatBase = floatBase;
            _referenceBase = referenceBase;
            checked
            {
                _integerTop = integerBase + program.IntegerRegisterCount;
                _floatTop = floatBase + program.FloatRegisterCount;
                _referenceTop = referenceBase + program.ReferenceRegisterCount;
            }
        }

        internal bool ReturnRegister<TResult>(CoflowValueRegister source, out TResult root)
        {
            if (_frameCount == 0)
            {
                root = CoflowInvocationContext.PromoteResult(
                    CoflowBoundaryCodec<TResult>.ReadRelative(this, source), _collections);
                return true;
            }
            CoflowFrame frame = _frames[_frameCount - 1];
            Copy(Offset(source), frame.ReturnTarget);
            ClearCurrentReferences();
            _frameCount--;
            Budget.ExitFrame();
            _budgetFrames--;
            Program = frame.Program;
            Pc = frame.ReturnPc;
            _integerBase = frame.IntegerBase;
            _floatBase = frame.FloatBase;
            _referenceBase = frame.ReferenceBase;
            checked
            {
                _integerTop = _integerBase + Program.RegisterProgram.IntegerRegisterCount;
                _floatTop = _floatBase + Program.RegisterProgram.FloatRegisterCount;
                _referenceTop = _referenceBase + Program.RegisterProgram.ReferenceRegisterCount;
                root = default!;
                return false;
            }
        }

        internal void MakeClosure(CoflowRegisterClosureSite site)
        {
            CoflowClosureTemplate template = site.Template;
            Budget.ClosureLanes(checked(template.IntegerCount + template.FloatCount + template.ReferenceCount));
            CoflowClosure coflowClosure = CoflowClosure.Create(template.Program, template.Captures,
                FreezeClosureCollections(), template.IntegerCount, template.FloatCount, template.ReferenceCount);
            for (int index = 0; index < template.CaptureCount; index++)
            {
                Capture(Offset(site.Captures[index]), template.Captures[index], coflowClosure);
            }
            coflowClosure.RetainReachableCollections();
            var environmentId = CoflowInvocationContext.AttachClosure(coflowClosure);
            WriteIntegerRelative(site.Target.IntegerBase,
                new CoflowFunctionId(CoflowInvocationContext.SnapshotId,
                    CoflowFunctionKind.Closure, template.TargetIndex).Packed);
            WriteIntegerRelative(site.Target.IntegerBase + 1, unchecked((long)environmentId.Packed));
        }

        private CoflowCollectionArena[] FreezeClosureCollections()
        {
            if (_collections.Count == 0) return _capturedCollections.ToArray();
            // closure 持有独立快照，池化执行上下文仍可在调用结束时清空自己的 Arena。
            return _capturedCollections.Append(_collections.Freeze()).ToArray();
        }

        private void Capture(CoflowValueRegister source, CoflowCaptureLayout target, CoflowClosure closure)
        {
            if (source.Shape.Kind == CoflowValueShapeKind.Unit)
            {
                return;
            }
            if (source.Shape.Kind is CoflowValueShapeKind.Scalar or
                CoflowValueShapeKind.Collection or CoflowValueShapeKind.Record)
            {
                switch (source.Shape.ScalarKind)
                {
                    case CoflowRegisterKind.Integer:
                        closure.SetInteger(target.IntegerBase, ReadInteger(source.Scalar));
                        break;
                    case CoflowRegisterKind.Float:
                        closure.SetFloat(target.FloatBase, ReadFloat(source.Scalar));
                        break;
                    default:
                        closure.SetReference(target.ReferenceBase, ReadReference(source.Scalar));
                        break;
                }
            }
            else if (source.Shape.Kind is CoflowValueShapeKind.Struct or CoflowValueShapeKind.Function)
            {
                for (int lane = 0; lane < source.Shape.IntegerCount; lane++)
                {
                    closure.SetInteger(target.IntegerBase + lane,
                        ReadInteger(new CoflowRegister(CoflowRegisterKind.Integer, source.IntegerBase + lane)));
                }
                for (int lane = 0; lane < source.Shape.FloatCount; lane++)
                {
                    closure.SetFloat(target.FloatBase + lane,
                        ReadFloat(new CoflowRegister(CoflowRegisterKind.Float, source.FloatBase + lane)));
                }
                for (int lane = 0; lane < source.Shape.ReferenceCount; lane++)
                {
                    closure.SetReference(target.ReferenceBase + lane,
                        ReadReference(new CoflowRegister(CoflowRegisterKind.Reference, source.ReferenceBase + lane)));
                }
            }
            else
            {
                closure.SetInteger(target.IntegerBase, ReadInteger(source.Tag));
                Capture(source.First, new CoflowCaptureLayout(source.Shape.First!, target.IntegerBase + 1, target.FloatBase, target.ReferenceBase), closure);
                if (source.Shape.Kind == CoflowValueShapeKind.Result)
                {
                    Capture(source.Second, new CoflowCaptureLayout(source.Shape.Second!, target.IntegerBase + 1 + source.Shape.First!.IntegerCount, target.FloatBase + source.Shape.First.FloatCount, target.ReferenceBase + source.Shape.First.ReferenceCount), closure);
                }
            }
        }

        private void Restore(CoflowCaptureLayout source, CoflowValueRegister target, CoflowClosure closure)
        {
            if (target.Shape.Type != source.Shape.Type)
            {
                throw new InvalidOperationException("closure capture type mismatch");
            }
            if (target.Shape.Kind == CoflowValueShapeKind.Unit)
            {
                return;
            }
            if (target.Shape.Kind is CoflowValueShapeKind.Scalar or
                CoflowValueShapeKind.Collection or CoflowValueShapeKind.Record)
            {
                switch (target.Shape.ScalarKind)
                {
                    case CoflowRegisterKind.Integer:
                        WriteInteger(target.Scalar, closure.Integer(source.IntegerBase));
                        break;
                    case CoflowRegisterKind.Float:
                        WriteFloat(target.Scalar, closure.Float(source.FloatBase));
                        break;
                    default:
                        WriteReference(target.Scalar, closure.Reference(source.ReferenceBase));
                        break;
                }
            }
            else if (target.Shape.Kind is CoflowValueShapeKind.Struct or CoflowValueShapeKind.Function)
            {
                for (int lane = 0; lane < target.Shape.IntegerCount; lane++)
                {
                    WriteInteger(new CoflowRegister(CoflowRegisterKind.Integer, target.IntegerBase + lane),
                        closure.Integer(source.IntegerBase + lane));
                }
                for (int lane = 0; lane < target.Shape.FloatCount; lane++)
                {
                    WriteFloat(new CoflowRegister(CoflowRegisterKind.Float, target.FloatBase + lane),
                        closure.Float(source.FloatBase + lane));
                }
                for (int lane = 0; lane < target.Shape.ReferenceCount; lane++)
                {
                    WriteReference(new CoflowRegister(CoflowRegisterKind.Reference, target.ReferenceBase + lane),
                        closure.Reference(source.ReferenceBase + lane));
                }
            }
            else
            {
                WriteInteger(target.Tag, closure.Integer(source.IntegerBase));
                Restore(new CoflowCaptureLayout(target.Shape.First!, source.IntegerBase + 1, source.FloatBase, source.ReferenceBase), target.First, closure);
                if (target.Shape.Kind == CoflowValueShapeKind.Result)
                {
                    Restore(new CoflowCaptureLayout(target.Shape.Second!, source.IntegerBase + 1 + target.Shape.First!.IntegerCount, source.FloatBase + target.Shape.First.FloatCount, source.ReferenceBase + target.Shape.First.ReferenceCount), target.Second, closure);
                }
            }
        }

        internal void WriteCaptures(CoflowClosure closure, int parameterOffset)
        {
            for (int i = 0; i < closure.Captures.Count; i++)
            {
                Restore(closure.Captures[i], Parameter(parameterOffset + i), closure);
            }
        }

        private void PushFrame(CoflowValueRegister returnTarget)
        {
            Budget.EnterFrame();
            _budgetFrames++;
            EnsureFrames(_frameCount + 1);
            _frames[_frameCount++] = new CoflowFrame
            {
                Program = Program,
                ReturnPc = Pc,
                IntegerBase = _integerBase,
                FloatBase = _floatBase,
                ReferenceBase = _referenceBase,
                ReturnTarget = returnTarget
            };
            _frameHighWater = Math.Max(_frameHighWater, _frameCount);
        }

        private void Reserve(CoflowRegisterProgram program)
        {
            checked
            {
                _integerTop = _integerBase + program.IntegerRegisterCount;
                _floatTop = _floatBase + program.FloatRegisterCount;
                _referenceTop = _referenceBase + program.ReferenceRegisterCount;
                var integerHighWater = Math.Max(_budgetIntegerRegisters, _integerTop);
                var floatHighWater = Math.Max(_budgetFloatRegisters, _floatTop);
                var referenceHighWater = Math.Max(_budgetReferenceRegisters, _referenceTop);
                Budget.AcquireRegisters(
                    integerHighWater - _budgetIntegerRegisters,
                    floatHighWater - _budgetFloatRegisters,
                    referenceHighWater - _budgetReferenceRegisters);
                _budgetIntegerRegisters = integerHighWater;
                _budgetFloatRegisters = floatHighWater;
                _budgetReferenceRegisters = referenceHighWater;
                Ensure(ref _integers, _integerTop);
                Ensure(ref _floats, _floatTop);
                Ensure(ref _references, _referenceTop);
                _referenceHighWater = Math.Max(_referenceHighWater, _referenceTop);
            }
        }

        private void ClearCurrentReferences()
        {
            if (_referenceTop > _referenceBase)
            {
                Array.Clear(_references, _referenceBase, _referenceTop - _referenceBase);
            }
        }

        private static void Ensure<T>(ref T[] values, int count)
        {
            if (count > values.Length)
            {
                T[] expanded = ArrayPool<T>.Shared.Rent(Math.Max(count, checked(values.Length * 2)));
                bool clearReferences = RuntimeHelpers.IsReferenceOrContainsReferences<T>();
                if (clearReferences)
                {
                    Array.Clear(expanded, 0, expanded.Length);
                }
                Array.Copy(values, expanded, values.Length);
                if (clearReferences)
                {
                    Array.Clear(values, 0, values.Length);
                }
                ArrayPool<T>.Shared.Return(values);
                values = expanded;
            }
        }

        private static T[] RentCleared<T>(int count)
        {
            T[] values = ArrayPool<T>.Shared.Rent(count);
            Array.Clear(values, 0, values.Length);
            return values;
        }

        private void EnsureFrames(int count)
        {
            Ensure(ref _frames, count);
        }

        public void Dispose()
        {
            if (_budget is not null)
            {
                _budget.ReleaseRegisters(
                    _budgetIntegerRegisters, _budgetFloatRegisters, _budgetReferenceRegisters);
                while (_budgetFrames > 0)
                {
                    _budget.ExitFrame();
                    _budgetFrames--;
                }
            }
            _budgetIntegerRegisters = 0;
            _budgetFloatRegisters = 0;
            _budgetReferenceRegisters = 0;
            if (_referenceHighWater != 0)
            {
                Array.Clear(_references, 0, _referenceHighWater);
            }
            if (_frameHighWater != 0)
            {
                Array.Clear(_frames, 0, _frameHighWater);
            }
            _collections.Clear();
            Program = null!;
            NextPooled = _pooledContexts;
            _pooledContexts = this;
        }
    }

    [ThreadStatic]
    private static CoflowExecutionContext? _pooledContexts;

    internal static TResult Execute<TResult>(CoflowProgram program)
    {
        return ExecuteCore<Arguments0, TResult>(program, default(Arguments0));
    }

    internal static TResult Execute<T1, TResult>(CoflowProgram program, T1 arg1)
    {
        return ExecuteCore<Arguments1<T1>, TResult>(program, new Arguments1<T1>(arg1));
    }

    internal static TResult ExecuteRaw<T1, TResult>(CoflowProgram program, T1 arg1)
    {
        return ExecuteCore<RawArguments1<T1>, TResult>(program, new RawArguments1<T1>(arg1), uint.MaxValue);
    }

    internal static TResult Execute<T1, T2, TResult>(CoflowProgram program, T1 arg1, T2 arg2)
    {
        return ExecuteCore<Arguments2<T1, T2>, TResult>(program, new Arguments2<T1, T2>(arg1, arg2));
    }

    internal static TResult Execute<T1, T2, T3, TResult>(CoflowProgram program, T1 arg1, T2 arg2, T3 arg3)
    {
        return ExecuteCore<Arguments3<T1, T2, T3>, TResult>(program, new Arguments3<T1, T2, T3>(arg1, arg2, arg3));
    }

    internal static TResult Execute<T1, T2, T3, T4, TResult>(CoflowProgram program, T1 arg1, T2 arg2, T3 arg3, T4 arg4)
    {
        return ExecuteCore<Arguments4<T1, T2, T3, T4>, TResult>(program, new Arguments4<T1, T2, T3, T4>(arg1, arg2, arg3, arg4));
    }

    internal static TResult Execute<T1, T2, T3, T4, T5, TResult>(CoflowProgram program, T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5)
    {
        return ExecuteCore<Arguments5<T1, T2, T3, T4, T5>, TResult>(program, new Arguments5<T1, T2, T3, T4, T5>(arg1, arg2, arg3, arg4, arg5));
    }

    internal static TResult Execute<T1, T2, T3, T4, T5, T6, TResult>(CoflowProgram program, T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6)
    {
        return ExecuteCore<Arguments6<T1, T2, T3, T4, T5, T6>, TResult>(program, new Arguments6<T1, T2, T3, T4, T5, T6>(arg1, arg2, arg3, arg4, arg5, arg6));
    }

    internal static TResult Execute<T1, T2, T3, T4, T5, T6, T7, TResult>(CoflowProgram program, T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6, T7 arg7)
    {
        return ExecuteCore<Arguments7<T1, T2, T3, T4, T5, T6, T7>, TResult>(program, new Arguments7<T1, T2, T3, T4, T5, T6, T7>(arg1, arg2, arg3, arg4, arg5, arg6, arg7));
    }

    internal static TResult Execute<T1, T2, T3, T4, T5, T6, T7, T8, TResult>(CoflowProgram program, T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6, T7 arg7, T8 arg8)
    {
        return ExecuteCore<Arguments8<T1, T2, T3, T4, T5, T6, T7, T8>, TResult>(program, new Arguments8<T1, T2, T3, T4, T5, T6, T7, T8>(arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8));
    }

    internal static TResult ExecuteReceiver<TReceiver, TResult>(CoflowProgram program, TReceiver receiver)
    {
        return ExecuteCore<ReceiverArguments<TReceiver, Arguments0>, TResult>(program, new ReceiverArguments<TReceiver, Arguments0>(receiver, default(Arguments0)));
    }

    internal static TResult ExecuteReceiver<TReceiver, T1, TResult>(CoflowProgram program, TReceiver receiver, T1 arg1)
    {
        return ExecuteCore<ReceiverArguments<TReceiver, Arguments1<T1>>, TResult>(program, new ReceiverArguments<TReceiver, Arguments1<T1>>(receiver, new Arguments1<T1>(arg1)));
    }

    internal static TResult ExecuteReceiver<TReceiver, T1, T2, TResult>(CoflowProgram program, TReceiver receiver, T1 arg1, T2 arg2)
    {
        return ExecuteCore<ReceiverArguments<TReceiver, Arguments2<T1, T2>>, TResult>(program, new ReceiverArguments<TReceiver, Arguments2<T1, T2>>(receiver, new Arguments2<T1, T2>(arg1, arg2)));
    }

    internal static TResult ExecuteReceiver<TReceiver, T1, T2, T3, TResult>(CoflowProgram program, TReceiver receiver, T1 arg1, T2 arg2, T3 arg3)
    {
        return ExecuteCore<ReceiverArguments<TReceiver, Arguments3<T1, T2, T3>>, TResult>(program, new ReceiverArguments<TReceiver, Arguments3<T1, T2, T3>>(receiver, new Arguments3<T1, T2, T3>(arg1, arg2, arg3)));
    }

    internal static TResult ExecuteReceiver<TReceiver, T1, T2, T3, T4, TResult>(CoflowProgram program, TReceiver receiver, T1 arg1, T2 arg2, T3 arg3, T4 arg4)
    {
        return ExecuteCore<ReceiverArguments<TReceiver, Arguments4<T1, T2, T3, T4>>, TResult>(program, new ReceiverArguments<TReceiver, Arguments4<T1, T2, T3, T4>>(receiver, new Arguments4<T1, T2, T3, T4>(arg1, arg2, arg3, arg4)));
    }

    internal static TResult ExecuteReceiver<TReceiver, T1, T2, T3, T4, T5, TResult>(CoflowProgram program, TReceiver receiver, T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5)
    {
        return ExecuteCore<ReceiverArguments<TReceiver, Arguments5<T1, T2, T3, T4, T5>>, TResult>(program, new ReceiverArguments<TReceiver, Arguments5<T1, T2, T3, T4, T5>>(receiver, new Arguments5<T1, T2, T3, T4, T5>(arg1, arg2, arg3, arg4, arg5)));
    }

    internal static TResult ExecuteReceiver<TReceiver, T1, T2, T3, T4, T5, T6, TResult>(CoflowProgram program, TReceiver receiver, T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6)
    {
        return ExecuteCore<ReceiverArguments<TReceiver, Arguments6<T1, T2, T3, T4, T5, T6>>, TResult>(program, new ReceiverArguments<TReceiver, Arguments6<T1, T2, T3, T4, T5, T6>>(receiver, new Arguments6<T1, T2, T3, T4, T5, T6>(arg1, arg2, arg3, arg4, arg5, arg6)));
    }

    internal static TResult ExecuteReceiver<TReceiver, T1, T2, T3, T4, T5, T6, T7, TResult>(CoflowProgram program, TReceiver receiver, T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6, T7 arg7)
    {
        return ExecuteCore<ReceiverArguments<TReceiver, Arguments7<T1, T2, T3, T4, T5, T6, T7>>, TResult>(program, new ReceiverArguments<TReceiver, Arguments7<T1, T2, T3, T4, T5, T6, T7>>(receiver, new Arguments7<T1, T2, T3, T4, T5, T6, T7>(arg1, arg2, arg3, arg4, arg5, arg6, arg7)));
    }

    internal static TResult ExecuteReceiver<TReceiver, T1, T2, T3, T4, T5, T6, T7, T8, TResult>(CoflowProgram program, TReceiver receiver, T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6, T7 arg7, T8 arg8)
    {
        return ExecuteCore<ReceiverArguments<TReceiver, Arguments8<T1, T2, T3, T4, T5, T6, T7, T8>>, TResult>(program, new ReceiverArguments<TReceiver, Arguments8<T1, T2, T3, T4, T5, T6, T7, T8>>(receiver, new Arguments8<T1, T2, T3, T4, T5, T6, T7, T8>(arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8)));
    }

    internal static TResult ExecuteBound<TResult>(CoflowProgram program, object receiver)
    {
        return ExecuteCore<BoxedReceiverArguments<Arguments0>, TResult>(program, new BoxedReceiverArguments<Arguments0>(receiver, default(Arguments0)));
    }

    internal static TResult ExecuteBound<T1, TResult>(CoflowProgram program, object receiver, T1 arg1)
    {
        return ExecuteCore<BoxedReceiverArguments<Arguments1<T1>>, TResult>(program, new BoxedReceiverArguments<Arguments1<T1>>(receiver, new Arguments1<T1>(arg1)));
    }

    internal static TResult ExecuteBound<T1, T2, TResult>(CoflowProgram program, object receiver, T1 arg1, T2 arg2)
    {
        return ExecuteCore<BoxedReceiverArguments<Arguments2<T1, T2>>, TResult>(program, new BoxedReceiverArguments<Arguments2<T1, T2>>(receiver, new Arguments2<T1, T2>(arg1, arg2)));
    }

    internal static TResult ExecuteBound<T1, T2, T3, TResult>(CoflowProgram program, object receiver, T1 arg1, T2 arg2, T3 arg3)
    {
        return ExecuteCore<BoxedReceiverArguments<Arguments3<T1, T2, T3>>, TResult>(program, new BoxedReceiverArguments<Arguments3<T1, T2, T3>>(receiver, new Arguments3<T1, T2, T3>(arg1, arg2, arg3)));
    }

    internal static TResult ExecuteBound<T1, T2, T3, T4, TResult>(CoflowProgram program, object receiver, T1 arg1, T2 arg2, T3 arg3, T4 arg4)
    {
        return ExecuteCore<BoxedReceiverArguments<Arguments4<T1, T2, T3, T4>>, TResult>(program, new BoxedReceiverArguments<Arguments4<T1, T2, T3, T4>>(receiver, new Arguments4<T1, T2, T3, T4>(arg1, arg2, arg3, arg4)));
    }

    internal static TResult ExecuteBound<T1, T2, T3, T4, T5, TResult>(CoflowProgram program, object receiver, T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5)
    {
        return ExecuteCore<BoxedReceiverArguments<Arguments5<T1, T2, T3, T4, T5>>, TResult>(program, new BoxedReceiverArguments<Arguments5<T1, T2, T3, T4, T5>>(receiver, new Arguments5<T1, T2, T3, T4, T5>(arg1, arg2, arg3, arg4, arg5)));
    }

    internal static TResult ExecuteBound<T1, T2, T3, T4, T5, T6, TResult>(CoflowProgram program, object receiver, T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6)
    {
        return ExecuteCore<BoxedReceiverArguments<Arguments6<T1, T2, T3, T4, T5, T6>>, TResult>(program, new BoxedReceiverArguments<Arguments6<T1, T2, T3, T4, T5, T6>>(receiver, new Arguments6<T1, T2, T3, T4, T5, T6>(arg1, arg2, arg3, arg4, arg5, arg6)));
    }

    internal static TResult ExecuteBound<T1, T2, T3, T4, T5, T6, T7, TResult>(CoflowProgram program, object receiver, T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6, T7 arg7)
    {
        return ExecuteCore<BoxedReceiverArguments<Arguments7<T1, T2, T3, T4, T5, T6, T7>>, TResult>(program, new BoxedReceiverArguments<Arguments7<T1, T2, T3, T4, T5, T6, T7>>(receiver, new Arguments7<T1, T2, T3, T4, T5, T6, T7>(arg1, arg2, arg3, arg4, arg5, arg6, arg7)));
    }

    internal static TResult ExecuteBound<T1, T2, T3, T4, T5, T6, T7, T8, TResult>(CoflowProgram program, object receiver, T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6, T7 arg7, T8 arg8)
    {
        return ExecuteCore<BoxedReceiverArguments<Arguments8<T1, T2, T3, T4, T5, T6, T7, T8>>, TResult>(program, new BoxedReceiverArguments<Arguments8<T1, T2, T3, T4, T5, T6, T7, T8>>(receiver, new Arguments8<T1, T2, T3, T4, T5, T6, T7, T8>(arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8)));
    }

    internal static TResult ExecuteClosure<TResult>(CoflowClosure closure)
    {
        return ExecuteClosureCore<ClosureArguments<Arguments0>, TResult>(closure, new ClosureArguments<Arguments0>(closure, default(Arguments0)));
    }

    internal static TResult ExecuteClosure<T1, TResult>(CoflowClosure closure, T1 arg1)
    {
        return ExecuteClosureCore<ClosureArguments<Arguments1<T1>>, TResult>(closure, new ClosureArguments<Arguments1<T1>>(closure, new Arguments1<T1>(arg1)));
    }

    internal static TResult ExecuteClosure<T1, T2, TResult>(CoflowClosure closure, T1 arg1, T2 arg2)
    {
        return ExecuteClosureCore<ClosureArguments<Arguments2<T1, T2>>, TResult>(closure, new ClosureArguments<Arguments2<T1, T2>>(closure, new Arguments2<T1, T2>(arg1, arg2)));
    }

    internal static TResult ExecuteClosure<T1, T2, T3, TResult>(CoflowClosure closure, T1 arg1, T2 arg2, T3 arg3)
    {
        return ExecuteClosureCore<ClosureArguments<Arguments3<T1, T2, T3>>, TResult>(closure, new ClosureArguments<Arguments3<T1, T2, T3>>(closure, new Arguments3<T1, T2, T3>(arg1, arg2, arg3)));
    }

    internal static TResult ExecuteClosure<T1, T2, T3, T4, TResult>(CoflowClosure closure, T1 arg1, T2 arg2, T3 arg3, T4 arg4)
    {
        return ExecuteClosureCore<ClosureArguments<Arguments4<T1, T2, T3, T4>>, TResult>(closure, new ClosureArguments<Arguments4<T1, T2, T3, T4>>(closure, new Arguments4<T1, T2, T3, T4>(arg1, arg2, arg3, arg4)));
    }

    internal static TResult ExecuteClosure<T1, T2, T3, T4, T5, TResult>(CoflowClosure closure, T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5)
    {
        return ExecuteClosureCore<ClosureArguments<Arguments5<T1, T2, T3, T4, T5>>, TResult>(closure, new ClosureArguments<Arguments5<T1, T2, T3, T4, T5>>(closure, new Arguments5<T1, T2, T3, T4, T5>(arg1, arg2, arg3, arg4, arg5)));
    }

    internal static TResult ExecuteClosure<T1, T2, T3, T4, T5, T6, TResult>(CoflowClosure closure, T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6)
    {
        return ExecuteClosureCore<ClosureArguments<Arguments6<T1, T2, T3, T4, T5, T6>>, TResult>(closure, new ClosureArguments<Arguments6<T1, T2, T3, T4, T5, T6>>(closure, new Arguments6<T1, T2, T3, T4, T5, T6>(arg1, arg2, arg3, arg4, arg5, arg6)));
    }

    internal static TResult ExecuteClosure<T1, T2, T3, T4, T5, T6, T7, TResult>(CoflowClosure closure, T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6, T7 arg7)
    {
        return ExecuteClosureCore<ClosureArguments<Arguments7<T1, T2, T3, T4, T5, T6, T7>>, TResult>(closure, new ClosureArguments<Arguments7<T1, T2, T3, T4, T5, T6, T7>>(closure, new Arguments7<T1, T2, T3, T4, T5, T6, T7>(arg1, arg2, arg3, arg4, arg5, arg6, arg7)));
    }

    internal static TResult ExecuteClosure<T1, T2, T3, T4, T5, T6, T7, T8, TResult>(CoflowClosure closure, T1 arg1, T2 arg2, T3 arg3, T4 arg4, T5 arg5, T6 arg6, T7 arg7, T8 arg8)
    {
        return ExecuteClosureCore<ClosureArguments<Arguments8<T1, T2, T3, T4, T5, T6, T7, T8>>, TResult>(closure, new ClosureArguments<Arguments8<T1, T2, T3, T4, T5, T6, T7, T8>>(closure, new Arguments8<T1, T2, T3, T4, T5, T6, T7, T8>(arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8)));
    }

    private static TResult ExecuteClosureCore<TArguments, TResult>(CoflowClosure closure, TArguments arguments) where TArguments : struct, ICoflowArguments
    {
        using (closure.Owner.EnterExecution())
        {
            return ExecuteCore<TArguments, TResult>(closure.Program, arguments, closure: closure);
        }
    }

    private static TResult ExecuteCore<TArguments, TResult>(CoflowProgram program, TArguments arguments,
        uint? standaloneGeneration = null, CoflowClosure? closure = null) where TArguments : struct, ICoflowArguments
    {
        if (arguments.Count != program.ParameterCount)
        {
            throw Fault(program, $"function expected {program.ParameterCount} arguments but received {arguments.Count}");
        }
        CoflowExecutionContext coflowExecutionContext = RentContext();
        CoflowProgram currentProgram = program;
        int faultPc = 0;
        try
        {
            coflowExecutionContext.Start(program, arguments, standaloneGeneration, closure);
            CoflowRegisterProgram registerProgram = coflowExecutionContext.Program.RegisterProgram;
            CoflowRegisterInstruction[] instructions = registerProgram.Instructions;
            int pc = coflowExecutionContext.Pc;
            while ((uint)pc < (uint)instructions.Length)
            {
                coflowExecutionContext.Budget.Instruction();
                currentProgram = coflowExecutionContext.Program;
                faultPc = pc;
                CoflowRegisterInstruction instruction = instructions[pc++];
                checked
                {
                    switch (instruction.Code)
                    {
                        case CoflowRegisterOpCode.ConstantInteger:
                            coflowExecutionContext.WriteIntegerRelative(instruction.A, registerProgram.Immediates[instruction.B]);
                            break;
                        case CoflowRegisterOpCode.ConstantFloat:
                            coflowExecutionContext.WriteFloatRelative(instruction.A, BitConverter.Int64BitsToDouble(registerProgram.Immediates[instruction.B]));
                            break;
                        case CoflowRegisterOpCode.ConstantReference:
                            coflowExecutionContext.WriteReferenceRelative(instruction.A, registerProgram.Operations.References[instruction.C]);
                            break;
                        case CoflowRegisterOpCode.ConstantValue:
                            {
                                CoflowRegisterConstantSite coflowRegisterConstantSite = registerProgram.Operations.Constants[instruction.C];
                                coflowExecutionContext.WriteEncodedRelative(coflowRegisterConstantSite.Value, coflowRegisterConstantSite.Target);
                                break;
                            }
                        case CoflowRegisterOpCode.MoveInteger:
                            coflowExecutionContext.WriteIntegerRelative(instruction.A, coflowExecutionContext.ReadIntegerRelative(instruction.B));
                            break;
                        case CoflowRegisterOpCode.MoveFloat:
                            coflowExecutionContext.WriteFloatRelative(instruction.A, coflowExecutionContext.ReadFloatRelative(instruction.B));
                            break;
                        case CoflowRegisterOpCode.MoveReference:
                            coflowExecutionContext.WriteReferenceRelative(instruction.A, coflowExecutionContext.ReadReferenceRelative(instruction.B));
                            break;
                        case CoflowRegisterOpCode.MoveValue:
                            {
                                var transfer = registerProgram.Operations.Transfers[instruction.C];
                                coflowExecutionContext.CopyRelative(transfer.Source, transfer.Target);
                                break;
                            }
                        case CoflowRegisterOpCode.LoadHostFieldInteger:
                        case CoflowRegisterOpCode.LoadHostFieldFloat:
                        case CoflowRegisterOpCode.LoadHostFieldReference:
                            {
                                CoflowFieldAccess coflowFieldAccess = registerProgram.Operations.Fields[instruction.C];
                                if (!coflowFieldAccess.IsHost)
                                {
                                    throw new InvalidOperationException("A non-Host field cannot use a CLR reader instruction.");
                                }
                                object arg = coflowExecutionContext.ReadReferenceRelative(instruction.B) ?? throw new InvalidOperationException("field `" + coflowFieldAccess.Name + "` receiver is null");
                                if (instruction.Code == CoflowRegisterOpCode.LoadHostFieldInteger)
                                {
                                    coflowExecutionContext.WriteIntegerRelative(instruction.A, coflowFieldAccess.ReadInteger!(arg));
                                }
                                else if (instruction.Code == CoflowRegisterOpCode.LoadHostFieldFloat)
                                {
                                    coflowExecutionContext.WriteFloatRelative(instruction.A, coflowFieldAccess.ReadFloat!(arg));
                                }
                                else
                                {
                                    coflowExecutionContext.WriteReferenceRelative(instruction.A, coflowFieldAccess.ReadReference!(arg));
                                }
                                break;
                            }
                        case CoflowRegisterOpCode.LoadArenaFieldInteger:
                        case CoflowRegisterOpCode.LoadArenaFieldFloat:
                        case CoflowRegisterOpCode.LoadArenaFieldReference:
                            {
                                var field = registerProgram.Operations.Fields[instruction.C];
                                if (field.IsHost)
                                {
                                    throw new InvalidOperationException("A Host field cannot use a record Arena instruction.");
                                }
                                var valueId = CoflowValueId.FromPacked(unchecked((ulong)coflowExecutionContext.ReadIntegerRelative(instruction.B)));
                                if (instruction.Code == CoflowRegisterOpCode.LoadArenaFieldInteger)
                                {
                                    coflowExecutionContext.WriteIntegerRelative(instruction.A, CoflowInvocationContext.ReadArenaInteger(valueId, field.IntegerOffset));
                                }
                                else if (instruction.Code == CoflowRegisterOpCode.LoadArenaFieldFloat)
                                {
                                    coflowExecutionContext.WriteFloatRelative(instruction.A, CoflowInvocationContext.ReadArenaFloat(valueId, field.FloatOffset));
                                }
                                else
                                {
                                    coflowExecutionContext.WriteReferenceRelative(instruction.A, CoflowInvocationContext.ReadArenaReference(valueId, field.ReferenceOffset));
                                }
                                break;
                            }
                        case CoflowRegisterOpCode.LoadHostFieldValue:
                            {
                                CoflowRegisterFieldValueSite coflowRegisterFieldValueSite = registerProgram.Operations.FieldValues[instruction.C];
                                if (!coflowRegisterFieldValueSite.Access.IsHost)
                                {
                                    throw new InvalidOperationException("A non-Host field cannot use a CLR reader instruction.");
                                }
                                object receiver = coflowExecutionContext.ReadReferenceRelative(instruction.A) ?? throw new InvalidOperationException("field `" + coflowRegisterFieldValueSite.Access.Name + "` receiver is null");
                                coflowRegisterFieldValueSite.Access.ReadValue!(coflowExecutionContext, coflowExecutionContext.OffsetRelative(coflowRegisterFieldValueSite.Target), receiver);
                                break;
                            }
                        case CoflowRegisterOpCode.LoadArenaFieldValue:
                            {
                                var fieldSite = registerProgram.Operations.FieldValues[instruction.C];
                                if (fieldSite.Access.IsHost)
                                {
                                    throw new InvalidOperationException("A Host field cannot use a record Arena instruction.");
                                }
                                CoflowValueId id = CoflowValueId.FromPacked(unchecked((ulong)coflowExecutionContext.ReadIntegerRelative(instruction.A)));
                                CoflowInvocationContext.CopyArenaField(id, fieldSite.Access, coflowExecutionContext, coflowExecutionContext.OffsetRelative(fieldSite.Target));
                                break;
                            }
                        case CoflowRegisterOpCode.Native:
                            {
                                CoflowNativeCallSite coflowNativeCallSite = registerProgram.Operations.NativeCalls[instruction.C];
                                coflowNativeCallSite.Call.Invoke(new CoflowNativeFrame(coflowExecutionContext, coflowNativeCallSite));
                                break;
                            }
                        case CoflowRegisterOpCode.MakeArray:
                        case CoflowRegisterOpCode.MakeDictionary:
                            {
                                CoflowRegisterCollectionSite site = registerProgram.Operations.Collections[instruction.C];
                                coflowExecutionContext.MakeCollection(site, instruction.Code == CoflowRegisterOpCode.MakeDictionary);
                                break;
                            }
                        case CoflowRegisterOpCode.ArrayIndex:
                        case CoflowRegisterOpCode.DictionaryIndex:
                            {
                                var indexSite = registerProgram.Operations.Indexes[instruction.C];
                                if (instruction.Code == CoflowRegisterOpCode.ArrayIndex)
                                {
                                    coflowExecutionContext.ArrayIndex(indexSite);
                                }
                                else
                                {
                                    coflowExecutionContext.DictionaryIndex(indexSite);
                                }
                                break;
                            }
                        case CoflowRegisterOpCode.CollectionCount:
                        case CoflowRegisterOpCode.ArrayItem:
                        case CoflowRegisterOpCode.DictionaryKey:
                        case CoflowRegisterOpCode.DictionaryValue:
                            {
                                var readSite = registerProgram.Operations.CollectionReads[instruction.C];
                                coflowExecutionContext.ReadCollection(readSite, instruction.Code);
                                break;
                            }
                        case CoflowRegisterOpCode.DictionaryKeys:
                        case CoflowRegisterOpCode.DictionaryValues:
                            {
                                var projectionSite = registerProgram.Operations.Projections[instruction.C];
                                coflowExecutionContext.ProjectDictionary(projectionSite, instruction.Code == CoflowRegisterOpCode.DictionaryValues);
                                break;
                            }
                        case CoflowRegisterOpCode.CollectionBuiltin:
                            {
                                var builtinSite = registerProgram.Operations.CollectionBuiltins[instruction.C];
                                coflowExecutionContext.ExecuteCollectionBuiltin(builtinSite);
                                break;
                            }
                        case CoflowRegisterOpCode.BeginArrayBuilder:
                        case CoflowRegisterOpCode.AppendArrayBuilder:
                            {
                                var builderSite = registerProgram.Operations.ArrayBuilders[instruction.C];
                                coflowExecutionContext.ArrayBuilder(builderSite, instruction.Code == CoflowRegisterOpCode.AppendArrayBuilder);
                                break;
                            }
                        case CoflowRegisterOpCode.MakeOptionSome:
                        case CoflowRegisterOpCode.MakeResultOk:
                        case CoflowRegisterOpCode.MakeResultErr:
                            {
                                var taggedTransfer = registerProgram.Operations.Transfers[instruction.C];
                                coflowExecutionContext.CopyRelative(taggedTransfer.Source, (instruction.Code == CoflowRegisterOpCode.MakeResultErr) ? taggedTransfer.Target.Second : taggedTransfer.Target.First);
                                coflowExecutionContext.WriteIntegerRelative(taggedTransfer.Target.IntegerBase, (instruction.Code != CoflowRegisterOpCode.MakeResultErr) ? 1 : 0);
                                break;
                            }
                        case CoflowRegisterOpCode.MakeOptionNone:
                            {
                                var noneTarget = registerProgram.Operations.Targets[instruction.C];
                                coflowExecutionContext.WriteIntegerRelative(noneTarget.Target.IntegerBase, 0L);
                                break;
                            }
                        case CoflowRegisterOpCode.ReadValueTag:
                            coflowExecutionContext.WriteIntegerRelative(instruction.A, coflowExecutionContext.ReadIntegerRelative(instruction.B));
                            break;
                        case CoflowRegisterOpCode.ReadFirstPayload:
                        case CoflowRegisterOpCode.ReadSecondPayload:
                            {
                                CoflowRegisterValueTransfer coflowRegisterValueTransfer = registerProgram.Operations.Transfers[instruction.C];
                                coflowExecutionContext.CopyRelative(coflowRegisterValueTransfer.Source, coflowRegisterValueTransfer.Target);
                                break;
                            }
                        case CoflowRegisterOpCode.Propagate:
                            {
                                CoflowRegisterPropagateSite coflowRegisterPropagateSite = registerProgram.Operations.Propagates[instruction.C];
                                if (coflowExecutionContext.ReadIntegerRelative(coflowRegisterPropagateSite.Source.IntegerBase) == 0)
                                {
                                    coflowExecutionContext.WriteIntegerRelative(coflowRegisterPropagateSite.ReturnValue.IntegerBase, 0L);
                                    if (coflowRegisterPropagateSite.Source.Shape.Kind == CoflowValueShapeKind.Result)
                                    {
                                        coflowExecutionContext.CopyRelative(coflowRegisterPropagateSite.Source.Second, coflowRegisterPropagateSite.ReturnValue.Second);
                                    }
                                    coflowExecutionContext.Pc = pc;
                                    if (coflowExecutionContext.ReturnRegister<TResult>(coflowRegisterPropagateSite.ReturnValue, out var propagatedResult))
                                    {
                                        return propagatedResult;
                                    }
                                    registerProgram = coflowExecutionContext.Program.RegisterProgram;
                                    instructions = registerProgram.Instructions;
                                    pc = coflowExecutionContext.Pc;
                                }
                                else
                                {
                                    coflowExecutionContext.CopyRelative(coflowRegisterPropagateSite.Source.First, coflowRegisterPropagateSite.Payload);
                                }
                                break;
                            }
                        case CoflowRegisterOpCode.MakeClosure:
                            coflowExecutionContext.MakeClosure(registerProgram.Operations.Closures[instruction.C]);
                            break;
                        case CoflowRegisterOpCode.ConvertIntToFloat:
                            coflowExecutionContext.WriteFloatRelative(instruction.A, coflowExecutionContext.ReadIntegerRelative(instruction.B));
                            break;
                        case CoflowRegisterOpCode.ConvertFloatToInt:
                            coflowExecutionContext.WriteIntegerRelative(instruction.A, (long)coflowExecutionContext.ReadFloatRelative(instruction.B));
                            break;
                        case CoflowRegisterOpCode.IsType:
                            coflowExecutionContext.WriteIntegerRelative(instruction.A, registerProgram.Operations.Types[instruction.C].IsInstanceOfType(coflowExecutionContext.ReadReferenceRelative(instruction.B)) ? 1 : 0);
                            break;
                        case CoflowRegisterOpCode.IsArenaType:
                            coflowExecutionContext.WriteIntegerRelative(instruction.A, CoflowInvocationContext.IsType(CoflowValueId.FromPacked(unchecked((ulong)coflowExecutionContext.ReadIntegerRelative(instruction.B))), registerProgram.Operations.Types[instruction.C]) ? 1 : 0);
                            break;
                        case CoflowRegisterOpCode.NegateInt:
                            coflowExecutionContext.WriteIntegerRelative(instruction.A, -coflowExecutionContext.ReadIntegerRelative(instruction.B));
                            break;
                        case CoflowRegisterOpCode.Not:
                            coflowExecutionContext.WriteIntegerRelative(instruction.A, (coflowExecutionContext.ReadIntegerRelative(instruction.B) == 0L) ? 1 : 0);
                            break;
                        case CoflowRegisterOpCode.BitNot:
                            coflowExecutionContext.WriteIntegerRelative(instruction.A, ~coflowExecutionContext.ReadIntegerRelative(instruction.B));
                            break;
                        case CoflowRegisterOpCode.NegateFloat:
                            coflowExecutionContext.WriteFloatRelative(instruction.A, 0.0 - coflowExecutionContext.ReadFloatRelative(instruction.B));
                            break;
                        case CoflowRegisterOpCode.AddInt:
                            coflowExecutionContext.WriteIntegerRelative(instruction.A, coflowExecutionContext.ReadIntegerRelative(instruction.B) + coflowExecutionContext.ReadIntegerRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.SubtractInt:
                            coflowExecutionContext.WriteIntegerRelative(instruction.A, coflowExecutionContext.ReadIntegerRelative(instruction.B) - coflowExecutionContext.ReadIntegerRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.MultiplyInt:
                            coflowExecutionContext.WriteIntegerRelative(instruction.A, coflowExecutionContext.ReadIntegerRelative(instruction.B) * coflowExecutionContext.ReadIntegerRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.DivideInt:
                        case CoflowRegisterOpCode.IntegerDivide:
                            coflowExecutionContext.WriteIntegerRelative(instruction.A, unchecked(coflowExecutionContext.ReadIntegerRelative(instruction.B) / coflowExecutionContext.ReadIntegerRelative(instruction.C)));
                            break;
                        case CoflowRegisterOpCode.Remainder:
                            coflowExecutionContext.WriteIntegerRelative(instruction.A, unchecked(coflowExecutionContext.ReadIntegerRelative(instruction.B) % coflowExecutionContext.ReadIntegerRelative(instruction.C)));
                            break;
                        case CoflowRegisterOpCode.PowerInt:
                            coflowExecutionContext.WriteIntegerRelative(instruction.A, PowerInteger(coflowExecutionContext.ReadIntegerRelative(instruction.B), coflowExecutionContext.ReadIntegerRelative(instruction.C)));
                            break;
                        case CoflowRegisterOpCode.ShiftLeft:
                            coflowExecutionContext.WriteIntegerRelative(instruction.A, coflowExecutionContext.ReadIntegerRelative(instruction.B) << (int)coflowExecutionContext.ReadIntegerRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.ShiftRight:
                            coflowExecutionContext.WriteIntegerRelative(instruction.A, coflowExecutionContext.ReadIntegerRelative(instruction.B) >> (int)coflowExecutionContext.ReadIntegerRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.BitAnd:
                            coflowExecutionContext.WriteIntegerRelative(instruction.A, coflowExecutionContext.ReadIntegerRelative(instruction.B) & coflowExecutionContext.ReadIntegerRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.BitXor:
                            coflowExecutionContext.WriteIntegerRelative(instruction.A, coflowExecutionContext.ReadIntegerRelative(instruction.B) ^ coflowExecutionContext.ReadIntegerRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.BitOr:
                            coflowExecutionContext.WriteIntegerRelative(instruction.A, coflowExecutionContext.ReadIntegerRelative(instruction.B) | coflowExecutionContext.ReadIntegerRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.AddFloat:
                            coflowExecutionContext.WriteFloatRelative(instruction.A, coflowExecutionContext.ReadFloatRelative(instruction.B) + coflowExecutionContext.ReadFloatRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.SubtractFloat:
                            coflowExecutionContext.WriteFloatRelative(instruction.A, coflowExecutionContext.ReadFloatRelative(instruction.B) - coflowExecutionContext.ReadFloatRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.MultiplyFloat:
                            coflowExecutionContext.WriteFloatRelative(instruction.A, coflowExecutionContext.ReadFloatRelative(instruction.B) * coflowExecutionContext.ReadFloatRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.DivideFloat:
                            coflowExecutionContext.WriteFloatRelative(instruction.A, coflowExecutionContext.ReadFloatRelative(instruction.B) / coflowExecutionContext.ReadFloatRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.PowerFloat:
                            coflowExecutionContext.WriteFloatRelative(instruction.A, Math.Pow(coflowExecutionContext.ReadFloatRelative(instruction.B), coflowExecutionContext.ReadFloatRelative(instruction.C)));
                            break;
                        case CoflowRegisterOpCode.AddString:
                            coflowExecutionContext.WriteReferenceRelative(instruction.A, (string?)coflowExecutionContext.ReadReferenceRelative(instruction.B) + (string?)coflowExecutionContext.ReadReferenceRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.LessInt:
                            coflowExecutionContext.WriteBooleanRelative(instruction.A, coflowExecutionContext.ReadIntegerRelative(instruction.B) < coflowExecutionContext.ReadIntegerRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.LessOrEqualInt:
                            coflowExecutionContext.WriteBooleanRelative(instruction.A, coflowExecutionContext.ReadIntegerRelative(instruction.B) <= coflowExecutionContext.ReadIntegerRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.GreaterInt:
                            coflowExecutionContext.WriteBooleanRelative(instruction.A, coflowExecutionContext.ReadIntegerRelative(instruction.B) > coflowExecutionContext.ReadIntegerRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.GreaterOrEqualInt:
                            coflowExecutionContext.WriteBooleanRelative(instruction.A, coflowExecutionContext.ReadIntegerRelative(instruction.B) >= coflowExecutionContext.ReadIntegerRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.EqualInteger:
                            coflowExecutionContext.WriteBooleanRelative(instruction.A, coflowExecutionContext.ReadIntegerRelative(instruction.B) == coflowExecutionContext.ReadIntegerRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.LessFloat:
                            coflowExecutionContext.WriteBooleanRelative(instruction.A, coflowExecutionContext.ReadFloatRelative(instruction.B) < coflowExecutionContext.ReadFloatRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.LessOrEqualFloat:
                            coflowExecutionContext.WriteBooleanRelative(instruction.A, coflowExecutionContext.ReadFloatRelative(instruction.B) <= coflowExecutionContext.ReadFloatRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.GreaterFloat:
                            coflowExecutionContext.WriteBooleanRelative(instruction.A, coflowExecutionContext.ReadFloatRelative(instruction.B) > coflowExecutionContext.ReadFloatRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.GreaterOrEqualFloat:
                            coflowExecutionContext.WriteBooleanRelative(instruction.A, coflowExecutionContext.ReadFloatRelative(instruction.B) >= coflowExecutionContext.ReadFloatRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.EqualFloat:
                            coflowExecutionContext.WriteBooleanRelative(instruction.A, coflowExecutionContext.ReadFloatRelative(instruction.B).Equals(coflowExecutionContext.ReadFloatRelative(instruction.C)));
                            break;
                        case CoflowRegisterOpCode.LessString:
                            coflowExecutionContext.WriteBooleanRelative(instruction.A, CompareString(coflowExecutionContext, instruction) < 0);
                            break;
                        case CoflowRegisterOpCode.LessOrEqualString:
                            coflowExecutionContext.WriteBooleanRelative(instruction.A, CompareString(coflowExecutionContext, instruction) <= 0);
                            break;
                        case CoflowRegisterOpCode.GreaterString:
                            coflowExecutionContext.WriteBooleanRelative(instruction.A, CompareString(coflowExecutionContext, instruction) > 0);
                            break;
                        case CoflowRegisterOpCode.GreaterOrEqualString:
                            coflowExecutionContext.WriteBooleanRelative(instruction.A, CompareString(coflowExecutionContext, instruction) >= 0);
                            break;
                        case CoflowRegisterOpCode.EqualReference:
                            coflowExecutionContext.WriteIntegerRelative(instruction.A, object.Equals(coflowExecutionContext.ReadReferenceRelative(instruction.B), coflowExecutionContext.ReadReferenceRelative(instruction.C)) ? 1 : 0);
                            break;
                        case CoflowRegisterOpCode.JumpIfFalse:
                            if (coflowExecutionContext.ReadIntegerRelative(instruction.A) == 0)
                            {
                                pc = instruction.B;
                            }
                            break;
                        case CoflowRegisterOpCode.JumpIfTrue:
                            if (coflowExecutionContext.ReadIntegerRelative(instruction.A) != 0)
                            {
                                pc = instruction.B;
                            }
                            break;
                        case CoflowRegisterOpCode.Jump:
                            pc = instruction.A;
                            break;
                        case CoflowRegisterOpCode.Call:
                            {
                                var callSite = registerProgram.Operations.Calls[instruction.C];
                                var target = CoflowInvocationContext.LinkedFunction(callSite.ProgramIndex);
                                coflowExecutionContext.Pc = pc;
                                if (!coflowExecutionContext.Call(callSite, target.Program, tail: false))
                                {
                                    coflowExecutionContext.Budget.HostCall(BoundaryLanes(
                                        callSite.Arguments, callSite.Result));
                                    target.Entry.InvokeBoundFromVm(new CoflowNativeFrame(coflowExecutionContext, callSite.Arguments, callSite.Result, callSite.Signature.ResultType));
                                    break;
                                }
                                registerProgram = coflowExecutionContext.Program.RegisterProgram;
                                instructions = registerProgram.Instructions;
                                pc = coflowExecutionContext.Pc;
                                break;
                            }
                        case CoflowRegisterOpCode.CallIndirect:
                            {
                                coflowExecutionContext.Pc = pc;
                                if (coflowExecutionContext.CallIndirect<TResult>(registerProgram.Operations.IndirectCalls[instruction.C], tail: false, out var indirectResult))
                                {
                                    return indirectResult;
                                }
                                registerProgram = coflowExecutionContext.Program.RegisterProgram;
                                instructions = registerProgram.Instructions;
                                pc = coflowExecutionContext.Pc;
                                break;
                            }
                        case CoflowRegisterOpCode.TailCall:
                            {
                                var tailCallSite = registerProgram.Operations.Calls[instruction.C];
                                var tailTarget = CoflowInvocationContext.LinkedFunction(tailCallSite.ProgramIndex);
                                coflowExecutionContext.Pc = pc;
                                if (!coflowExecutionContext.Call(tailCallSite, tailTarget.Program, tail: true))
                                {
                                    coflowExecutionContext.Budget.HostCall(BoundaryLanes(
                                        tailCallSite.Arguments, tailCallSite.Result));
                                    tailTarget.Entry.InvokeBoundFromVm(new CoflowNativeFrame(coflowExecutionContext, tailCallSite.Arguments, tailCallSite.Result, tailCallSite.Signature.ResultType));
                                    if (coflowExecutionContext.ReturnRegister<TResult>(tailCallSite.Result, out var tailResult))
                                    {
                                        return tailResult;
                                    }
                                }
                                registerProgram = coflowExecutionContext.Program.RegisterProgram;
                                instructions = registerProgram.Instructions;
                                pc = coflowExecutionContext.Pc;
                                break;
                            }
                        case CoflowRegisterOpCode.TailCallIndirect:
                            {
                                coflowExecutionContext.Pc = pc;
                                if (coflowExecutionContext.CallIndirect<TResult>(registerProgram.Operations.IndirectCalls[instruction.C], tail: true, out var returned))
                                {
                                    return returned;
                                }
                                registerProgram = coflowExecutionContext.Program.RegisterProgram;
                                instructions = registerProgram.Instructions;
                                pc = coflowExecutionContext.Pc;
                                break;
                            }
                        case CoflowRegisterOpCode.Return:
                            {
                                CoflowRegisterTargetSite coflowRegisterTargetSite = registerProgram.Operations.Targets[instruction.C];
                                coflowExecutionContext.Pc = pc;
                                if (coflowExecutionContext.ReturnRegister<TResult>(coflowRegisterTargetSite.Target, out var result))
                                {
                                    return result;
                                }
                                registerProgram = coflowExecutionContext.Program.RegisterProgram;
                                instructions = registerProgram.Instructions;
                                pc = coflowExecutionContext.Pc;
                                break;
                            }
                        default:
                            throw new InvalidOperationException($"Unknown Coflow opcode `{instruction.Code}`.");
                        case CoflowRegisterOpCode.Nop:
                            break;
                    }
                }
            }
            throw new InvalidOperationException("Coflow function ended without Return.");
        }

        catch (CoflowFaultException)
        {
            throw;
        }
        catch (Exception error)
        {
            CoflowRegisterProgram faultProgram = currentProgram.RegisterProgram;
            CfdSpan? span = (uint)faultPc < (uint)faultProgram.InstructionSpans.Length
                ? faultProgram.InstructionSpans[faultPc] : null;
            throw Fault(currentProgram, error.Message, error, coflowExecutionContext.CallStack, span);
        }
        finally
        {
            coflowExecutionContext.Dispose();
        }
    }

    private static long BoundaryLanes(IReadOnlyList<CoflowValueRegister> arguments, CoflowValueRegister result)
    {
        var lanes = (long)result.Shape.IntegerCount + result.Shape.FloatCount + result.Shape.ReferenceCount;
        foreach (var argument in arguments)
            lanes += (long)argument.Shape.IntegerCount + argument.Shape.FloatCount + argument.Shape.ReferenceCount;
        return lanes;
    }

    private static CoflowExecutionContext RentContext()
    {
        CoflowExecutionContext? coflowExecutionContext = _pooledContexts;
        if (coflowExecutionContext == null)
        {
            coflowExecutionContext = new CoflowExecutionContext();
        }
        else
        {
            _pooledContexts = coflowExecutionContext.NextPooled;
            coflowExecutionContext.NextPooled = null;
        }
        coflowExecutionContext.Reset();
        return coflowExecutionContext;
    }

    private static int CompareString(CoflowExecutionContext context, CoflowRegisterInstruction instruction)
    {
        return string.CompareOrdinal((string?)context.ReadReferenceRelative(instruction.B), (string?)context.ReadReferenceRelative(instruction.C));
    }

    private static long PowerInteger(long value, long exponent)
    {
        if (exponent < 0)
        {
            throw new InvalidOperationException("integer exponent must be non-negative");
        }
        long result = 1L;
        long factor = value;
        checked
        {
            while (exponent != 0)
            {
                if ((exponent & 1) != 0)
                {
                    result *= factor;
                }
                if (exponent > 1)
                {
                    factor *= factor;
                }
                exponent >>= 1;
            }
            return result;
        }
    }

    private static CoflowFaultException Fault(CoflowProgram program, string message, Exception? inner = null, IEnumerable<CoflowFunctionIdentity>? stack = null, CfdSpan? span = null)
    {
        return new CoflowFaultException(program.Identity, program.SourcePath, span ?? program.SourceSpan, (stack ?? new CoflowFunctionIdentity[1] { program.Identity }).Take(32).ToArray(), message, inner);
    }
}
