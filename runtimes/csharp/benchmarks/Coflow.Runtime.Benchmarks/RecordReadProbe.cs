using System;
using System.Diagnostics;
using System.IO;
using System.Runtime.CompilerServices;
using System.Text;
using Coflow;
using Game.Config;

internal static class RecordReadProbe
{
    internal static void Run()
    {
        var timer = Stopwatch.StartNew();
        using var contract = Generated.LoadContract(File.ReadAllBytes(Path.Combine(AppContext.BaseDirectory, "coflow.contract")));
        Console.WriteLine($"contract_load_ms={timer.Elapsed.TotalMilliseconds:F3}");
        var source = new StringBuilder();
        for (int i = 0; i < 1000; ++i) source.Append($"h{i}: Hero {{ name: \"Hero {i}\", stats: Stats {{ health: {i + 1}, weights: [1, 2, 3] }} }}\n");
        source.Append("RuntimeSettings: RuntimeSettings {}");
        var text = source.ToString();
        Collect();
        long managedBefore = GC.GetTotalMemory(true), allocatedBefore = GC.GetTotalAllocatedBytes(true), privateBefore = PrivateBytes();
        var weak = Measure(contract, text);
        Collect();
        // 在创建线程触发一次原生请求，排空终结释放队列后再记录保留量。
        Native.ReadBuffer(Native.Call(NativeOperation.CreateBuffer, data: Array.Empty<byte>()));
        Console.WriteLine($"managed_retained_delta={GC.GetTotalMemory(true) - managedBefore},total_managed_allocated={GC.GetTotalAllocatedBytes(true) - allocatedBefore},process_private_delta={PrivateBytes() - privateBefore},runtime_collected={!weak.IsAlive}");
    }
    [MethodImpl(MethodImplOptions.NoInlining)]
    private static WeakReference Measure(Contract contract, string source)
    {
        long baseline = GC.GetTotalMemory(true), requests = Native.RequestCount;
        var timer = Stopwatch.StartNew();
        using var builder = new RuntimeBuilder(contract).AddSource(source);
        var runtime = builder.Build();
        var built = timer.Elapsed.TotalMilliseconds;
        long loaded = GC.GetTotalMemory(true);
        Console.WriteLine($"records=1000,build_ms={built:F3},managed_live_delta={loaded - baseline},build_ffi_requests={Native.RequestCount - requests},process_private_bytes={PrivateBytes()}");
        var hero = runtime.Table<Character>().Get("h0");
        requests = Native.RequestCount;
        // 预热属性和 JIT 后单独验证热读取不分配。
        _ = hero.name.Length + hero.stats.health + hero.stats.weights.Count;
        long readAllocated = GC.GetAllocatedBytesForCurrentThread();
        timer.Restart(); long sum = 0;
        for (int i = 0; i < 1_000_000; ++i) sum += hero.name.Length + hero.stats.health + hero.stats.weights.Count;
        if (sum != 10_000_000 || Native.RequestCount != requests) throw new Exception("Managed read oracle failed.");
        long readBytes = GC.GetAllocatedBytesForCurrentThread() - readAllocated;
        if (readBytes != 0) throw new Exception($"Managed reads allocated {readBytes} bytes.");
        Console.WriteLine($"local_read_allocated_bytes={readBytes},local_read_iterations=1000000,local_read_ms={timer.Elapsed.TotalMilliseconds:F3},local_read_ffi_requests={Native.RequestCount - requests},result={sum}");
        timer.Restart(); sum = 0;
        for (int i = 0; i < 10_000; ++i) sum += hero.score(1);
        if (sum != 20_000) throw new Exception("VM invocation oracle failed.");
        Console.WriteLine($"vm_call_iterations=10000,vm_call_ms={timer.Elapsed.TotalMilliseconds:F3},result={sum}");
        runtime.Dispose();
        if (hero.name != "Hero 0") throw new Exception("Disposed record read failed.");
        return new WeakReference(runtime);
    }
    private static long PrivateBytes() { using var process = Process.GetCurrentProcess(); process.Refresh(); return process.PrivateMemorySize64; }
    private static void Collect() { GC.Collect(); GC.WaitForPendingFinalizers(); GC.Collect(); }
}
