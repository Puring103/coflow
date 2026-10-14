# 错误码索引

错误码用于识别诊断类别。源码位置、记录坐标和字段路径由诊断结构提供。

使用 `coflow explain <code> --json` 查询含义、触发条件、修复方向与进一步查询入口；`check --explain` 和 `schema check --explain` 返回当前诊断的解释。

```bash
coflow explain CFT-SCHEMA-006 --json
coflow explain REF-001 --json
```

## CFT 声明与检查表达式

| 错误码 | 含义 |
| --- | --- |
| `CFT-LEX-001` | 非法字符 |
| `CFT-LEX-002` | 字符串转义无效 |
| `CFT-LEX-003` | 字符串未结束 |
| `CFT-LEX-004` | 整数字面量无效 |
| `CFT-LEX-005` | 浮点数字面量无效 |
| `CFT-SYN-001` | 意外的语法单元 |
| `CFT-SYN-002` | 源码提前结束 |
| `CFT-SYN-003` | 缺少标识符 |
| `CFT-SYN-004` | 缺少指定符号 |
| `CFT-SYN-005` | 顶层声明无效 |
| `CFT-SYN-006` | 链式比较无效 |
| `CFT-SYN-007` | check 块位置无效 |
| `CFT-SYN-008` | 注解语法无效 |
| `CFT-SYN-009` | 检查语句无效 |
| `CFT-SYN-010` | 检查块重复 |
| `CFT-SYN-011` | 语法结构超出限制 |
| `CFT-SCHEMA-001` | 模块重复 |
| `CFT-SCHEMA-002` | 全局名称重复 |
| `CFT-SCHEMA-003` | 字段名称重复 |
| `CFT-SCHEMA-004` | 枚举成员重复 |
| `CFT-SCHEMA-005` | 枚举数值重复 |
| `CFT-SCHEMA-006` | 类型未声明 |
| `CFT-SCHEMA-007` | 继承目标不是类型 |
| `CFT-SCHEMA-008` | 常量未声明 |
| `CFT-SCHEMA-009` | 类型继承成环 |
| `CFT-SCHEMA-010` | 继承 sealed 类型 |
| `CFT-SCHEMA-011` | 继承字段重名 |
| `CFT-SCHEMA-012` | 类型修饰符冲突 |
| `CFT-SCHEMA-014` | 字典键类型无效 |
| `CFT-SCHEMA-015` | 默认值表达式无效 |
| `CFT-SCHEMA-016` | 默认值类型不匹配 |
| `CFT-SCHEMA-017` | 默认值引用字段 |
| `CFT-SCHEMA-018` | 枚举数值序列无效 |
| `CFT-SCHEMA-019` | flag 枚举数值无效 |
| `CFT-SCHEMA-020` | 注解未定义 |
| `CFT-SCHEMA-021` | 注解重复 |
| `CFT-SCHEMA-022` | 注解没有目标 |
| `CFT-SCHEMA-023` | 注解目标无效 |
| `CFT-SCHEMA-024` | 注解参数无效 |
| `CFT-SCHEMA-025` | 注解字段类型无效 |
| `CFT-SCHEMA-026` | struct 缺少 sealed |
| `CFT-SCHEMA-027` | idAsEnum 目标枚举非空 |
| `CFT-SCHEMA-028` | 枚举成员目标错误 |
| `CFT-SCHEMA-029` | 枚举成员不存在 |
| `CFT-SCHEMA-030` | 常量值无效 |
| `CFT-SCHEMA-031` | 使用了保留标识符 |
| `CFT-SCHEMA-034` | localized 目标无效 |
| `CFT-SCHEMA-035` | 本地化名称无效 |
| `CFT-SCHEMA-032` | dimension 目标无效 |
| `CFT-SCHEMA-033` | 维度名称无效 |
| `CFT-SCHEMA-036` | 抽象类型不能 singleton |
| `CFT-SCHEMA-037` | singleton 与 idAsEnum 冲突 |
| `CFT-SCHEMA-038` | Schema 结构超出限制 |
| `CFT-SCHEMA-039` | 项目级检查重名 |
| `CFT-SCHEMA-040` | 类型别名成环 |
| `CFT-SCHEMA-042` | 必填对象依赖成环 |
| `CFT-SCHEMA-043` | 默认值物化成环 |
| `CFT-TYPE-001` | 值名称未定义 |
| `CFT-TYPE-002` | 字段未定义 |
| `CFT-TYPE-003` | 检查中的枚举成员不存在 |
| `CFT-TYPE-004` | 检查中的枚举目标错误 |
| `CFT-TYPE-005` | 运算符类型不匹配 |
| `CFT-TYPE-006` | 比较类型不匹配 |
| `CFT-TYPE-007` | 检查条件不是 bool |
| `CFT-TYPE-008` | 检查函数未定义 |
| `CFT-TYPE-009` | 函数参数数量错误 |
| `CFT-TYPE-010` | 函数参数类型错误 |
| `CFT-TYPE-011` | 非对象不能访问字段 |
| `CFT-TYPE-012` | 目标不能使用下标 |
| `CFT-TYPE-013` | 下标类型不匹配 |
| `CFT-TYPE-014` | 类型判断无效 |
| `CFT-TYPE-015` | 量词目标不是集合 |
| `CFT-TYPE-016` | unique 元素类型无效 |
| `CFT-TYPE-017` | 位运算类型无效 |
| `CFT-TYPE-018` | 位移要求整数 |
| `CFT-TYPE-019` | 正则模式不是字面量 |
| `CFT-TYPE-020` | 正则模式无效 |
| `CFT-TYPE-021` | 量词绑定无效 |
| `CFT-TYPE-022` | 记录集合查询无效 |

