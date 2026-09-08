using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System.Collections.Generic;
using System;
namespace Coflow.Runtime.CompilerServices
{

internal enum CoflowRegisterDescriptorKind : byte
{
    None, Reference, Constant, Transfer, Field, FieldValue, NativeCall, Collection, Index,
    CollectionRead, Projection, CollectionBuiltin, ArrayBuilder, Target, Propagate, Closure,
    Type, Call, IndirectCall,
}

internal enum CoflowRegisterOperandAccess : byte { None, Read, Write }
internal enum CoflowRegisterControlFlow : byte { Next, Branch, Jump, Terminal, Propagate }

internal readonly struct CoflowRegisterOperandSpec
{
    public CoflowRegisterKind? Kind { get; init; }
    public CoflowRegisterOperandAccess Access { get; init; }

    public CoflowRegisterOperandSpec(CoflowRegisterKind? Kind, CoflowRegisterOperandAccess Access)
    {
        this.Kind = Kind;
        this.Access = Access;
    }

    internal static readonly CoflowRegisterOperandSpec None = new(null, CoflowRegisterOperandAccess.None);
}

internal readonly struct CoflowRegisterOpSpec
{
    public CoflowRegisterOperandSpec A { get; init; }
    public CoflowRegisterOperandSpec B { get; init; }
    public CoflowRegisterOperandSpec C { get; init; }
    public CoflowRegisterDescriptorKind Descriptor { get; init; }
    public CoflowRegisterControlFlow ControlFlow { get; init; }

    public CoflowRegisterOpSpec(CoflowRegisterOperandSpec A, CoflowRegisterOperandSpec B, CoflowRegisterOperandSpec C, CoflowRegisterDescriptorKind Descriptor, CoflowRegisterControlFlow ControlFlow)
    {
        this.A = A;
        this.B = B;
        this.C = C;
        this.Descriptor = Descriptor;
        this.ControlFlow = ControlFlow;
    }
}

/// <summary>最终指令的操作数、descriptor 与控制流规格；编码、验证和 CFG 必须共同使用。</summary>
internal static class CoflowRegisterInstructionSpec
{
    internal static IReadOnlyList<CoflowRegisterOpCode> All { get; } =
        System.Enum.GetValues(typeof(CoflowRegisterOpCode)).Cast<CoflowRegisterOpCode>().ToArray();

    private static readonly CoflowRegisterOperandSpec None = CoflowRegisterOperandSpec.None;
    private static readonly CoflowRegisterOperandSpec ReadInteger = new(CoflowRegisterKind.Integer, CoflowRegisterOperandAccess.Read);
    private static readonly CoflowRegisterOperandSpec ReadFloat = new(CoflowRegisterKind.Float, CoflowRegisterOperandAccess.Read);
    private static readonly CoflowRegisterOperandSpec ReadReference = new(CoflowRegisterKind.Reference, CoflowRegisterOperandAccess.Read);
    private static readonly CoflowRegisterOperandSpec WriteInteger = new(CoflowRegisterKind.Integer, CoflowRegisterOperandAccess.Write);
    private static readonly CoflowRegisterOperandSpec WriteFloat = new(CoflowRegisterKind.Float, CoflowRegisterOperandAccess.Write);
    private static readonly CoflowRegisterOperandSpec WriteReference = new(CoflowRegisterKind.Reference, CoflowRegisterOperandAccess.Write);

    internal static bool IsKnown(CoflowRegisterOpCode code) =>
        (uint)code <= (uint)CoflowRegisterOpCode.Return && All[(int)code] == code;
    internal static CoflowRegisterKind? OperandKind(CoflowRegisterOpCode code, int operand) => Operand(Describe(code), operand).Kind;
    internal static CoflowRegisterOperandAccess OperandAccess(CoflowRegisterOpCode code, int operand) => Operand(Describe(code), operand).Access;
    internal static CoflowRegisterDescriptorKind Descriptor(CoflowRegisterOpCode code) => Describe(code).Descriptor;
    internal static CoflowRegisterControlFlow ControlFlow(CoflowRegisterOpCode code) => Describe(code).ControlFlow;
    internal static bool HasDescriptor(CoflowRegisterOpCode code) => Descriptor(code) != CoflowRegisterDescriptorKind.None;

    private static CoflowRegisterOperandSpec Operand(CoflowRegisterOpSpec spec, int operand) => operand switch
    {
        0 => spec.A,
        1 => spec.B,
        2 => spec.C,
        _ => throw new ArgumentOutOfRangeException(nameof(operand)),
    };

