using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;

namespace Coflow.Loader
{
// 深依赖按固定长度的原生调用段执行。暂停帧保留 DFS 的正在构造身份与字段循环检测。
internal sealed class ConstructionSuspension : Exception
{
    internal ConstructionSuspension(ConstructionTarget target, int depth) { Target = target; Depth = depth; }
    internal ConstructionTarget Target { get; }
    internal int Depth { get; }
    internal List<RecordState> Records { get; } = new List<RecordState>();
    internal List<(CfdReader Reader, int Slot)> Fields { get; } = new List<(CfdReader, int)>();
    // 返回 false 让异常继续寻找唯一调度器；第一遍筛选仍按内到外的 DFS 顺序记录。
    // 此处只登记身份，不读取尚未执行 finally 的深度/活动槽位；逻辑深度取抛出时快照。
    internal bool CaptureRecord(RecordState record, ref bool suspended)
    {
        Records.Add(record);
        suspended = true;
        return false;
    }
    internal bool CaptureField(CfdReader reader, int slot, ref bool suspended)
    {
        Fields.Add((reader, slot));
        suspended = true;
        return false;
    }
    internal void Resume()
    {
        foreach (var field in Fields) field.Reader.Resume(field.Slot);
        foreach (var record in Records) record.Resume();
    }
}

internal readonly struct ConstructionTarget
{
    private readonly RecordState? _record;
    private readonly CfdReader? _reader;
    private readonly int _slot;
    internal ConstructionTarget(RecordState record) { _record = record; _reader = null; _slot = 0; }
    internal ConstructionTarget(CfdReader reader, int slot) { _record = null; _reader = reader; _slot = slot; }
    internal void Run()
    {
        if (_record != null) _record.BuildScheduled();
        else _reader!.Read(_slot);
    }
}

internal sealed partial class LoadContext
{
    private const int ConstructionSegmentDepth = 32;
    private int _segmentDepth;

    internal void BuildScheduled(RecordState record)
    {
        if (record.Available) return;
        var initialDepth = _depth;
        var initialSegmentDepth = _segmentDepth;
        Stack<(RecordState Record, int Slot)>? pending = null;
        var current = record;
        var slot = 0;
        var suspended = false;
        current.Begin();
        try
        {
            while (true)
            {
                var dependency = current.Reader.ValidateScheduled(ref slot);
                if (dependency != null)
                {
                    // 普通记录引用在当前字段处切到显式 DFS 栈；父字段与记录继续保持正在求值身份。
                    if (pending == null) pending = new Stack<(RecordState, int)>();
                    pending.Push((current, slot));
                    current = dependency;
                    slot = 0;
                    current.Begin();
                    // 逻辑字段深度仍累计；这个边没有增加原生调用栈，不能占用复杂读取的 32 层段预算。
                    _segmentDepth++;
                    continue;
                }
                current.Complete();
                if (pending == null || pending.Count == 0) break;
                var parent = pending.Pop();
                current = parent.Record;
                slot = parent.Slot;
                current.Reader.ResumeScheduled(slot);
                _segmentDepth--;
                // 子记录已经完整构造；按原字段读取包装 Option 等结果，再继续下一个字段。
                current.Reader.Read(slot++);
            }
        }
        catch (ConstructionSuspension suspension) when (CaptureScheduled(suspension, current, pending, ref suspended)) { throw; }
        finally
        {
            if (!suspended)
            {
                current.Resume();
                if (pending != null)
                    foreach (var parent in pending)
                    {
                        parent.Record.Reader.Resume(parent.Slot);
                        parent.Record.Resume();
                    }
            }
            _depth = initialDepth;
            _segmentDepth = initialSegmentDepth;
        }
    }

    private static bool CaptureScheduled(ConstructionSuspension suspension, RecordState current,
        Stack<(RecordState Record, int Slot)>? pending, ref bool suspended)
    {
        // 复杂模板或集合仍可暂停；把显式父帧加入同一个恢复协议，顺序与真实递归栈一致。
        suspension.CaptureRecord(current, ref suspended);
        if (pending != null)
            foreach (var parent in pending)
            {
                suspension.CaptureField(parent.Record.Reader, parent.Slot, ref suspended);
                suspension.CaptureRecord(parent.Record, ref suspended);
            }
        return false;
    }

    internal void CheckConstructionStack(CfdReader reader, int slot)
    {
        if (_depth - _segmentDepth >= ConstructionSegmentDepth)
            throw new ConstructionSuspension(new ConstructionTarget(reader, slot), _depth);
    }

    private object RequireRecord(RecordState record, DimensionSelection selection)
    {
        var projected = InView(record, selection);
        if (!projected.Available)
        {
            if (_depth - _segmentDepth >= ConstructionSegmentDepth)
                throw new ConstructionSuspension(new ConstructionTarget(projected), _depth);
            projected.Build();
        }
        return projected.Value;
    }

    private void BuildSelection(RecordState record)
    {
        var depth = _depth;
        var segmentDepth = _segmentDepth;
        _segmentDepth = depth;
        try
        {
            // 普通引用使用记录栈；复杂读取仅到调用段边界才分配异常恢复栈。
            try { record.BuildScheduled(); }
            catch (ConstructionSuspension suspension)
            {
                ContinueConstruction(new ConstructionTarget(record), depth, suspension);
            }
        }
        finally { _depth = depth; _segmentDepth = segmentDepth; }
    }

