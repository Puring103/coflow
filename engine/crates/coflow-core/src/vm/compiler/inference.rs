//! 期望类型探测使用独立语义环境，不复制已生成的指令、闭包程序或控制流。
use super::*;
impl Compiler<'_> {
    pub(super) fn infer_expression_type(&self, expression: &Expr) -> Result<Ty> {
        // 局部编号仍引用同一类型表；仅复制语义状态，发射结果随探测结束立即丢弃。
        // 共享表达式检查规则，防止探测阶段和正式编译对可选值、收窄及捕获作不同判断。
        let mut probe = Compiler::new(self.schema, &self.program.name, self.program.source.clone(), self.context.clone());
        probe.program.values = self.program.values.clone();
        probe.program.parameters = self.program.parameters.clone();
        probe.program.result = self.program.result.clone();
        probe.scopes = self.scopes.clone();
        probe.outer = self.outer.clone();
        probe.captured = self.captured.clone();
        probe.capture_sources = self.capture_sources.clone();
        probe.program.captures = self.program.captures.clone();
        probe.literal_owner = self.literal_owner.clone();
        probe.refinements = self.refinements.clone();
        probe.expression(expression, None).map(|value| value.ty)
    }
}
