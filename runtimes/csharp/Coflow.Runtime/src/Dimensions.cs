using System;
namespace Coflow.Runtime
{
    public sealed class CoflowDimension<T> : IDisposable, ICoflowValue
    {
        private readonly CoflowValue value;
        private readonly Func<CoflowValue,T> read;
        public CoflowDimension(CoflowValue value, Func<CoflowValue,T> read) { this.value=value; this.read=read; }
        public T Default() => read(value.DimensionDefault());
        public T For(string variant) => read(value.DimensionValue(variant));
        public CoflowValue RetainValue() => value.Retain();
        public void Dispose() => value.Dispose();
    }
}
