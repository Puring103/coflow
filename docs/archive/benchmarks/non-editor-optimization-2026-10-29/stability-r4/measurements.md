# 原始样本汇总

耗时单位为毫秒。每次样本包含的操作数量见 `每样本操作数`；批量查询的单操作均值不能当作单次调用尾延迟。少于 20 个样本不报告 P95，少于 100 个不报告 P99；已列分位数也仅描述本次样本。

| 运行 | 指标 | 样本数 | 中位数 | 最小值 | 最大值 | P95 | 每样本操作数 |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| rust-main-100k-r1-baseline | load_checks | 1 | 4668.779 | 4668.779 | 4668.779 | — | 1 |
| rust-main-100k-r1-baseline | load_no_checks | 1 | 2454.045 | 2454.045 | 2454.045 | — | 1 |
| rust-main-100k-r1-baseline | set_scalar | 1 | 27.484 | 27.484 | 27.484 | — | 1 |
| rust-main-100k-r1-baseline | batch_grouped | 1 | 67.850 | 67.850 | 67.850 | — | 1000 |
| rust-main-100k-r1-baseline | batch_interleaved | 1 | 2667.747 | 2667.747 | 2667.747 | — | 1000 |
| rust-main-100k-r1-optimized | load_checks | 1 | 4598.147 | 4598.147 | 4598.147 | — | 1 |
| rust-main-100k-r1-optimized | load_no_checks | 1 | 2457.266 | 2457.266 | 2457.266 | — | 1 |
| rust-main-100k-r1-optimized | set_scalar | 1 | 30.132 | 30.132 | 30.132 | — | 1 |
| rust-main-100k-r1-optimized | batch_grouped | 1 | 69.486 | 69.486 | 69.486 | — | 1000 |
| rust-main-100k-r1-optimized | batch_interleaved | 1 | 2647.290 | 2647.290 | 2647.290 | — | 1000 |
| rust-main-100k-r2-baseline | load_checks | 1 | 4618.710 | 4618.710 | 4618.710 | — | 1 |
| rust-main-100k-r2-baseline | load_no_checks | 1 | 2529.333 | 2529.333 | 2529.333 | — | 1 |
| rust-main-100k-r2-baseline | set_scalar | 1 | 27.615 | 27.615 | 27.615 | — | 1 |
| rust-main-100k-r2-baseline | batch_grouped | 1 | 66.717 | 66.717 | 66.717 | — | 1000 |
| rust-main-100k-r2-baseline | batch_interleaved | 1 | 2642.832 | 2642.832 | 2642.832 | — | 1000 |
| rust-main-100k-r2-optimized | load_checks | 1 | 4638.077 | 4638.077 | 4638.077 | — | 1 |
| rust-main-100k-r2-optimized | load_no_checks | 1 | 2444.548 | 2444.548 | 2444.548 | — | 1 |
| rust-main-100k-r2-optimized | set_scalar | 1 | 29.150 | 29.150 | 29.150 | — | 1 |
| rust-main-100k-r2-optimized | batch_grouped | 1 | 68.970 | 68.970 | 68.970 | — | 1000 |
| rust-main-100k-r2-optimized | batch_interleaved | 1 | 2669.877 | 2669.877 | 2669.877 | — | 1000 |
| rust-main-100k-r3-baseline | load_checks | 1 | 4707.819 | 4707.819 | 4707.819 | — | 1 |
| rust-main-100k-r3-baseline | load_no_checks | 1 | 2471.463 | 2471.463 | 2471.463 | — | 1 |
| rust-main-100k-r3-baseline | set_scalar | 1 | 27.519 | 27.519 | 27.519 | — | 1 |
| rust-main-100k-r3-baseline | batch_grouped | 1 | 66.913 | 66.913 | 66.913 | — | 1000 |
| rust-main-100k-r3-baseline | batch_interleaved | 1 | 2630.356 | 2630.356 | 2630.356 | — | 1000 |
| rust-main-100k-r3-optimized | load_checks | 1 | 4654.653 | 4654.653 | 4654.653 | — | 1 |
| rust-main-100k-r3-optimized | load_no_checks | 1 | 2450.878 | 2450.878 | 2450.878 | — | 1 |
| rust-main-100k-r3-optimized | set_scalar | 1 | 30.036 | 30.036 | 30.036 | — | 1 |
| rust-main-100k-r3-optimized | batch_grouped | 1 | 66.954 | 66.954 | 66.954 | — | 1000 |
| rust-main-100k-r3-optimized | batch_interleaved | 1 | 2682.748 | 2682.748 | 2682.748 | — | 1000 |
