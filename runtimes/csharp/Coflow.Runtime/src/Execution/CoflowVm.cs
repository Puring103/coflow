using System.Threading.Tasks;
using System.Threading;
using System.IO;
using System;
using System.Buffers;
using System.Collections.Generic;
using System.Linq;
using System.Runtime.CompilerServices;

namespace Coflow.Runtime.CompilerServices
{

internal static class CoflowVm
{
    [ThreadStatic]
    private static CoflowExecutionSession? _pooledContexts;

    internal static TResult Execute<TArguments, TResult>(CoflowProgram program, TArguments arguments)
        where TArguments : struct, ICoflowArgumentPack =>
        ExecuteCore<TArguments, TResult>(program, arguments);

    internal static TResult ExecuteRaw<T1, TResult>(CoflowProgram program, T1 argument) =>
        ExecuteCore<CoflowRawArguments1<T1>, TResult>(program, new CoflowRawArguments1<T1>(argument), uint.MaxValue);

    internal static TResult ExecuteReceiver<TReceiver, TArguments, TResult>(
        CoflowProgram program,
        TReceiver receiver,
        TArguments arguments)
        where TArguments : struct, ICoflowArgumentPack =>
        ExecuteCore<CoflowReceiverArguments<TReceiver, TArguments>, TResult>(
            program,
            new CoflowReceiverArguments<TReceiver, TArguments>(receiver, arguments));

    internal static TResult ExecuteBound<TArguments, TResult>(
        CoflowProgram program,
        object receiver,
        TArguments arguments)
        where TArguments : struct, ICoflowArgumentPack =>
        ExecuteCore<CoflowBoxedReceiverArguments<TArguments>, TResult>(
            program,
            new CoflowBoxedReceiverArguments<TArguments>(receiver, arguments));

    internal static TResult ExecuteClosure<TArguments, TResult>(CoflowClosure closure, TArguments arguments)
        where TArguments : struct, ICoflowArgumentPack
    {
        using (closure.Owner.EnterExecution())
        {
            return ExecuteCore<CoflowClosureArguments<TArguments>, TResult>(
                closure.Program,
                new CoflowClosureArguments<TArguments>(closure, arguments),
                closure: closure);
        }
    }

