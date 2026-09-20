-- Coflow VM 性能对比基准：与 coflow-core/tests/vm_perf_probe.rs 中负载一一对应
-- Windows os.clock 分辨率低（~15ms），每个负载重复运行到累计 >100ms 再取平均。
local function bench(name, f)
  local expected = f()
  local runs = 1
  local elapsed, result = 0, nil
  repeat
    runs = runs * 2
    local start = os.clock()
    for _ = 1, runs do result = f() end
    elapsed = os.clock() - start
    assert(result == expected, "benchmark result changed")
  until elapsed > 0.1 or runs >= 1024
  local samples = {}
  for sample = 1, 7 do
    local start = os.clock()
    for _ = 1, runs do result = f() end
    samples[sample] = (os.clock() - start) * 1000 / runs
    assert(result == expected, "benchmark result changed")
  end
  table.sort(samples)
  local raw = {}
  for i, value in ipairs(samples) do raw[i] = string.format("%.6f", value) end
  print(string.format("%s: median_ms=%.6f, runs=%d, samples_ms=[%s], result=%s", name, samples[4], runs, table.concat(raw, ","), tostring(expected)))
end

bench("int-loop 1_000_000", function()
  local total = 0
  for value = 0, 999999 do total = total + 1 end
  return total
end)

bench("int-eq 200_000", function()
  local hits = 0
  for value = 0, 199999 do
    if value == 123456 then hits = hits + 1 end
  end
  return hits
end)

local function map(values, callback)
  local output = {}
  for index, value in ipairs(values) do output[index] = callback(value) end
  return output
end
bench("map-10x10000 calls", function()
  local base = {0,1,2,3,4,5,6,7,8,9}
  local total = 0
  for i = 1, 10000 do
    local values = map(base, function(value) return value + 1 end)
    for _, value in ipairs(values) do total = total + value end
  end
  return total
end)

bench("map-hand-fused-10x10000", function()
  local base = {0,1,2,3,4,5,6,7,8,9}
  local total = 0
  for i = 1, 10000 do
    local sum = 0
    for x = 1, 10 do sum = sum + base[x] + 1 end
    total = total + sum
  end
  return total
end)

bench("string-concat 20_000", function()
  local text = ""
  for value = 1, 20000 do text = text .. "x" end
  return #text
end)

bench("string-index 1_000 over 2_000 chars", function()
  local text = ""
  for value = 1, 2000 do text = text .. "y" end
  local total = 0
  for value = 1, 1000 do
    local c = text:sub(1, 1)
    total = total + #c
  end
  return total
end)

bench("field-read self.value x10000", function()
  local obj = { value = 7 }
  local total = 0
  for value = 1, 10000 do total = total + obj.value end
  return total
end)

bench("closure-create 200_000", function()
  local total = 0
  for i = 0, 199999 do
    local f = function() return i end
    f()
    total = total + 1
  end
  return total
end)

bench("closure-call-hoisted 200_000", function()
  local f = function(x) return x + 1 end
  local total = 0
  for i = 0, 199999 do total = f(i) end
  return total
end)

bench("fib-self-recursion fib(20)", function()
  local obj = {}
  function obj.fib(n)
    if n < 2 then return n end
    return obj.fib(n - 1) + obj.fib(n - 2)
  end
  return obj.fib(20)
end)

bench("array-iter 10x10_000", function()
  local values = {1,2,3,4,5,6,7,8,9,0}
  local total = 0
  for w = 1, 10000 do
    for _, v in ipairs(values) do total = total + v end
  end
  return total
end)

bench("array-2bind 10x10_000", function()
  local values = {1,2,3,4,5,6,7,8,9,0}
  local total = 0
  for w = 1, 10000 do
    -- Lua 数组索引从 1 开始；对照 Coflow 的从 0 开始索引。
    for index, v in ipairs(values) do total = total + (index - 1) + v end
  end
  return total
end)

bench("array-build 10_000x5", function()
  local total = 0
  for w = 1, 10000 do
    local list = {1, 2, 3, 4, 5}
    total = total + list[5]
  end
  return total
end)

local dict = {}
for k = 0, 99 do dict[k] = k end
bench("dict-int-lookup 100_000", function()
  local total = 0
  for w = 0, 99999 do total = total + dict[w % 100] end
  return total
end)

bench("template-1_000", function()
  local total = 0
  for i = 1, 1000 do total = total + #string.format("x%dy", 7) end
  return total
end)

bench("record-ref-field 100_000", function()
  local rule = { value = 7 }
  local total = 0
  for w = 1, 100000 do total = total + rule.value end
  return total
end)
