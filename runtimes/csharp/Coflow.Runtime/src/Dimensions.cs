using System;
using System.Collections.Generic;
namespace Coflow
{
    public sealed class RuntimeDimension<T> : IRuntimeValue
    {
        public RuntimeValue RuntimeValue { get; }
        private readonly Func<RuntimeValue, T> read;
        public RuntimeDimension(RuntimeValue value, Func<RuntimeValue, T> read) { RuntimeValue = value; this.read = read; }
        public T Default() => read(RuntimeValue.DimensionDefault());
        public T For(string variant) => read(RuntimeValue.DimensionValue(variant));
        public IReadOnlyDictionary<string, T> Variants()
        {
            var values = new Dictionary<string, T>(StringComparer.Ordinal);
            for (var index = 0; index < RuntimeValue.Count; index++)
            {
                var variant = RuntimeValue.DimensionVariantAt(index);
                values.Add(variant, For(variant));
            }
            return values;
        }
    }
}
