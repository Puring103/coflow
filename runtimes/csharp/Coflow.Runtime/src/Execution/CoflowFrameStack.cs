using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System.Collections.Generic;
using System;
namespace Coflow.Runtime.CompilerServices
{

using System.Buffers;

internal struct CoflowFrame
{
    internal CoflowProgram Program;
    internal int ReturnPc;
    internal int IntegerBase;
    internal int FloatBase;
    internal int ReferenceBase;
    internal CoflowValueRegister ReturnTarget;
}

/// <summary>唯一拥有调用帧缓冲区、深度和池化清理协议。</summary>
internal sealed class CoflowFrameStack
{
    private CoflowFrame[] _frames = RentCleared(16);
    private readonly int _baselineCapacity;
    private int _count;
    private int _highWater;

    internal CoflowFrameStack() => _baselineCapacity = _frames.Length;

    internal int Count => _count;

    internal IEnumerable<CoflowFunctionIdentity> CallStack(CoflowProgram current) =>
        _frames.Take(_count).Select(frame => frame.Program.Identity)
            .Append(current.Identity).Reverse();

    internal void Push(CoflowFrame frame)
    {
        Ensure(_count + 1);
        _frames[_count++] = frame;
        _highWater = Math.Max(_highWater, _count);
    }

    internal CoflowFrame Pop()
    {
        if (_count == 0) throw new InvalidOperationException("The Coflow frame stack is empty.");
        var index = --_count;
        var frame = _frames[index];
        _frames[index] = default;
        return frame;
    }

    internal void Reset()
    {
        _count = 0;
        _highWater = 0;
    }

    internal void ClearAndTrim()
    {
        if (_highWater != 0) Array.Clear(_frames, 0, _highWater);
        _count = 0;
        _highWater = 0;
        if (_frames.Length <= _baselineCapacity) return;
        var expanded = _frames;
        _frames = RentCleared(_baselineCapacity);
        ArrayPool<CoflowFrame>.Shared.Return(expanded);
    }

    private void Ensure(int count)
    {
        if (count <= _frames.Length) return;
        var expanded = RentCleared(Math.Max(count, checked(_frames.Length * 2)));
        Array.Copy(_frames, expanded, _frames.Length);
        Array.Clear(_frames, 0, _frames.Length);
        ArrayPool<CoflowFrame>.Shared.Return(_frames);
        _frames = expanded;
    }

    private static CoflowFrame[] RentCleared(int count)
    {
        var frames = ArrayPool<CoflowFrame>.Shared.Rent(count);
        Array.Clear(frames, 0, frames.Length);
        return frames;
    }
}
}
