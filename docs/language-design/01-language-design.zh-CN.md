# 语言语义与工具边界

CFT 提供数据声明、默认值、字段维度及业务检查规则。CFD 提供基础记录和维度覆盖。
声明编译产生可供编辑器、模型构建器及代码生成器使用的 schema。

语言规格按主题维护于 [语言总览](spec/01-总览.md)。
数据构建与字段校验由 coflow-model 负责；check 求值由 coflow-checker 负责。
共享源码扫描由 coflow-language 提供，规范格式化由 coflow-format 提供。

字符串类型统一为 string，普通字符串保存文本。模板使用显式 f 前缀，只支持字段路径、基础值转文本和花括号转义。
加载时解析模板依赖并求值；语法、路径及循环依赖错误保留来源。

check 使用条件语句、when 和 all/any/none 量词，由共享运行时按请求执行并生成诊断。
CLI 的 `coflow check --rules` 执行全部规则，`--rule` 选择具体规则，`coflow build` 在全部规则通过后生成代码；
编辑器会话在加载与变更后执行对应规则。`coflow codegen` 负责代码生成。
C# 加载直接构造生成类型，相关协议见 [生成类型与加载器](../codegen-design/csharp-runtime.zh-CN.md)。

字段省略只依据显式默认值，Option 表达值的空态。data、table 和 singleton 均可声明命名或匿名规则；
规则按拥有路径定位内联对象，类型继承与规则声明顺序决定执行次序。记录业务身份统一为 id，模型代次索引使用 handle。
完整的模块协作与验证边界见 [语言核心实现契约](05-language-evolution.zh-CN.md)。
