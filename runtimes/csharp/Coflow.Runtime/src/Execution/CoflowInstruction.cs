using System;

namespace Coflow.Runtime.CompilerServices;

internal readonly record struct CoflowInstruction(CoflowOpCode Code, int Operand = 0, Type? ValueType = null);