    private static CoflowRegisterOpSpec Describe(CoflowRegisterOpCode code)
    {
        if (!IsKnown(code)) throw new ArgumentOutOfRangeException(nameof(code), code, "Unknown register opcode.");
        return code switch
        {
            CoflowRegisterOpCode.Nop => Spec(),
            CoflowRegisterOpCode.ConstantInteger => Spec(WriteInteger),
            CoflowRegisterOpCode.ConstantFloat => Spec(WriteFloat),
            CoflowRegisterOpCode.ConstantReference => Spec(WriteReference, descriptor: CoflowRegisterDescriptorKind.Reference),
            CoflowRegisterOpCode.ConstantValue => Spec(descriptor: CoflowRegisterDescriptorKind.Constant),
            CoflowRegisterOpCode.MoveInteger => Spec(WriteInteger, ReadInteger),
            CoflowRegisterOpCode.MoveFloat => Spec(WriteFloat, ReadFloat),
            CoflowRegisterOpCode.MoveReference => Spec(WriteReference, ReadReference),
            CoflowRegisterOpCode.ClearReference => Spec(WriteReference),
            CoflowRegisterOpCode.MoveValue => Spec(descriptor: CoflowRegisterDescriptorKind.Transfer),

            CoflowRegisterOpCode.LoadHostFieldInteger => Spec(WriteInteger, ReadReference, descriptor: CoflowRegisterDescriptorKind.Field),
            CoflowRegisterOpCode.LoadHostFieldFloat => Spec(WriteFloat, ReadReference, descriptor: CoflowRegisterDescriptorKind.Field),
            CoflowRegisterOpCode.LoadHostFieldReference => Spec(WriteReference, ReadReference, descriptor: CoflowRegisterDescriptorKind.Field),
            CoflowRegisterOpCode.LoadHostFieldValue => Spec(ReadReference, descriptor: CoflowRegisterDescriptorKind.FieldValue),
            CoflowRegisterOpCode.LoadArenaFieldInteger => Spec(WriteInteger, ReadInteger, descriptor: CoflowRegisterDescriptorKind.Field),
            CoflowRegisterOpCode.LoadArenaFieldFloat => Spec(WriteFloat, ReadInteger, descriptor: CoflowRegisterDescriptorKind.Field),
            CoflowRegisterOpCode.LoadArenaFieldReference => Spec(WriteReference, ReadInteger, descriptor: CoflowRegisterDescriptorKind.Field),
            CoflowRegisterOpCode.LoadArenaFieldValue => Spec(ReadInteger, descriptor: CoflowRegisterDescriptorKind.FieldValue),
            CoflowRegisterOpCode.Native => Spec(descriptor: CoflowRegisterDescriptorKind.NativeCall),

            CoflowRegisterOpCode.MakeArray or CoflowRegisterOpCode.MakeDictionary => Spec(descriptor: CoflowRegisterDescriptorKind.Collection),
            CoflowRegisterOpCode.ArrayIndex or CoflowRegisterOpCode.DictionaryIndex => Spec(descriptor: CoflowRegisterDescriptorKind.Index),
            CoflowRegisterOpCode.CollectionCount or CoflowRegisterOpCode.ArrayItem or
                CoflowRegisterOpCode.DictionaryKey or CoflowRegisterOpCode.DictionaryValue => Spec(descriptor: CoflowRegisterDescriptorKind.CollectionRead),
            CoflowRegisterOpCode.DictionaryKeys or CoflowRegisterOpCode.DictionaryValues => Spec(descriptor: CoflowRegisterDescriptorKind.Projection),
            CoflowRegisterOpCode.CollectionBuiltin => Spec(descriptor: CoflowRegisterDescriptorKind.CollectionBuiltin),
            CoflowRegisterOpCode.BeginArrayBuilder or CoflowRegisterOpCode.AppendArrayBuilder => Spec(descriptor: CoflowRegisterDescriptorKind.ArrayBuilder),

            CoflowRegisterOpCode.MakeOptionNone => Spec(descriptor: CoflowRegisterDescriptorKind.Target),
            CoflowRegisterOpCode.MakeOptionSome or CoflowRegisterOpCode.MakeResultOk or CoflowRegisterOpCode.MakeResultErr or
                CoflowRegisterOpCode.ReadFirstPayload or CoflowRegisterOpCode.ReadSecondPayload => Spec(descriptor: CoflowRegisterDescriptorKind.Transfer),
            CoflowRegisterOpCode.ReadValueTag => Spec(WriteInteger, ReadInteger),
            CoflowRegisterOpCode.Propagate => Spec(descriptor: CoflowRegisterDescriptorKind.Propagate, controlFlow: CoflowRegisterControlFlow.Propagate),
            CoflowRegisterOpCode.MakeClosure => Spec(descriptor: CoflowRegisterDescriptorKind.Closure),

            CoflowRegisterOpCode.ConvertIntToFloat => Spec(WriteFloat, ReadInteger),
            CoflowRegisterOpCode.ConvertFloatToInt => Spec(WriteInteger, ReadFloat),
            CoflowRegisterOpCode.IsType => Spec(WriteInteger, ReadReference, descriptor: CoflowRegisterDescriptorKind.Type),
            CoflowRegisterOpCode.IsArenaType => Spec(WriteInteger, ReadInteger, descriptor: CoflowRegisterDescriptorKind.Type),
            CoflowRegisterOpCode.NegateInt or CoflowRegisterOpCode.Not or CoflowRegisterOpCode.BitNot => Spec(WriteInteger, ReadInteger),
            CoflowRegisterOpCode.NegateFloat => Spec(WriteFloat, ReadFloat),

            CoflowRegisterOpCode.AddInt or CoflowRegisterOpCode.SubtractInt or CoflowRegisterOpCode.MultiplyInt or
                CoflowRegisterOpCode.DivideInt or CoflowRegisterOpCode.IntegerDivide or CoflowRegisterOpCode.Remainder or
                CoflowRegisterOpCode.PowerInt or CoflowRegisterOpCode.ShiftLeft or CoflowRegisterOpCode.ShiftRight or
                CoflowRegisterOpCode.BitAnd or CoflowRegisterOpCode.BitXor or CoflowRegisterOpCode.BitOr =>
                Spec(WriteInteger, ReadInteger, ReadInteger),
            CoflowRegisterOpCode.AddFloat or CoflowRegisterOpCode.SubtractFloat or CoflowRegisterOpCode.MultiplyFloat or
                CoflowRegisterOpCode.DivideFloat or CoflowRegisterOpCode.PowerFloat => Spec(WriteFloat, ReadFloat, ReadFloat),
            CoflowRegisterOpCode.AddString => Spec(WriteReference, ReadReference, ReadReference),

            CoflowRegisterOpCode.LessInt or CoflowRegisterOpCode.LessOrEqualInt or CoflowRegisterOpCode.GreaterInt or
                CoflowRegisterOpCode.GreaterOrEqualInt or CoflowRegisterOpCode.EqualInteger => Spec(WriteInteger, ReadInteger, ReadInteger),
            CoflowRegisterOpCode.LessFloat or CoflowRegisterOpCode.LessOrEqualFloat or CoflowRegisterOpCode.GreaterFloat or
                CoflowRegisterOpCode.GreaterOrEqualFloat or CoflowRegisterOpCode.EqualFloat => Spec(WriteInteger, ReadFloat, ReadFloat),
            CoflowRegisterOpCode.LessString or CoflowRegisterOpCode.LessOrEqualString or CoflowRegisterOpCode.GreaterString or
                CoflowRegisterOpCode.GreaterOrEqualString or CoflowRegisterOpCode.EqualReference => Spec(WriteInteger, ReadReference, ReadReference),

            CoflowRegisterOpCode.JumpIfFalse or CoflowRegisterOpCode.JumpIfTrue => Spec(ReadInteger, controlFlow: CoflowRegisterControlFlow.Branch),
            CoflowRegisterOpCode.Jump => Spec(controlFlow: CoflowRegisterControlFlow.Jump),
            CoflowRegisterOpCode.Call => Spec(descriptor: CoflowRegisterDescriptorKind.Call),
            CoflowRegisterOpCode.CallIndirect => Spec(descriptor: CoflowRegisterDescriptorKind.IndirectCall),
            CoflowRegisterOpCode.TailCall => Spec(descriptor: CoflowRegisterDescriptorKind.Call, controlFlow: CoflowRegisterControlFlow.Terminal),
            CoflowRegisterOpCode.TailCallIndirect => Spec(descriptor: CoflowRegisterDescriptorKind.IndirectCall, controlFlow: CoflowRegisterControlFlow.Terminal),
            CoflowRegisterOpCode.Return => Spec(descriptor: CoflowRegisterDescriptorKind.Target, controlFlow: CoflowRegisterControlFlow.Terminal),
            _ => throw new ArgumentOutOfRangeException(nameof(code), code, "Unknown register opcode."),
        };
    }

    private static CoflowRegisterOpSpec Spec(
        CoflowRegisterOperandSpec? a = null,
        CoflowRegisterOperandSpec? b = null,
        CoflowRegisterOperandSpec? c = null,
        CoflowRegisterDescriptorKind descriptor = CoflowRegisterDescriptorKind.None,
        CoflowRegisterControlFlow controlFlow = CoflowRegisterControlFlow.Next) =>
        new(a ?? None, b ?? None, c ?? None, descriptor, controlFlow);
}
}
