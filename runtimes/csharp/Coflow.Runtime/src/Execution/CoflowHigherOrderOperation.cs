using System;

namespace Coflow.Runtime.CompilerServices;

internal readonly record struct CoflowHigherOrderOperation(string Name, Type ElementType, Type OutputElementType, Type ResultType);