    private void ContinueConstruction(ConstructionTarget root, int rootDepth, ConstructionSuspension first)
    {
        var pending = new Stack<(ConstructionTarget Target, int Depth, ConstructionSuspension Suspension)>();
        pending.Push((root, rootDepth, first));
        var target = first.Target;
        var depth = first.Depth;
        try
        {
            while (true)
            {
                // 恢复逻辑深度而非重置预算，256 层限制与原来的 DFS 诊断顺序保持一致。
                _depth = depth;
                _segmentDepth = depth;
                try { target.Run(); }
                catch (ConstructionSuspension suspension)
                {
                    pending.Push((target, depth, suspension));
                    target = suspension.Target;
                    depth = suspension.Depth;
                    continue;
                }
                if (pending.Count == 0) break;
                var parent = pending.Pop();
                parent.Suspension.Resume();
                target = parent.Target;
                depth = parent.Depth;
            }
        }
        finally
        {
            // 失败时同时清理暂停中的循环标志；外层 Select 统一逐出该组合的所有视图。
            foreach (var parent in pending) parent.Suspension.Resume();
        }
    }
}

public sealed partial class CfdReader
{
    private int _activeSlot;
    private Dictionary<(int Slot, CfdValueNode Node), object>? _partialValues;
    internal void Resume(int slot) { if (_states[slot] == 1) _states[slot] = 0; }
    internal void BuildScheduled(RecordState record) => _context.BuildScheduled(record);
    internal void ResumeScheduled(int slot) { Resume(slot); _context.ExitRead(); }

    internal RecordState? ValidateScheduled(ref int slot)
    {
        if (!_type.HasReferenceDependencies) { Validate(); return null; }
        for (; slot < _type.Fields.Count; slot++)
        {
            var descriptor = _type.Fields[slot];
            if (_states[slot] == 0 && descriptor.Type.HasReferenceDependency)
            {
                // 先按普通 Read 的顺序检查深度与字段状态，保证第 257 层及回读父字段的首错不变。
                _context.CheckConstructionStack(this, slot);
                _context.EnterRead(Path, _span);
                _states[slot] = 1;
                var pending = false;
                try
                {
                    var node = SourceNode(slot)
                        ?? throw LoadContext.Error("CFD-MISSING", $"missing required field `{descriptor.Name}`", Path, _span);
                    var dependency = descriptor.Type.ReferenceDependency(_context, node, this);
                    if (dependency != null && !dependency.Available)
                    {
                        pending = true;
                        return dependency;
                    }
                }
                finally
                {
                    if (!pending) ResumeScheduled(slot);
                }
            }
            Read(slot);
        }
        return null;
    }

    private sealed class ListProgress<T>
    {
        internal ListProgress(T[] values, int index) { Values = values; Index = index; }
        internal T[] Values { get; }
        internal int Index { get; }
    }

    internal IReadOnlyList<T> ReadList<T>(CfdValueType<T> type, CfdValueNode node)
    {
        if (!(node is CfdArrayValue array)) throw Error(node, "CFD-TYPE", "expected array");
        var key = (_activeSlot, node);
        var progress = _partialValues != null && _partialValues.TryGetValue(key, out var saved) ? (ListProgress<T>)saved : null;
        var values = progress?.Values ?? new T[array.Items.Count];
        var index = progress?.Index ?? 0;
        try
        {
            for (; index < values.Length; index++) values[index] = (T)type.Read(_context, array.Items[index], this)!;
            _partialValues?.Remove(key);
            return System.Array.AsReadOnly(values);
        }
        catch (ConstructionSuspension)
        {
            // 只在暂停时保留游标，避免重新构造已完成的内联对象或反复扫描集合前缀。
            if (_partialValues == null) _partialValues = new Dictionary<(int, CfdValueNode), object>();
            _partialValues[key] = new ListProgress<T>(values, index);
            throw;
        }
        catch { _partialValues?.Remove(key); throw; }
    }

    private sealed class DictionaryProgress<K, V> where K : notnull
    {
        internal DictionaryProgress(Dictionary<K, V> values, int index, K key, bool hasKey)
        { Values = values; Index = index; Key = key; HasKey = hasKey; }
        internal Dictionary<K, V> Values { get; }
        internal int Index { get; }
        internal K Key { get; }
        internal bool HasKey { get; }
    }

    internal IReadOnlyDictionary<K, V> ReadDictionary<K, V>(CfdValueType<K> keyType, CfdValueType<V> valueType, CfdValueNode node) where K : notnull
    {
        if (!(node is CfdDictionaryValue dictionary)) throw Error(node, "CFD-TYPE", "expected dictionary");
        var progressKey = (_activeSlot, node);
        var progress = _partialValues != null && _partialValues.TryGetValue(progressKey, out var saved) ? (DictionaryProgress<K, V>)saved : null;
        var values = progress?.Values ?? new Dictionary<K, V>(dictionary.Entries.Count);
        var index = progress?.Index ?? 0;
        var key = progress == null ? default! : progress.Key;
        var hasKey = progress?.HasKey ?? false;
        try
        {
            for (; index < dictionary.Entries.Count; index++)
            {
                var entry = dictionary.Entries[index];
                if (!hasKey)
                {
                    key = (K)keyType.Read(_context, entry.Key, this)!;
                    if (values.ContainsKey(key)) throw Error(entry.Key, "CFD-DUPLICATE", "duplicate dictionary key");
                    hasKey = true;
                }
                values.Add(key, (V)valueType.Read(_context, entry.Value, this)!);
                hasKey = false;
            }
            _partialValues?.Remove(progressKey);
            return new ReadOnlyDictionary<K, V>(values);
        }
        catch (ConstructionSuspension)
        {
            if (_partialValues == null) _partialValues = new Dictionary<(int, CfdValueNode), object>();
            _partialValues[progressKey] = new DictionaryProgress<K, V>(values, index, key, hasKey);
            throw;
        }
        catch { _partialValues?.Remove(progressKey); throw; }
    }
}
}
