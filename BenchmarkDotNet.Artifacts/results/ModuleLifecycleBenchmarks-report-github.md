```

BenchmarkDotNet v0.14.0, Linux Mint 22.3 (Zena)
13th Gen Intel Core i5-13600KF, 1 CPU, 20 logical and 14 physical cores
.NET SDK 10.0.102
  [Host]     : .NET 10.0.2 (10.0.225.61305), X64 RyuJIT AVX2
  DefaultJob : .NET 10.0.2 (10.0.225.61305), X64 RyuJIT AVX2


```
| Method         | Mean     | Error    | StdDev   | Min      | Max      | Median   | Gen0     | Allocated |
|--------------- |---------:|---------:|---------:|---------:|---------:|---------:|---------:|----------:|
| LoadAndCompile | 66.22 ms | 0.068 ms | 0.063 ms | 66.08 ms | 66.31 ms | 66.23 ms | 142.8571 |   1.87 MB |
