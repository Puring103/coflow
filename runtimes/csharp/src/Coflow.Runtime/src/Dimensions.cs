using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;
namespace Coflow
{
    public sealed class RuntimeDimension<T>
    {
        private readonly Projection projection;
        private readonly T defaultValue;
        private readonly IReadOnlyDictionary<string, T> variants;
        public RuntimeDimension(Projection value, Func<Projection, T> read)
        {
            projection = value;
            defaultValue = value.DimensionDefault().ReadProjected(read);
            var values = new Dictionary<string, T>(StringComparer.Ordinal);
            for (var index = 0; index < value.Count; index++) {
                var variant = value.DimensionVariantAt(index);
                values.Add(variant, value.DimensionValue(variant).ReadProjected(read));
            }
            variants = new ReadOnlyDictionary<string, T>(values);
        }
        public T Default() => defaultValue;
        public T For(string variant) => variants.TryGetValue(variant, out var value) ? value : defaultValue;
        public bool TryGetVariant(string variant, out T value)
        {
            if (!projection.HasDimensionVariant(variant)) { value = default!; return false; }
            value = For(variant); return true;
        }
        public IReadOnlyDictionary<string, T> Variants() => variants;
    }
}