    private static TResult ExecuteCore<TArguments, TResult>(CoflowProgram program, TArguments arguments,
        uint? standaloneGeneration = null, CoflowClosure? closure = null) where TArguments : struct, ICoflowArgumentPack
    {
        if (arguments.Count != program.ParameterCount)
        {
            throw Fault(program, $"function expected {program.ParameterCount} arguments but received {arguments.Count}");
        }
        CoflowExecutionSession coflowExecutionContext = RentContext();
        CoflowProgram currentProgram = program;
        int faultPc = 0;
        try
        {
            coflowExecutionContext.Start(program, arguments, standaloneGeneration, closure);
            CoflowRegisterProgram registerProgram = coflowExecutionContext.Program.RegisterProgram;
            CoflowFrozenArray<CoflowRegisterInstruction> instructions = registerProgram.Instructions;
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
                            coflowExecutionContext.Registers.WriteIntegerRelative(instruction.A, registerProgram.Immediates[instruction.B]);
                            break;
                        case CoflowRegisterOpCode.ConstantFloat:
                            coflowExecutionContext.Registers.WriteFloatRelative(instruction.A, BitConverter.Int64BitsToDouble(registerProgram.Immediates[instruction.B]));
                            break;
                        case CoflowRegisterOpCode.ConstantReference:
                            coflowExecutionContext.Registers.WriteReferenceRelative(instruction.A, registerProgram.Operations.References[instruction.C]);
                            break;
                        case CoflowRegisterOpCode.ConstantValue:
                            {
                                CoflowRegisterConstantSite coflowRegisterConstantSite = registerProgram.Operations.Constants[instruction.C];
                                coflowExecutionContext.WriteEncodedRelative(coflowRegisterConstantSite.Value, coflowRegisterConstantSite.Target);
                                break;
                            }
                        case CoflowRegisterOpCode.MoveInteger:
                            coflowExecutionContext.Registers.WriteIntegerRelative(instruction.A, coflowExecutionContext.Registers.ReadIntegerRelative(instruction.B));
                            break;
                        case CoflowRegisterOpCode.MoveFloat:
                            coflowExecutionContext.Registers.WriteFloatRelative(instruction.A, coflowExecutionContext.Registers.ReadFloatRelative(instruction.B));
                            break;
                        case CoflowRegisterOpCode.MoveReference:
                            coflowExecutionContext.Registers.WriteReferenceRelative(instruction.A, coflowExecutionContext.Registers.ReadReferenceRelative(instruction.B));
                            break;
                        case CoflowRegisterOpCode.ClearReference:
                            coflowExecutionContext.Registers.WriteReferenceRelative(instruction.A, null);
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
                                object arg = coflowExecutionContext.Registers.ReadReferenceRelative(instruction.B) ?? throw new InvalidOperationException("field `" + coflowFieldAccess.Name + "` receiver is null");
                                if (instruction.Code == CoflowRegisterOpCode.LoadHostFieldInteger)
                                {
                                    coflowExecutionContext.Registers.WriteIntegerRelative(instruction.A, coflowFieldAccess.ReadInteger!(arg));
                                }
                                else if (instruction.Code == CoflowRegisterOpCode.LoadHostFieldFloat)
                                {
                                    coflowExecutionContext.Registers.WriteFloatRelative(instruction.A, coflowFieldAccess.ReadFloat!(arg));
                                }
                                else
                                {
                                    coflowExecutionContext.Registers.WriteReferenceRelative(instruction.A, coflowFieldAccess.ReadReference!(arg));
                                }
                                break;
                            }
                        case CoflowRegisterOpCode.LoadArenaFieldInteger:
                        case CoflowRegisterOpCode.LoadArenaFieldFloat:
                        case CoflowRegisterOpCode.LoadArenaFieldReference:
                            {
                                var field = registerProgram.Operations.Fields[instruction.C];
                                var valueId = CoflowValueId.FromPacked(unchecked((ulong)coflowExecutionContext.Registers.ReadIntegerRelative(instruction.B)));
                                if (instruction.Code == CoflowRegisterOpCode.LoadArenaFieldInteger)
                                {
                                    coflowExecutionContext.Registers.WriteIntegerRelative(instruction.A, coflowExecutionContext.Environment.ReadArenaInteger(valueId, field.IntegerOffset));
                                }
                                else if (instruction.Code == CoflowRegisterOpCode.LoadArenaFieldFloat)
                                {
                                    coflowExecutionContext.Registers.WriteFloatRelative(instruction.A, coflowExecutionContext.Environment.ReadArenaFloat(valueId, field.FloatOffset));
                                }
                                else
                                {
                                    coflowExecutionContext.Registers.WriteReferenceRelative(instruction.A, coflowExecutionContext.Environment.ReadArenaReference(valueId, field.ReferenceOffset));
                                }
                                break;
                            }
                        case CoflowRegisterOpCode.LoadHostFieldValue:
                            {
                                CoflowRegisterFieldValueSite coflowRegisterFieldValueSite = registerProgram.Operations.FieldValues[instruction.C];
                                object receiver = coflowExecutionContext.Registers.ReadReferenceRelative(instruction.A) ?? throw new InvalidOperationException("field `" + coflowRegisterFieldValueSite.Access.Name + "` receiver is null");
                                coflowRegisterFieldValueSite.Access.ReadValue!(coflowExecutionContext, coflowExecutionContext.OffsetRelative(coflowRegisterFieldValueSite.Target), receiver);
                                break;
                            }
                        case CoflowRegisterOpCode.LoadArenaFieldValue:
                            {
                                var fieldSite = registerProgram.Operations.FieldValues[instruction.C];
                                CoflowValueId id = CoflowValueId.FromPacked(unchecked((ulong)coflowExecutionContext.Registers.ReadIntegerRelative(instruction.A)));
                                coflowExecutionContext.Environment.CopyArenaField(id, fieldSite.Access, coflowExecutionContext, coflowExecutionContext.OffsetRelative(fieldSite.Target));
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
                                coflowExecutionContext.Registers.WriteIntegerRelative(taggedTransfer.Target.IntegerBase, (instruction.Code != CoflowRegisterOpCode.MakeResultErr) ? 1 : 0);
                                break;
                            }
                        case CoflowRegisterOpCode.MakeOptionNone:
                            {
                                var noneTarget = registerProgram.Operations.Targets[instruction.C];
                                coflowExecutionContext.Registers.WriteIntegerRelative(noneTarget.Target.IntegerBase, 0L);
                                break;
                            }
                        case CoflowRegisterOpCode.ReadValueTag:
                            coflowExecutionContext.Registers.WriteIntegerRelative(instruction.A, coflowExecutionContext.Registers.ReadIntegerRelative(instruction.B));
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
                                if (coflowExecutionContext.Registers.ReadIntegerRelative(coflowRegisterPropagateSite.Source.IntegerBase) == 0)
                                {
                                    coflowExecutionContext.Registers.WriteIntegerRelative(coflowRegisterPropagateSite.ReturnValue.IntegerBase, 0L);
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
                            coflowExecutionContext.Registers.WriteFloatRelative(instruction.A, coflowExecutionContext.Registers.ReadIntegerRelative(instruction.B));
                            break;
                        case CoflowRegisterOpCode.ConvertFloatToInt:
                            coflowExecutionContext.Registers.WriteIntegerRelative(instruction.A, (long)coflowExecutionContext.Registers.ReadFloatRelative(instruction.B));
                            break;
                        case CoflowRegisterOpCode.IsType:
                            coflowExecutionContext.Registers.WriteIntegerRelative(instruction.A, registerProgram.Operations.Types[instruction.C].IsInstanceOfType(coflowExecutionContext.Registers.ReadReferenceRelative(instruction.B)) ? 1 : 0);
                            break;
                        case CoflowRegisterOpCode.IsArenaType:
                            coflowExecutionContext.Registers.WriteIntegerRelative(instruction.A, coflowExecutionContext.Environment.IsType(CoflowValueId.FromPacked(unchecked((ulong)coflowExecutionContext.Registers.ReadIntegerRelative(instruction.B))), registerProgram.Operations.Types[instruction.C]) ? 1 : 0);
                            break;
                        case CoflowRegisterOpCode.NegateInt:
                            coflowExecutionContext.Registers.WriteIntegerRelative(instruction.A, -coflowExecutionContext.Registers.ReadIntegerRelative(instruction.B));
                            break;
                        case CoflowRegisterOpCode.Not:
                            coflowExecutionContext.Registers.WriteIntegerRelative(instruction.A, (coflowExecutionContext.Registers.ReadIntegerRelative(instruction.B) == 0L) ? 1 : 0);
                            break;
                        case CoflowRegisterOpCode.BitNot:
                            coflowExecutionContext.Registers.WriteIntegerRelative(instruction.A, ~coflowExecutionContext.Registers.ReadIntegerRelative(instruction.B));
                            break;
                        case CoflowRegisterOpCode.NegateFloat:
                            coflowExecutionContext.Registers.WriteFloatRelative(instruction.A, 0.0 - coflowExecutionContext.Registers.ReadFloatRelative(instruction.B));
                            break;
                        case CoflowRegisterOpCode.AddInt:
                            coflowExecutionContext.Registers.WriteIntegerRelative(instruction.A, coflowExecutionContext.Registers.ReadIntegerRelative(instruction.B) + coflowExecutionContext.Registers.ReadIntegerRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.SubtractInt:
                            coflowExecutionContext.Registers.WriteIntegerRelative(instruction.A, coflowExecutionContext.Registers.ReadIntegerRelative(instruction.B) - coflowExecutionContext.Registers.ReadIntegerRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.MultiplyInt:
                            coflowExecutionContext.Registers.WriteIntegerRelative(instruction.A, coflowExecutionContext.Registers.ReadIntegerRelative(instruction.B) * coflowExecutionContext.Registers.ReadIntegerRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.DivideInt:
                        case CoflowRegisterOpCode.IntegerDivide:
                            coflowExecutionContext.Registers.WriteIntegerRelative(instruction.A, unchecked(coflowExecutionContext.Registers.ReadIntegerRelative(instruction.B) / coflowExecutionContext.Registers.ReadIntegerRelative(instruction.C)));
                            break;
                        case CoflowRegisterOpCode.Remainder:
                            coflowExecutionContext.Registers.WriteIntegerRelative(instruction.A, unchecked(coflowExecutionContext.Registers.ReadIntegerRelative(instruction.B) % coflowExecutionContext.Registers.ReadIntegerRelative(instruction.C)));
                            break;
                        case CoflowRegisterOpCode.PowerInt:
                            coflowExecutionContext.Registers.WriteIntegerRelative(instruction.A, PowerInteger(coflowExecutionContext.Registers.ReadIntegerRelative(instruction.B), coflowExecutionContext.Registers.ReadIntegerRelative(instruction.C)));
                            break;
                        case CoflowRegisterOpCode.ShiftLeft:
                            coflowExecutionContext.Registers.WriteIntegerRelative(instruction.A, coflowExecutionContext.Registers.ReadIntegerRelative(instruction.B) << (int)coflowExecutionContext.Registers.ReadIntegerRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.ShiftRight:
                            coflowExecutionContext.Registers.WriteIntegerRelative(instruction.A, coflowExecutionContext.Registers.ReadIntegerRelative(instruction.B) >> (int)coflowExecutionContext.Registers.ReadIntegerRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.BitAnd:
                            coflowExecutionContext.Registers.WriteIntegerRelative(instruction.A, coflowExecutionContext.Registers.ReadIntegerRelative(instruction.B) & coflowExecutionContext.Registers.ReadIntegerRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.BitXor:
                            coflowExecutionContext.Registers.WriteIntegerRelative(instruction.A, coflowExecutionContext.Registers.ReadIntegerRelative(instruction.B) ^ coflowExecutionContext.Registers.ReadIntegerRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.BitOr:
                            coflowExecutionContext.Registers.WriteIntegerRelative(instruction.A, coflowExecutionContext.Registers.ReadIntegerRelative(instruction.B) | coflowExecutionContext.Registers.ReadIntegerRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.AddFloat:
                            coflowExecutionContext.Registers.WriteFloatRelative(instruction.A, coflowExecutionContext.Registers.ReadFloatRelative(instruction.B) + coflowExecutionContext.Registers.ReadFloatRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.SubtractFloat:
                            coflowExecutionContext.Registers.WriteFloatRelative(instruction.A, coflowExecutionContext.Registers.ReadFloatRelative(instruction.B) - coflowExecutionContext.Registers.ReadFloatRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.MultiplyFloat:
                            coflowExecutionContext.Registers.WriteFloatRelative(instruction.A, coflowExecutionContext.Registers.ReadFloatRelative(instruction.B) * coflowExecutionContext.Registers.ReadFloatRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.DivideFloat:
                            coflowExecutionContext.Registers.WriteFloatRelative(instruction.A, coflowExecutionContext.Registers.ReadFloatRelative(instruction.B) / coflowExecutionContext.Registers.ReadFloatRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.PowerFloat:
                            coflowExecutionContext.Registers.WriteFloatRelative(instruction.A, Math.Pow(coflowExecutionContext.Registers.ReadFloatRelative(instruction.B), coflowExecutionContext.Registers.ReadFloatRelative(instruction.C)));
                            break;
                        case CoflowRegisterOpCode.AddString:
                            coflowExecutionContext.Registers.WriteReferenceRelative(instruction.A, (string?)coflowExecutionContext.Registers.ReadReferenceRelative(instruction.B) + (string?)coflowExecutionContext.Registers.ReadReferenceRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.LessInt:
                            coflowExecutionContext.WriteBooleanRelative(instruction.A, coflowExecutionContext.Registers.ReadIntegerRelative(instruction.B) < coflowExecutionContext.Registers.ReadIntegerRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.LessOrEqualInt:
                            coflowExecutionContext.WriteBooleanRelative(instruction.A, coflowExecutionContext.Registers.ReadIntegerRelative(instruction.B) <= coflowExecutionContext.Registers.ReadIntegerRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.GreaterInt:
                            coflowExecutionContext.WriteBooleanRelative(instruction.A, coflowExecutionContext.Registers.ReadIntegerRelative(instruction.B) > coflowExecutionContext.Registers.ReadIntegerRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.GreaterOrEqualInt:
                            coflowExecutionContext.WriteBooleanRelative(instruction.A, coflowExecutionContext.Registers.ReadIntegerRelative(instruction.B) >= coflowExecutionContext.Registers.ReadIntegerRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.EqualInteger:
                            coflowExecutionContext.WriteBooleanRelative(instruction.A, coflowExecutionContext.Registers.ReadIntegerRelative(instruction.B) == coflowExecutionContext.Registers.ReadIntegerRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.LessFloat:
                            coflowExecutionContext.WriteBooleanRelative(instruction.A, coflowExecutionContext.Registers.ReadFloatRelative(instruction.B) < coflowExecutionContext.Registers.ReadFloatRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.LessOrEqualFloat:
                            coflowExecutionContext.WriteBooleanRelative(instruction.A, coflowExecutionContext.Registers.ReadFloatRelative(instruction.B) <= coflowExecutionContext.Registers.ReadFloatRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.GreaterFloat:
                            coflowExecutionContext.WriteBooleanRelative(instruction.A, coflowExecutionContext.Registers.ReadFloatRelative(instruction.B) > coflowExecutionContext.Registers.ReadFloatRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.GreaterOrEqualFloat:
                            coflowExecutionContext.WriteBooleanRelative(instruction.A, coflowExecutionContext.Registers.ReadFloatRelative(instruction.B) >= coflowExecutionContext.Registers.ReadFloatRelative(instruction.C));
                            break;
                        case CoflowRegisterOpCode.EqualFloat:
                            coflowExecutionContext.WriteBooleanRelative(instruction.A, coflowExecutionContext.Registers.ReadFloatRelative(instruction.B).Equals(coflowExecutionContext.Registers.ReadFloatRelative(instruction.C)));
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
                            coflowExecutionContext.Registers.WriteIntegerRelative(instruction.A, object.Equals(coflowExecutionContext.Registers.ReadReferenceRelative(instruction.B), coflowExecutionContext.Registers.ReadReferenceRelative(instruction.C)) ? 1 : 0);
                            break;
                        case CoflowRegisterOpCode.JumpIfFalse:
                            if (coflowExecutionContext.Registers.ReadIntegerRelative(instruction.A) == 0)
                            {
                                pc = instruction.B;
                            }
                            break;
                        case CoflowRegisterOpCode.JumpIfTrue:
                            if (coflowExecutionContext.Registers.ReadIntegerRelative(instruction.A) != 0)
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
                                var target = coflowExecutionContext.Environment.LinkedFunction(callSite.ProgramIndex);
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
                                var tailTarget = coflowExecutionContext.Environment.LinkedFunction(tailCallSite.ProgramIndex);
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

    internal static long BoundaryLanes(IReadOnlyList<CoflowValueRegister> arguments, CoflowValueRegister result)
    {
        var lanes = (long)result.Shape.IntegerCount + result.Shape.FloatCount + result.Shape.ReferenceCount;
        foreach (var argument in arguments)
            lanes += (long)argument.Shape.IntegerCount + argument.Shape.FloatCount + argument.Shape.ReferenceCount;
        return lanes;
    }

    private static CoflowExecutionSession RentContext()
    {
        CoflowExecutionSession? coflowExecutionContext = _pooledContexts;
        if (coflowExecutionContext == null)
        {
            coflowExecutionContext = new CoflowExecutionSession();
        }
        else
        {
            _pooledContexts = coflowExecutionContext.NextPooled;
            coflowExecutionContext.NextPooled = null;
        }
        coflowExecutionContext.Reset();
        return coflowExecutionContext;
    }

    internal static void ReturnContext(CoflowExecutionSession context)
    {
        context.NextPooled = _pooledContexts;
        _pooledContexts = context;
    }

    private static int CompareString(CoflowExecutionSession context, CoflowRegisterInstruction instruction)
    {
        return string.CompareOrdinal((string?)context.Registers.ReadReferenceRelative(instruction.B), (string?)context.Registers.ReadReferenceRelative(instruction.C));
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
}