## 数据模型、引用与规则执行

| 错误码 | 含义 |
| --- | --- |
| `DATA-001` | 记录类型未定义 |
| `DATA-002` | 记录类型为抽象类型 |
| `DATA-003` | 对象类型缺失 |
| `DATA-004` | 对象类型不匹配 |
| `DATA-005` | 数据字段未声明 |
| `DATA-006` | 必填字段缺失 |
| `DATA-007` | 数据值类型不匹配 |
| `DATA-008` | 枚举值无效 |
| `DATA-009` | 字典键重复 |
| `DATA-010` | 记录身份信息缺失 |
| `DATA-011` | 记录键重复 |
| `DATA-012` | 多态记录键重复 |
| `DATA-013` | 记录键无效 |
| `DATA-014` | 值依赖成环 |
| `DATA-015` | singleton 记录数量无效 |
| `DATA-016` | singleton 记录键无效 |
| `DATA-017` | singleton 记录键冲突 |
| `DATA-018` | 数据结构超出限制 |
| `REF-001` | 引用目标不存在 |
| `REF-002` | 引用目标类型不匹配 |
| `CHECK-001` | 业务规则未通过 |
| `CHECK-002` | 规则求值类型错误 |
| `CHECK-004` | 规则数组下标越界 |
| `CHECK-005` | 规则字典键不存在 |
| `CHECK-006` | 空集合不能求极值 |
| `CHECK-007` | 规则比较未通过 |
| `CHECK-008` | 规则布尔条件未通过 |
| `CHECK-009` | 规则否定条件未通过 |
| `CHECK-010` | 规则合取条件未通过 |
| `CHECK-011` | 规则析取条件未通过 |
| `CHECK-012` | 规则类型判断未通过 |
| `CHECK-014` | 规则包含条件未通过 |
| `CHECK-015` | 规则唯一性条件未通过 |
| `CHECK-016` | 规则模式匹配未通过 |
| `CHECK-017` | any 规则未通过 |
| `CHECK-018` | none 规则未通过 |
| `CHECK-019` | all 规则未通过 |
| `CHECK-020` | 规则执行预算耗尽 |

## 项目、文件与命令执行

