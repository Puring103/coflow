# 编写 CFD

CFD 是唯一数据格式。顶层记录使用声明类型和 key，字段值遵循 CFT 类型：

```cfd
sword: Item {
  name: "Fire Sword",
  tags: ["weapon", "fire"],
}
```

编辑 `.cfd` 后运行 `coflow check <project>`。需要更新绑定时运行 `coflow codegen <project>`。不要把生成目录当作数据输入。
