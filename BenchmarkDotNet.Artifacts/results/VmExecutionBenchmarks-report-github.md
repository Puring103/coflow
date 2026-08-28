```

BenchmarkDotNet v0.14.0, Linux Mint 22.3 (Zena)
13th Gen Intel Core i5-13600KF, 1 CPU, 20 logical and 14 physical cores
.NET SDK 10.0.102
  [Host]     : .NET 10.0.2 (10.0.225.61305), X64 RyuJIT AVX2
  DefaultJob : .NET 10.0.2 (10.0.225.61305), X64 RyuJIT AVX2


```
| Method             | Mean     | Error   | StdDev  | Min      | Max      | Median   | Allocated |
|------------------- |---------:|--------:|--------:|---------:|---------:|---------:|----------:|
| GeneratedFieldRead | 113.0 μs | 0.14 μs | 0.12 μs | 112.8 μs | 113.3 μs | 113.0 μs |         - |
