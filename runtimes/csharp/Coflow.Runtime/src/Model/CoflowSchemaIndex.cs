using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System.Collections.Generic;
using System;
namespace Coflow.Runtime
{

using global::Coflow.Runtime.CompilerServices;

/// <summary>快照构建和执行共享的 Schema 索引，避免按值和按调用重复扫描元数据。</summary>
internal sealed class CoflowSchemaIndex
{
    private readonly HashSet<long> _assignableTypes;

    internal CoflowSchemaIndex(ICoflowSchema schema)
    {
        ByName = schema.Types.ToDictionary(value => value.DeclaredType, StringComparer.Ordinal);
        ById = schema.Types.ToDictionary(value => value.TypeId);
        ByRuntimeType = schema.Types.ToDictionary(value => value.RuntimeType);
        EnumsByRuntimeType = schema.Enums.ToDictionary(value => value.RuntimeType);
        FieldSlots = schema.Types.SelectMany(metadata => metadata.Fields.Select(
                (field, index) => new KeyValuePair<(CoflowTypeId, string), int>(
                    (metadata.TypeId, field.Name), index)))
            .ToDictionary(value => value.Key, value => value.Value);
        _assignableTypes = schema.Types.SelectMany(metadata => metadata.AssignableTypeIds,
                static (metadata, target) => TypePair(metadata.TypeId, target))
            .ToHashSet();
    }

    internal IReadOnlyDictionary<string, ICoflowTypeMetadata> ByName { get; }
    internal IReadOnlyDictionary<CoflowTypeId, ICoflowTypeMetadata> ById { get; }
    internal IReadOnlyDictionary<Type, ICoflowTypeMetadata> ByRuntimeType { get; }
    internal IReadOnlyDictionary<Type, ICoflowEnumMetadata> EnumsByRuntimeType { get; }
    internal IReadOnlyDictionary<(CoflowTypeId TypeId, string FieldName), int> FieldSlots { get; }

    internal (CoflowTypeId TypeId, int FieldId) FunctionSlot(string declaredType, string fieldName)
    {
        var typeId = ByName[declaredType].TypeId;
        return (typeId, FieldSlots[(typeId, fieldName)]);
    }

    internal bool IsAssignable(CoflowTypeId concrete, CoflowTypeId target) =>
        _assignableTypes.Contains(TypePair(concrete, target));

    private static long TypePair(CoflowTypeId concrete, CoflowTypeId target) =>
        ((long)concrete.Value << 32) | (uint)target.Value;
}
}