| 错误码 | 含义 |
| --- | --- |
| `PROJECT-CONFIG-NOT-FOUND` | 项目配置未找到 |
| `PROJECT-CONFIG-AMBIGUOUS` | 项目配置有歧义 |
| `PROJECT-CONFIG-PATH` | 项目配置路径无效 |
| `PROJECT-CONFIG-READ` | 项目配置读取失败 |
| `PROJECT-CONFIG-PARSE` | 项目配置解析失败 |
| `PROJECT-CONFIG-WRITE` | 项目配置写入失败 |
| `PROJECT-CONFIG-OVERLAP` | 项目输入范围重叠 |
| `PROJECT-SCHEMA-PATH` | Schema 输入路径无效 |
| `PROJECT-SCHEMA-READ` | Schema 输入读取失败 |
| `PROJECT-CWD` | 当前目录无法读取 |
| `PROJECT-INIT-IO` | 项目初始化失败 |
| `PROJECT-FILE-CREATE` | 项目文件创建失败 |
| `PROJECT-FILE-DELETE` | 项目文件删除失败 |
| `PROJECT-FILE-RESTORE` | 项目文件恢复失败 |
| `CFD-READ` | CFD 写入来源读取失败 |
| `CFD-WRITE` | CFD 源码修改失败 |
| `CFD-SOURCE` | CFD 输入来源无效 |
| `CLI-REQUEST` | 命令请求无效 |
| `CLI-TARGET` | 查询或修改目标不存在 |
| `CLI-CONFLICT` | 输入或前置条件冲突 |
| `CLI-IO` | 请求载体读写失败 |
| `CLI-PUBLISH` | 候选发布失败 |
| `CLI-CANDIDATE` | 候选项目处理失败 |
| `CLI-FIELD` | 字段投影无效 |
| `CLI-PATH` | 值路径无效 |
| `CLI-SORT` | 排序字段无效 |
| `CLI-FILTER` | 筛选条件无效 |
| `CLI-VALUE` | 写入值无效 |
| `ARTIFACT-001` | 代码产物处理失败 |
| `ARTIFACT-IO` | 代码产物读写失败 |
| `CFD-TEXT-Syntax` | CFD 源码语法无效 |
| `CHECK-BUDGET` | 规则规划预算耗尽 |
| `CLI-LSP` | 语言服务启动失败 |
| `DIM-CONFIG-002` | 维度目录配置无效 |
| `GIT-DIFF` | Git 差异读取失败 |
| `GIT-EXTERNAL-SOURCE` | 历史外部输入不可还原 |
| `PROJECT-001` | 项目配置或生成目标无效 |
| `RUNTIME-INTERNAL` | 运行时内部状态无效 |
| `SCHEMA-STDIN-PATH` | 标准输入声明路径无效 |
| `MUTATION-COORDINATE` | 记录坐标无效 |
| `MUTATION-DEFAULT` | 默认值物化失败 |
| `MUTATION-DIMENSION` | 维度修改目标无效 |
| `MUTATION-DIMENSION-DESTINATION` | 维度目标文件无效 |
| `MUTATION-DIMENSION-PATH` | 维度值路径无效 |
| `MUTATION-DIMENSION-STALE` | 维度旧值条件冲突 |
| `MUTATION-FILE` | 修改文件无效 |
| `MUTATION-FILE-GUARD` | 来源文件条件冲突 |
| `MUTATION-INSERT` | 记录创建请求无效 |
| `MUTATION-INSERT-CONFLICT` | 创建记录身份冲突 |
| `MUTATION-PATH` | 修改路径无效 |
| `MUTATION-RENAME` | 记录改名请求无效 |
| `MUTATION-RENAME-CONFLICT` | 改名记录身份冲突 |
| `MUTATION-REORDER` | 记录排序请求无效 |
| `MUTATION-REORDER-BATCH` | 批量排序组合无效 |
| `MUTATION-REORDER-CONTAINER` | 记录排序容器不一致 |
| `MUTATION-SHAPE` | 修改值结构无效 |
| `MUTATION-TRANSFER` | 记录迁移请求无效 |
| `MUTATION-TRANSFER-FILE` | 迁移目标文件无效 |
| `MUTATION-TXN-INVARIANT` | 修改事务状态不一致 |
| `MUTATION-TYPE` | 修改值类型无效 |
| `MUTATION-VALUE` | 修改值无效 |
| `WRITE-AMBIGUOUS-SOURCE` | 写入来源有歧义 |
| `WRITE-NO-SOURCE` | 写入来源不存在 |
| `WRITE-NOT-FOUND` | 写入记录不存在 |
| `WRITE-RENAME` | 记录键写入值无效 |
| `WRITE-REORDER-CONTAINER` | 排序来源容器不一致 |
| `WRITE-REORDER-INDEX` | 排序位置越界 |
| `WRITE-REORDER-ORIGIN` | 排序来源不可写 |
| `WRITE-REORDER-TYPE` | 排序来源类型无效 |
| `WRITE-SHAPE` | 写入数据结构无效 |
| `WRITE-TRANSFER-FILE` | 记录迁移文件无效 |
| `WRITE-TRANSFER-INDEX` | 记录迁移位置越界 |
| `WRITE-TRANSFER-ORIGIN` | 记录迁移来源不可写 |
| `SELF-UPDATE-CONFIG` | 更新服务配置无效 |
| `SELF-UPDATE-CONFIRM` | 更新确认读取失败 |
| `SELF-UPDATE-INSTALL` | CLI 更新安装失败 |
| `SELF-UPDATE-OUTPUT` | 更新提示输出失败 |
| `SELF-UPDATE-PLATFORM` | 更新平台不受支持 |
| `SELF-UPDATE-RELEASE` | 更新版本查询失败 |
| `SELF-UPDATE-VERSION` | 更新版本信息无效 |
| `SKILL-HOME` | 技能安装目录无法确定 |
| `SKILL-DIR-CREATE` | 技能目录创建失败 |
| `SKILL-REMOVE` | 技能移除失败 |
| `SKILL-BUNDLE` | 技能包无效 |
| `SKILL-SWAP` | 技能包替换失败 |
| `SKILL-CLEANUP` | 技能临时文件清理失败 |
| `SKILL-FILE-WRITE` | 技能文件写入失败 |
| `SKILL-MANIFEST` | 技能安装清单无效 |
| `SKILL-DIR-READ` | 技能目录读取失败 |
| `SKILL-DIR-REMOVE` | 技能目录删除失败 |
| `CFD-TEXT-UnknownType` | CFD 记录或对象类型未定义 |
| `CFD-TEXT-AbstractObjectType` | CFD 对象使用抽象类型 |
| `CFD-TEXT-ObjectTypeMismatch` | CFD 对象类型不匹配 |
| `CFD-TEXT-UnknownField` | CFD 赋值字段未声明 |
| `CFD-TEXT-DuplicateField` | CFD 字段重复赋值 |
| `CFD-TEXT-ReservedIdField` | CFD 保留身份字段被赋值 |
| `CFD-TEXT-TypeMismatch` | CFD 表达式类型不匹配 |
| `CFD-TEXT-InvalidEnumVariant` | CFD 枚举表达式无效 |
| `CFD-TEXT-ReferenceNeedsMarker` | CFD 引用缺少标记 |
| `WRITE-CONFLICT` | 写入来源内容冲突 |
