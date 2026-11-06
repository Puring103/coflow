# 最终源码的默认调度测量

五轮串行 AB/BA，各进程一次预热、三个测量样本；每项每版十五个样本，二十个进程全部成功。固定源码见 [setup.json](setup.json)、[after.patch](after.patch)、[after-source-overlay.zip](after-source-overlay.zip)，构建与二进制证据见 [build-after.log](build-after.log)、[artifacts.json](artifacts.json)。

本组使用 Windows 默认调度。前两轮基线多个核心 API 样本整体抬升，例如显式数组默认值分别约 106.7ms 与 102.0ms，后三轮明显降低；未将其归因于实现收益。核心 API 的主要结论采用同一二进制的 [固定 P 核结果](../controlled/README.md)。本组全部原始数据保留，无样本删除。

独立 CLI 两个场景的五轮中位数均更快；CLI 主结果使用本组，明确与固定核核心 API 的口径分开。

| 场景 | 基线中位数 [范围] ms | 最终中位数 [范围] ms | 中位数变化 | 更快轮数 |
| --- | ---: | ---: | ---: | ---: |
| CLI 同文件 1,000 项修改 | 2018.609 [1953.897, 2115.308] | 1894.329 [1833.826, 2026.099] | -6.2% | 5/5 |
| CLI 跨文件 1,000 项修改 | 2412.647 [2349.003, 2494.234] | 2253.967 [2106.887, 2404.815] | -6.6% | 5/5 |

[完整统计](summary.json)与[全部指标表](summary-table.md)保存核心和 CLI 的原始配对结果；`python summarize.py` 可复算。逐进程 `r*-*.meta.json` 记录命令、环境变量、固定二进制摘要、开始和结束时间及退出码；stdout、stderr 和 CLI results JSON 按原名保存。
