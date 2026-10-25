# 单元格设计预览

从仓库根目录启动前端开发服务：

```powershell
npm --prefix editors/cfd-editor/frontend run dev -- --host 127.0.0.1 --port 5174
```

打开 <http://127.0.0.1:5174/previews/cells.html>。

页面直接使用 `TableView`、`EditableCell`、`DataCardCompact`、`InspectorPanel`、`ScalarValueInput` 和现有引用、枚举选择器。预览页面仅提供示例数据、字段路径写入和页面布局。

示例覆盖简单值、长文本、对象、对象数组、对象字典、多层混合嵌套、空态、字段只读状态与多态类型切换。支持深浅主题、有限预览、原位编辑和引用查看。双击记录 ID 打开整条记录，复杂单元格双击空白处或点击左上角放大按钮编辑当前字段。拖动行底边调整高度，双击适配内容，右键恢复默认。数据修改保存在页面内存中，可以重置；示例行高独立保存在当前浏览器，桌面端行高写入用户目录中的项目设置。

验证预览：

```powershell
npm --prefix editors/cfd-editor/frontend exec -- tsc -p editors/cfd-editor/frontend/previews/tsconfig.json
node --test editors/cfd-editor/frontend/tests/browser/cells.browser.mjs
```

预览直接使用正式编辑器的结构摘要和样式。摘要只在折叠时显示，多态实际类型始终位于右侧值区域，有摘要时排列在摘要最前面；只读字段保留类型文字，可编辑字段支持切换类型。
