# 单元格设计预览

从仓库根目录启动前端开发服务：

```powershell
npm --prefix editors/cfd-editor/frontend run dev -- --host 127.0.0.1 --port 5174
```

打开 <http://127.0.0.1:5174/previews/cells.html>。

页面直接使用 `TableView` 的 `EditableCell`、`DataCardCompact` 字段树、`ScalarValueInput` 和现有引用、枚举选择器。预览页面仅提供示例数据、字段路径写入和页面布局。

示例覆盖简单值、长文本、对象、对象数组、对象字典、多层混合嵌套、空态与字段只读状态。支持深浅主题、全部展开与收起、原位编辑和引用查看。修改保存在页面内存中，可以重置。

验证预览：

```powershell
npm --prefix editors/cfd-editor/frontend exec -- tsc -p editors/cfd-editor/frontend/previews/tsconfig.json
node --test editors/cfd-editor/frontend/tests/browser/cells.browser.mjs
```
