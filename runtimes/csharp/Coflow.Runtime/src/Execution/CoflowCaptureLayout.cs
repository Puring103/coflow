namespace Coflow.Runtime.CompilerServices;

internal readonly record struct CoflowCaptureLayout(CoflowValueShape Shape, int IntegerBase, int FloatBase, int ReferenceBase);
