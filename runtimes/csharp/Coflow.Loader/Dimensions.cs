using System.Collections.Generic;

namespace Coflow.Loader
{
    /// <summary>已加载的维度字段：默认值及各变体的有效值。</summary>
    public sealed class DimensionValue<T>
    {
        private readonly IReadOnlyDictionary<string, T> _variants;

        internal DimensionValue(T defaultValue, IReadOnlyDictionary<string, T> variants)
        {
            Default = defaultValue;
            _variants = variants;
        }

        public T Default { get; }

        // 覆盖已在加载阶段合成；选择未知变体时直接使用基础值。
        public T For(string variant) => _variants.TryGetValue(variant, out var value) ? value : Default;

        public IReadOnlyDictionary<string, T> Variants => _variants;
    }
}
