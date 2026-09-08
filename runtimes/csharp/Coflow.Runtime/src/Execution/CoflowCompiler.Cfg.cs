using System.Threading.Tasks;
using System.Threading;
using System.Linq;
using System.IO;
using System.Collections.Generic;
using System;
namespace Coflow.Runtime.CompilerServices
{

internal static partial class CoflowFunctionFrontend
{
        /// <summary>只把不可变 typed function 降为 CFG，不读取 parser 的任何可变状态。</summary>
        internal sealed class TypedCfgLowerer
        {
            private readonly CoflowFunctionEntry _entry;
            private readonly IReadOnlyDictionary<string, ICoflowTypeMetadata> _metadata;
            private readonly IReadOnlyDictionary<string, ICoflowEnumMetadata> _enums;

            internal TypedCfgLowerer(
                CoflowFunctionEntry entry,
                IReadOnlyDictionary<string, ICoflowTypeMetadata> metadata,
                IReadOnlyDictionary<string, ICoflowEnumMetadata> enums)
            {
                _entry = entry;
                _metadata = metadata;
                _enums = enums;
            }

            internal CoflowProgramTemplate Lower(TypedFunction typed)
            {
                if (typed is null) throw new ArgumentNullException(nameof(typed));
                if (TryCompileCfg(
                    typed.Body,
                    _entry.VmParameterTypes,
                    _entry.Signature.ResultType,
                    typed.BindingDependencies,
                    out var virtualProgram))
                    return new CoflowProgramTemplate(virtualProgram);
                throw new InvalidOperationException(
                    $"Expression `{typed.Body.GetType().Name}` has no typed CFG emitter.");
            }

            private CoflowProgramTemplate CompileLambda(
                CoflowFunctionSignature signature,
                IReadOnlyList<Expr> captures,
                Expr body)
            {
                var parameterTypes = signature.ParameterTypes
                    .Concat(captures.Select(value => value.Type))
                    .ToArray();
                if (TryCompileCfg(
                    body,
                    parameterTypes,
                    signature.ResultType,
                    Array.Empty<CoflowBindingDependency>(),
                    out var virtualProgram))
                    return new CoflowProgramTemplate(virtualProgram);
                throw new InvalidOperationException(
                    $"Lambda expression `{body.GetType().Name}` has no typed CFG emitter.");
            }

        private bool TryCompileCfg(
            Expr expression,
            IReadOnlyList<Type> parameterTypes,
            Type returnType,
            IReadOnlyList<CoflowBindingDependency> bindingDependencies,
            out CoflowVirtualProgram program)
        {
            if (!IsSimpleCfgExpression(expression, tailPosition: true))
            {
                program = null!;
                return false;
            }

            var builder = new CoflowVirtualProgramBuilder(
                _entry.Identity,
                _entry.SourcePath,
                _entry.SourceSpan,
                parameterTypes,
                returnType,
                bindingDependencies);
            EmitTailCfg(expression, builder);
            program = builder.Build();
            return true;
        }

        private static bool IsSimpleCfgExpression(
            Expr expression,
            bool tailPosition = false,
            bool insideLoop = false) => expression switch
        {
            ConstantExpr => true,
            ArgumentExpr => true,
            LocalExpr => true,
            TypeIsExpr typeIs => IsSimpleCfgExpression(typeIs.Value),
            RetypedExpr retyped => IsSimpleCfgExpression(retyped.Value, false, insideLoop),
            InterpolatedStringExpr interpolation => interpolation.Parts.All(part =>
                part.Value is null || IsSimpleCfgExpression(part.Value)),
            ObjectExpr value => value.Fields.All(field => IsSimpleCfgExpression(field.Value)),
            TypedNoneExpr => true,
            SomeExpr some => IsSimpleCfgExpression(some.Value),
            TypedResultBranchExpr branch => IsSimpleCfgExpression(branch.Value),
            ArrayExpr array => array.Values.All(value => IsSimpleCfgExpression(value)),
            DictionaryExpr dictionary => dictionary.Entries.All(entry =>
                IsSimpleCfgExpression(entry.Key) && IsSimpleCfgExpression(entry.Value)),
            IndexExpr index => IsSimpleCfgExpression(index.Receiver) && IsSimpleCfgExpression(index.Index),
            FieldExpr field => IsSimpleCfgExpression(field.Receiver),
            TransformExpr transform => IsSimpleCfgExpression(transform.Receiver),
            PropagateExpr propagate => IsSimpleCfgExpression(propagate.Operand),
            MatchExpr match => IsSimpleCfgExpression(match.Subject) &&
                match.Arms.All(arm => IsSimpleCfgExpression(arm.Body, tailPosition, insideLoop)),
            BuiltinExpr builtin => IsSimpleCfgExpression(builtin.Receiver) &&
                builtin.Arguments.All(argument => IsSimpleCfgExpression(argument)),
            HigherOrderExpr higherOrder => IsSimpleCfgExpression(higherOrder.Receiver) &&
                higherOrder.Arguments.All(argument => IsSimpleCfgExpression(argument)),
            FunctionReferenceExpr function => function.Receiver is null || IsSimpleCfgExpression(function.Receiver),
            LambdaExpr lambda => lambda.Captures.All(capture => IsSimpleCfgExpression(capture)),
            CallExpr call => IsSimpleCfgExpression(call.Target) &&
                call.Arguments.All(argument => IsSimpleCfgExpression(argument)),
            UnaryExpr unary => IsSimpleCfgExpression(unary.Operand),
            EnumUnaryExpr unary => IsSimpleCfgExpression(unary.Operand),
            EnumBinaryExpr binary => IsSimpleCfgExpression(binary.Left) && IsSimpleCfgExpression(binary.Right),
            ConversionExpr conversion => IsSimpleCfgExpression(conversion.Value),
            BinaryExpr binary => IsSimpleCfgExpression(binary.Left) && IsSimpleCfgExpression(binary.Right),
            EqualityExpr equality => IsSimpleCfgExpression(equality.Left) && IsSimpleCfgExpression(equality.Right),
            ComparisonChainExpr chain => chain.Operands.All(operand => IsSimpleCfgExpression(operand)),
            StoreLocalExpr store => IsSimpleCfgExpression(store.Value, false, insideLoop),
            AssignLocalExpr assign => IsSimpleCfgExpression(assign.Value, false, insideLoop),
            DiscardExpr discard => IsSimpleCfgExpression(discard.Value, false, insideLoop),
            ReturnExpr @return => tailPosition && IsSimpleCfgExpression(@return.Value, tailPosition: true, insideLoop),
            LoopControlExpr => insideLoop
                ? true
                : throw new InvalidOperationException("Loop control lost its typed CFG loop context."),
            WhileExpr loop => IsSimpleCfgExpression(loop.Condition) &&
                IsSimpleCfgExpression(loop.Body, tailPosition: true, insideLoop: true),
            ForExpr loop => IsSimpleCfgExpression(loop.Collection) &&
                IsSimpleCfgExpression(loop.Body, tailPosition: true, insideLoop: true),
            RangeForExpr loop => IsSimpleCfgExpression(loop.Start) && IsSimpleCfgExpression(loop.End) &&
                IsSimpleCfgExpression(loop.Body, tailPosition: true, insideLoop: true),
            RangeExpr => true,
            IfExpr conditional => IsSimpleCfgExpression(conditional.Condition) &&
                IsSimpleCfgExpression(conditional.WhenTrue, tailPosition, insideLoop) &&
                IsSimpleCfgExpression(conditional.WhenFalse, tailPosition, insideLoop),
            BlockExpr block => block.Statements.All(statement =>
                    IsSimpleCfgExpression(
                        statement,
                        tailPosition && statement is ReturnExpr,
                        insideLoop)) &&
                IsSimpleCfgExpression(block.Result, tailPosition, insideLoop),
            _ => throw new InvalidOperationException(
                $"Expression `{expression.GetType().Name}` has no typed CFG support."),
        };

        private void EmitTailCfg(Expr expression, CoflowVirtualProgramBuilder builder)
        {
            var origin = Origin(expression);
            if (expression is CallExpr call)
            {
                if (call.Target is FunctionReferenceExpr direct)
                {
                    var inputs = call.Arguments.Select(argument => EmitSimpleCfg(argument, builder)).ToList();
                    if (direct.Receiver is not null) inputs.Add(EmitSimpleCfg(direct.Receiver, builder));
                    builder.DirectTailCall(
                        inputs, CoflowCallSite.From(direct.Entry, inputs.Count), origin);
                }
                else
                {
                    var inputs = new[] { EmitSimpleCfg(call.Target, builder) }
                        .Concat(call.Arguments.Select(argument => EmitSimpleCfg(argument, builder))).ToArray();
                    builder.IndirectTailCall(inputs, origin);
                }
                return;
            }

            if (expression is IfExpr conditional)
            {
                var condition = EmitSimpleCfg(conditional.Condition, builder);
                var whenTrue = builder.CreateBlock();
                var whenFalse = builder.CreateBlock();
                builder.Branch(condition, whenTrue, whenFalse, origin);
                builder.Enter(whenTrue);
                EmitTailCfg(conditional.WhenTrue, builder);
                builder.Enter(whenFalse);
                EmitTailCfg(conditional.WhenFalse, builder);
                return;
            }

            if (expression is MatchExpr match)
            {
                EmitMatchCfg(match, builder, tailPosition: true, origin);
                return;
            }

            if (expression is BlockExpr block)
            {
                foreach (var statement in block.Statements)
                {
                    EmitStatementCfg(statement, builder);
                    if (builder.IsCurrentTerminated) return;
                }
                EmitTailCfg(block.Result, builder);
                return;
            }

            if (expression is ReturnExpr returned)
            {
                EmitTailCfg(returned.Value, builder);
                return;
            }

            builder.Return(EmitSimpleCfg(expression, builder), origin);
        }

        private void EmitStatementCfg(Expr expression, CoflowVirtualProgramBuilder builder)
        {
            var origin = Origin(expression);
            switch (expression)
            {
                case ReturnExpr returned:
                    EmitTailCfg(returned.Value, builder);
                    return;
                case LoopControlExpr loopControl:
                    var targets = builder.CurrentLoop();
                    builder.Jump(loopControl.IsBreak ? targets.Break : targets.Continue, origin);
                    return;
                case WhileExpr loop:
                    EmitWhileCfg(loop, builder, origin);
                    return;
                case ForExpr loop:
                    EmitForCfg(loop, builder, origin);
                    return;
                case RangeForExpr loop:
                    EmitRangeForCfg(loop, builder, origin);
                    return;
                case DiscardExpr discard:
                    EmitStatementCfg(discard.Value, builder);
                    return;
                case IfExpr conditional:
                    EmitIfStatementCfg(conditional, builder, origin);
                    return;
                case BlockExpr block:
                    foreach (var statement in block.Statements)
                    {
                        EmitStatementCfg(statement, builder);
                        if (builder.IsCurrentTerminated) return;
                    }
                    if (block.Result.Type == typeof(Unit) || block.Result.AlwaysTerminates)
                        EmitStatementCfg(block.Result, builder);
                    else
                        EmitSimpleCfg(block.Result, builder);
                    return;
                default:
                    EmitSimpleCfg(expression, builder);
                    return;
            }
        }

        private void EmitIfStatementCfg(
            IfExpr conditional,
            CoflowVirtualProgramBuilder builder,
            CoflowSourceOrigin origin)
        {
            var condition = EmitSimpleCfg(conditional.Condition, builder);
            var whenTrue = builder.CreateBlock();
            var whenFalse = builder.CreateBlock();
            var done = builder.CreateBlock();
            builder.Branch(condition, whenTrue, whenFalse, origin);

            builder.Enter(whenTrue);
            EmitStatementCfg(conditional.WhenTrue, builder);
            if (!builder.IsCurrentTerminated) builder.Jump(done, origin);

            builder.Enter(whenFalse);
            EmitStatementCfg(conditional.WhenFalse, builder);
            if (!builder.IsCurrentTerminated) builder.Jump(done, origin);

            builder.Enter(done);
        }

        private void EmitWhileCfg(
            WhileExpr loop,
            CoflowVirtualProgramBuilder builder,
            CoflowSourceOrigin origin)
        {
            var conditionBlock = builder.CreateBlock();
            var bodyBlock = builder.CreateBlock();
            var doneBlock = builder.CreateBlock();
            builder.Jump(conditionBlock, origin);

            builder.Enter(conditionBlock);
            var condition = EmitSimpleCfg(loop.Condition, builder);
            builder.Branch(condition, bodyBlock, doneBlock, origin);

            builder.Enter(bodyBlock);
            builder.EnterLoop(conditionBlock, doneBlock);
            try { EmitStatementCfg(loop.Body, builder); }
            finally { builder.ExitLoop(); }
            if (!builder.IsCurrentTerminated) builder.Jump(conditionBlock, origin);

            builder.Enter(doneBlock);
        }

        private void EmitForCfg(
            ForExpr loop,
            CoflowVirtualProgramBuilder builder,
            CoflowSourceOrigin origin)
        {
            var collection = AssignLocalCfg(
                builder, loop.CollectionLocal, loop.Collection.Type,
                EmitSimpleCfg(loop.Collection, builder), origin);
            var index = AssignLocalCfg(
                builder, loop.IndexLocal, typeof(long), IntegerConstant(builder, 0, origin), origin);
            var conditionBlock = builder.CreateBlock();
            var bodyBlock = builder.CreateBlock();
            var incrementBlock = builder.CreateBlock();
            var doneBlock = builder.CreateBlock();
            builder.Jump(conditionBlock, origin);

            builder.Enter(conditionBlock);
            var count = builder.Emit(
                new CoflowVirtualOperation.CollectionRead(CoflowVirtualCollectionReadKind.Count),
                typeof(long), new[] { collection }, origin);
            var hasNext = builder.Binary("<", typeof(bool), index, count, origin);
            builder.Branch(hasNext, bodyBlock, doneBlock, origin);

            builder.Enter(bodyBlock);
            var first = builder.Emit(
                new CoflowVirtualOperation.CollectionRead(loop.IsArray
                    ? CoflowVirtualCollectionReadKind.ArrayItem
                    : CoflowVirtualCollectionReadKind.DictionaryKey),
                loop.FirstType, new[] { collection, index }, origin);
            AssignLocalCfg(builder, loop.FirstLocal, loop.FirstType, first, origin);
            if (loop.SecondLocal is { } secondLocal)
            {
                var second = loop.IsArray
                    ? index
                    : builder.Emit(
                        new CoflowVirtualOperation.CollectionRead(CoflowVirtualCollectionReadKind.DictionaryValue),
                        loop.SecondType!, new[] { collection, index }, origin);
                AssignLocalCfg(builder, secondLocal, loop.SecondType ?? typeof(long), second, origin);
            }
            builder.EnterLoop(incrementBlock, doneBlock);
            try { EmitStatementCfg(loop.Body, builder); }
            finally { builder.ExitLoop(); }
            if (!builder.IsCurrentTerminated) builder.Jump(incrementBlock, origin);

            builder.Enter(incrementBlock);
            var next = builder.Binary(
                "+", typeof(long), index, IntegerConstant(builder, 1, origin), origin);
            builder.MoveTo(index, next, origin);
            builder.Jump(conditionBlock, origin);
            builder.Enter(doneBlock);
        }

        private void EmitRangeForCfg(
            RangeForExpr loop,
            CoflowVirtualProgramBuilder builder,
            CoflowSourceOrigin origin)
        {
            var value = AssignLocalCfg(
                builder, loop.ValueLocal, typeof(long), EmitSimpleCfg(loop.Start, builder), origin);
            var end = AssignLocalCfg(
                builder, loop.EndLocal, typeof(long), EmitSimpleCfg(loop.End, builder), origin);
            CoflowVirtualValue? index = loop.IndexLocal is { } indexLocal
                ? AssignLocalCfg(builder, indexLocal, typeof(long), IntegerConstant(builder, 0, origin), origin)
                : null;
            var conditionBlock = builder.CreateBlock();
            var bodyBlock = builder.CreateBlock();
            var incrementBlock = builder.CreateBlock();
            var addBlock = builder.CreateBlock();
            var doneBlock = builder.CreateBlock();
            builder.Jump(conditionBlock, origin);

            builder.Enter(conditionBlock);
            var inRange = builder.Binary(loop.Inclusive ? "<=" : "<", typeof(bool), value, end, origin);
            builder.Branch(inRange, bodyBlock, doneBlock, origin);

            builder.Enter(bodyBlock);
            builder.EnterLoop(incrementBlock, doneBlock);
            try { EmitStatementCfg(loop.Body, builder); }
            finally { builder.ExitLoop(); }
            if (!builder.IsCurrentTerminated) builder.Jump(incrementBlock, origin);

            builder.Enter(incrementBlock);
            if (loop.Inclusive)
            {
                var reachedEnd = builder.Binary("==", typeof(bool), value, end, origin);
                builder.Branch(reachedEnd, doneBlock, addBlock, origin);
            }
            else builder.Jump(addBlock, origin);

            builder.Enter(addBlock);
            var nextValue = builder.Binary(
                "+", typeof(long), value, IntegerConstant(builder, 1, origin), origin);
            builder.MoveTo(value, nextValue, origin);
            if (index is { } indexValue)
            {
                var nextIndex = builder.Binary(
                    "+", typeof(long), indexValue, IntegerConstant(builder, 1, origin), origin);
                builder.MoveTo(indexValue, nextIndex, origin);
            }
            builder.Jump(conditionBlock, origin);
            builder.Enter(doneBlock);
        }

        private static CoflowVirtualValue AssignLocalCfg(
            CoflowVirtualProgramBuilder builder,
            int index,
            Type type,
            CoflowVirtualValue source,
            CoflowSourceOrigin origin)
        {
            var local = builder.Local(index, type);
            builder.MoveTo(local, source, origin);
            return local;
        }

        private static CoflowVirtualValue IntegerConstant(
            CoflowVirtualProgramBuilder builder,
            long value,
            CoflowSourceOrigin origin) => builder.Constant(typeof(long), value, origin);

        private CoflowVirtualValue EmitSimpleCfg(Expr expression, CoflowVirtualProgramBuilder builder)
        {
            var origin = Origin(expression);
            switch (expression)
            {
                case ConstantExpr constant:
                    return builder.Constant(
                        constant.Type, constant.TemplateValue ?? constant.Value, origin);
                case RangeExpr:
                    throw new FunctionCompileException(
                        "COFLOW-FUNCTION-TYPE",
                        "range expressions can only be used by a for loop",
                        expression.SourceOffset);
                case ArgumentExpr argument:
                    return builder.Parameters[argument.Index];
                case LocalExpr local:
                    return builder.Local(local.Index, local.Type);
                case TypeIsExpr typeIs:
                    var typeInput = EmitSimpleCfg(typeIs.Value, builder);
                    return builder.Emit(
                        new CoflowVirtualOperation.TypeTest(typeIs.TargetType,
                            CoflowValueShape.Of(typeIs.Value.Type).Kind == CoflowValueShapeKind.Record),
                        typeof(bool), new[] { typeInput }, origin);
                case RetypedExpr retyped:
                    return builder.Emit(new CoflowVirtualOperation.Move(), retyped.Type,
                        new[] { EmitSimpleCfg(retyped.Value, builder) }, origin);
                case InterpolatedStringExpr interpolation:
                    return EmitNative(
                        interpolation.Type,
                        interpolation.Parts.Where(part => part.Value is not null)
                            .Select(part => EmitSimpleCfg(part.Value!, builder)).ToArray(),
                        CoflowFormatting.Interpolation(
                            interpolation.Parts.Select(part => part.Text).ToArray(),
                            interpolation.Parts.Select(part => part.Value?.Type).ToArray(),
                            _metadata,
                            _enums));
                case ObjectExpr value:
                    return EmitObjectCfg(value, builder, origin);
                case TypedNoneExpr none:
                    return builder.Emit(
                        new CoflowVirtualOperation.MakeOptionNone(), none.Type,
                        Array.Empty<CoflowVirtualValue>(), origin);
                case SomeExpr some:
                    return builder.Emit(
                        new CoflowVirtualOperation.MakeOptionSome(), some.Type,
                        new[] { EmitSimpleCfg(some.Value, builder) }, origin);
                case TypedResultBranchExpr branch:
                    return builder.Emit(
                        new CoflowVirtualOperation.MakeResult(branch.IsOk), branch.Type,
                        new[] { EmitSimpleCfg(branch.Value, builder) }, origin);
                case ArrayExpr array:
                    return builder.Emit(
                        new CoflowVirtualOperation.MakeCollection(false), array.Type,
                        array.Values.Select(value => EmitSimpleCfg(value, builder)).ToArray(), origin);
                case DictionaryExpr dictionary:
                    return builder.Emit(
                        new CoflowVirtualOperation.MakeCollection(true), dictionary.Type,
                        dictionary.Entries.SelectMany(entry => new[]
                        {
                            EmitSimpleCfg(entry.Key, builder),
                            EmitSimpleCfg(entry.Value, builder),
                        }).ToArray(), origin);
                case IndexExpr index:
                    return builder.Emit(
                        new CoflowVirtualOperation.CollectionIndex(index.IsDictionary), index.Type,
                        new[] { EmitSimpleCfg(index.Receiver, builder), EmitSimpleCfg(index.Index, builder) },
                        origin);
                case FieldExpr field:
                    return builder.Emit(
                        new CoflowVirtualOperation.FieldRead(field.Access), field.Type,
                        new[] { EmitSimpleCfg(field.Receiver, builder) },
                        origin);
                case TransformExpr transform:
                    return EmitNative(
                        transform.Type,
                        new[] { EmitSimpleCfg(transform.Receiver, builder) },
                        transform.Transform);
                case PropagateExpr propagate:
                    var source = EmitSimpleCfg(propagate.Operand, builder);
                    var continuation = builder.CreateBlock();
                    var payload = builder.Propagate(source, propagate.Type, continuation, origin);
                    builder.Enter(continuation);
                    return payload;
                case MatchExpr match:
                    return EmitMatchCfg(match, builder, tailPosition: false, origin)!.Value;
                case BuiltinExpr builtin:
                    var builtinInputs = new[] { EmitSimpleCfg(builtin.Receiver, builder) }
                        .Concat(builtin.Arguments.Select(argument => EmitSimpleCfg(argument, builder))).ToArray();
                    return builtin.Builtin.Kind switch
                    {
                        CoflowBuiltinKind.CollectionCount => builder.Emit(
                            new CoflowVirtualOperation.CollectionRead(CoflowVirtualCollectionReadKind.Count),
                            builtin.Type, builtinInputs, origin),
                        CoflowBuiltinKind.DictionaryKeys => builder.Emit(
                            new CoflowVirtualOperation.DictionaryProjection(false), builtin.Type, builtinInputs, origin),
                        CoflowBuiltinKind.DictionaryValues => builder.Emit(
                            new CoflowVirtualOperation.DictionaryProjection(true), builtin.Type, builtinInputs, origin),
                        CoflowBuiltinKind.Native => builder.Native(
                            builtin.Type, builtinInputs, builtin.Builtin.Call!, origin),
                        _ => builder.Emit(
                            new CoflowVirtualOperation.CollectionBuiltin(builtin.Builtin),
                            builtin.Type, builtinInputs, origin),
                    };
                case HigherOrderExpr higherOrder:
                    return EmitHigherOrderCfg(higherOrder, builder, origin);
                case FunctionReferenceExpr function:
                    if (function.Receiver is null)
                        return builder.Constant(function.Type,
                            new CoflowFunctionReferenceTemplate(function.Entry.Identity, null), origin);
                    return builder.Emit(
                        new CoflowVirtualOperation.BindFunction(
                            new CoflowFunctionReferenceTemplate(function.Entry.Identity, function.Receiver.Type)),
                        function.Type, new[] { EmitSimpleCfg(function.Receiver, builder) }, origin);
                case LambdaExpr lambda:
                    var captures = lambda.Captures.Select(capture => EmitSimpleCfg(capture, builder)).ToArray();
                    var closureProgram = CompileLambda(lambda.Signature, lambda.Captures, lambda.Body);
                    var closureTemplate = new CoflowClosureProgramTemplate(
                        closureProgram, lambda.Captures.Select(capture => capture.Type).ToArray());
                    return builder.Emit(
                        new CoflowVirtualOperation.MakeClosure(closureTemplate),
                        lambda.Type, captures, origin);
                case CallExpr call:
                    if (call.Target is FunctionReferenceExpr direct)
                    {
                        var directInputs = call.Arguments.Select(argument => EmitSimpleCfg(argument, builder)).ToList();
                        if (direct.Receiver is not null) directInputs.Add(EmitSimpleCfg(direct.Receiver, builder));
                        return builder.Emit(
                            new CoflowVirtualOperation.DirectCall(
                                CoflowCallSite.From(direct.Entry, directInputs.Count)),
                            call.Type, directInputs, origin);
                    }
                    var indirectInputs = new[] { EmitSimpleCfg(call.Target, builder) }
                        .Concat(call.Arguments.Select(argument => EmitSimpleCfg(argument, builder))).ToArray();
                    return builder.Emit(
                        new CoflowVirtualOperation.IndirectCall(), call.Type, indirectInputs, origin);
                case UnaryExpr unary:
                    return Unary(unary.Operation, unary.Type, unary.Operand);
                case EnumUnaryExpr unary:
                    return Unary("~", unary.Type, unary.Operand);
                case EnumBinaryExpr binary:
                    return Binary(binary.Operation, binary.Type, binary.Left, binary.Right);
                case ConversionExpr conversion:
                    return builder.Emit(
                        new CoflowVirtualOperation.Convert(conversion.Value.Type, conversion.Type),
                        conversion.Type, new[] { EmitSimpleCfg(conversion.Value, builder) }, origin);
                case BinaryExpr binary:
                    if (binary.Operation is "&&" or "||")
                        return EmitShortCircuitCfg(binary, builder, origin);
                    return Binary(binary.Operation, binary.Type, binary.Left, binary.Right);
                case EqualityExpr equality:
                    var equal = EmitNative(
                        typeof(bool),
                        new[] { EmitSimpleCfg(equality.Left, builder), EmitSimpleCfg(equality.Right, builder) },
                        CoflowEquality.Create(equality.Left.Type));
                    return equality.Negated
                        ? builder.Unary("!", typeof(bool), equal, origin)
                        : equal;
                case ComparisonChainExpr chain:
                    return EmitComparisonChainCfg(chain, builder, origin);
                case StoreLocalExpr store:
                    return StoreLocal(store.Index, store.Value);
                case AssignLocalExpr assign:
                    return StoreLocal(assign.Index, assign.Value);
                case DiscardExpr discard:
                    EmitSimpleCfg(discard.Value, builder);
                    return UnitValue();
                case IfExpr conditional:
                    return EmitIfCfg(conditional, builder, origin);
                case BlockExpr block:
                    foreach (var statement in block.Statements) EmitSimpleCfg(statement, builder);
                    return EmitSimpleCfg(block.Result, builder);
                default:
                    throw new InvalidOperationException($"Expression `{expression.GetType().Name}` is not in the simple CFG subset.");
            }

            CoflowVirtualValue Unary(string operation, Type type, Expr operand) => builder.Unary(
                operation, type, EmitSimpleCfg(operand, builder), origin);

            CoflowVirtualValue Binary(string operation, Type type, Expr left, Expr right) => builder.Binary(
                operation, type, EmitSimpleCfg(left, builder), EmitSimpleCfg(right, builder), origin);

            CoflowVirtualValue EmitNative(
                Type type,
                CoflowVirtualValue[] inputs,
                CoflowNativeCall call) => builder.Native(type, inputs, call, origin);

            CoflowVirtualValue StoreLocal(int index, Expr value)
            {
                var source = EmitSimpleCfg(value, builder);
                var local = builder.Local(index, value.Type);
                builder.MoveTo(local, source, origin);
                return UnitValue();
            }

            CoflowVirtualValue UnitValue() => builder.Constant(typeof(Unit), Unit.Value, origin);
        }

        private CoflowVirtualValue EmitHigherOrderCfg(
            HigherOrderExpr expression,
            CoflowVirtualProgramBuilder builder,
            CoflowSourceOrigin origin)
        {
            var fold = expression.Operation.Name == "fold";
            var collection = builder.CreateLocal(expression.Receiver.Type);
            Assign(collection, EmitSimpleCfg(expression.Receiver, builder));
            var result = builder.CreateLocal(expression.Type);
            CoflowVirtualValue callable;
            if (fold)
            {
                Assign(result, EmitSimpleCfg(expression.Arguments[0], builder));
                callable = EmitSimpleCfg(expression.Arguments[1], builder);
            }
            else callable = EmitSimpleCfg(expression.Arguments[0], builder);

            var count = builder.Emit(
                new CoflowVirtualOperation.CollectionRead(CoflowVirtualCollectionReadKind.Count),
                typeof(long), new[] { collection }, origin);
            if (expression.Operation.Name is "map" or "filter")
            {
                var array = builder.Emit(
                    new CoflowVirtualOperation.ArrayBuilder(false), expression.Type, new[] { count }, origin);
                Assign(result, array);
            }
            else if (expression.Operation.Name == "find")
            {
                var none = builder.Emit(
                    new CoflowVirtualOperation.MakeOptionNone(), expression.Type,
                    Array.Empty<CoflowVirtualValue>(), origin);
                Assign(result, none);
            }
            else if (expression.Operation.Name is "any" or "all")
            {
                var initial = builder.Constant(typeof(bool), expression.Operation.Name == "all", origin);
                Assign(result, initial);
            }

            var index = builder.CreateLocal(typeof(long));
            Assign(index, IntegerConstant(builder, 0, origin));
            var conditionBlock = builder.CreateBlock();
            var bodyBlock = builder.CreateBlock();
            var incrementBlock = builder.CreateBlock();
            var doneBlock = builder.CreateBlock();
            builder.Jump(conditionBlock, origin);

            builder.Enter(conditionBlock);
            var hasNext = builder.Binary("<", typeof(bool), index, count, origin);
            builder.Branch(hasNext, bodyBlock, doneBlock, origin);

            builder.Enter(bodyBlock);
            var item = builder.Emit(
                new CoflowVirtualOperation.CollectionRead(CoflowVirtualCollectionReadKind.ArrayItem),
                expression.Operation.ElementType, new[] { collection, index }, origin);
            var callbackInputs = fold
                ? new[] { callable, result, item }
                : new[] { callable, item };
            var callbackType = expression.Operation.Name == "map" || fold
                ? expression.Operation.OutputElementType
                : typeof(bool);
            var callbackResult = builder.Emit(
                new CoflowVirtualOperation.IndirectCall(), callbackType, callbackInputs, origin);

            switch (expression.Operation.Name)
            {
                case "map":
                    builder.Emit(
                        new CoflowVirtualOperation.ArrayBuilder(true), typeof(Unit),
                        new[] { result, callbackResult }, origin);
                    builder.Jump(incrementBlock, origin);
                    break;
                case "filter":
                    var append = builder.CreateBlock();
                    builder.Branch(callbackResult, append, incrementBlock, origin);
                    builder.Enter(append);
                    builder.Emit(
                        new CoflowVirtualOperation.ArrayBuilder(true), typeof(Unit),
                        new[] { result, item }, origin);
                    builder.Jump(incrementBlock, origin);
                    break;
                case "fold":
                    Assign(result, callbackResult);
                    builder.Jump(incrementBlock, origin);
                    break;
                default:
                    var early = builder.CreateBlock();
                    var keepGoing = builder.CreateBlock();
                    var shouldStop = expression.Operation.Name == "all"
                        ? builder.Unary("!", typeof(bool), callbackResult, origin)
                        : callbackResult;
                    builder.Branch(shouldStop, early, keepGoing, origin);
                    builder.Enter(early);
                    if (expression.Operation.Name == "find")
                    {
                        var found = builder.Emit(
                            new CoflowVirtualOperation.MakeOptionSome(), expression.Type,
                            new[] { item }, origin);
                        Assign(result, found);
                    }
                    else
                    {
                        var value = builder.Constant(
                            typeof(bool), expression.Operation.Name == "any", origin);
                        Assign(result, value);
                    }
                    builder.Jump(doneBlock, origin);
                    builder.Enter(keepGoing);
                    builder.Jump(incrementBlock, origin);
                    break;
            }

            builder.Enter(incrementBlock);
            var nextIndex = builder.Binary(
                "+", typeof(long), index, IntegerConstant(builder, 1, origin), origin);
            Assign(index, nextIndex);
            builder.Jump(conditionBlock, origin);
            builder.Enter(doneBlock);
            return result;

            void Assign(CoflowVirtualValue target, CoflowVirtualValue source) =>
                builder.MoveTo(target, source, origin);
        }

        private CoflowVirtualValue? EmitMatchCfg(
            MatchExpr match,
            CoflowVirtualProgramBuilder builder,
            bool tailPosition,
            CoflowSourceOrigin origin)
        {
            var subject = AssignLocalCfg(
                builder, match.SubjectLocal, match.Subject.Type,
                EmitSimpleCfg(match.Subject, builder), origin);
            var done = tailPosition ? null : builder.CreateBlock();
            var result = tailPosition ? (CoflowVirtualValue?)null : builder.CreateValue(match.Type);

            for (var index = 0; index < match.Arms.Count; index++)
            {
                var arm = match.Arms[index];
                var isLastComplement = index == match.Arms.Count - 1 &&
                    (arm.Pattern.IsCatchAll || match.LastIsComplement);
                CoflowBasicBlock? next = null;
                if (!isLastComplement)
                {
                    var body = builder.CreateBlock();
                    next = builder.CreateBlock();
                    var condition = EmitMatchCondition(match, arm.Pattern, subject, builder, origin);
                    builder.Branch(condition, body, next, origin);
                    builder.Enter(body);
                }

                if (arm.BindingLocal is { } binding)
                {
                    var bound = subject;
                    if (arm.Pattern.Payload is { } payload)
                        bound = builder.Emit(
                            new CoflowVirtualOperation.ReadPayload(payload == 0),
                            arm.Pattern.BindingType!, new[] { subject }, origin);
                    else if (arm.Pattern.TypeTarget is not null && arm.Pattern.BindingType is { } bindingType &&
                        bindingType != subject.Type)
                        bound = builder.Emit(
                            new CoflowVirtualOperation.Move(), bindingType, new[] { subject }, origin);
                    AssignLocalCfg(builder, binding, arm.Pattern.BindingType ?? subject.Type, bound, origin);
                }

                if (tailPosition) EmitTailCfg(arm.Body, builder);
                else
                {
                    var armValue = EmitSimpleCfg(arm.Body, builder);
                    builder.MoveTo(result!.Value, armValue, origin);
                    builder.Jump(done!, origin);
                }
                if (next is not null) builder.Enter(next);
            }

            if (done is not null) builder.Enter(done);
            return result;
        }

        private CoflowVirtualValue EmitMatchCondition(
            MatchExpr match,
            MatchPattern pattern,
            CoflowVirtualValue subject,
            CoflowVirtualProgramBuilder builder,
            CoflowSourceOrigin origin)
        {
            if (pattern.TagValue is { } tag)
            {
                var valueTag = builder.Emit(
                    new CoflowVirtualOperation.ReadValueTag(), typeof(bool), new[] { subject }, origin);
                return tag ? valueTag : builder.Unary("!", typeof(bool), valueTag, origin);
            }
            if (pattern.TypeTarget is { } targetType)
            {
                return builder.Emit(
                    new CoflowVirtualOperation.TypeTest(targetType,
                        CoflowValueShape.Of(match.Subject.Type).Kind == CoflowValueShapeKind.Record),
                    typeof(bool), new[] { subject }, origin);
            }
            var literal = builder.Constant(match.Subject.Type, pattern.LiteralValue, origin);
            return builder.Binary("==", typeof(bool), subject, literal, origin);
        }

        private CoflowVirtualValue EmitComparisonChainCfg(
            ComparisonChainExpr chain,
            CoflowVirtualProgramBuilder builder,
            CoflowSourceOrigin origin)
        {
            var operands = chain.Operands.Select(operand => EmitSimpleCfg(operand, builder)).ToArray();
            var result = builder.CreateValue(typeof(bool));
            var failed = builder.CreateBlock();
            var done = builder.CreateBlock();
            for (var index = 0; index < chain.Operations.Count; index++)
            {
                var comparison = builder.Binary(
                    chain.Operations[index], typeof(bool), operands[index], operands[index + 1], origin);
                if (index + 1 == chain.Operations.Count)
                {
                    builder.MoveTo(result, comparison, origin);
                    builder.Jump(done, origin);
                    break;
                }
                var next = builder.CreateBlock();
                builder.Branch(comparison, next, failed, origin);
                builder.Enter(next);
            }
            builder.Enter(failed);
            var falseValue = builder.Constant(typeof(bool), false, origin);
            builder.MoveTo(result, falseValue, origin);
            builder.Jump(done, origin);
            builder.Enter(done);
            return result;
        }

        private CoflowVirtualValue EmitObjectCfg(
            ObjectExpr value,
            CoflowVirtualProgramBuilder builder,
            CoflowSourceOrigin origin)
        {
            var supplied = value.Fields.ToDictionary(field => field.Name, StringComparer.Ordinal);
            var fields = new List<CoflowVirtualValue>();
            foreach (var metadata in value.Metadata.Fields)
            {
                if (metadata.Binding.IsFunction) continue;
                if (supplied.TryGetValue(metadata.Name, out var suppliedField))
                    fields.Add(EmitSimpleCfg(suppliedField.Value, builder));
                else
                    fields.Add(builder.Native(
                        metadata.Binding.RuntimeType, Array.Empty<CoflowVirtualValue>(),
                        value.Metadata.CreateVmDefaultFactory(metadata.Name, value.Context).Call, origin));
            }
            return builder.Native(
                value.Type, fields, value.Metadata.CreateVmObjectFactory(value.Context).Call, origin);
        }

        private CoflowVirtualValue EmitShortCircuitCfg(
            BinaryExpr binary,
            CoflowVirtualProgramBuilder builder,
            CoflowSourceOrigin origin)
        {
            var left = EmitSimpleCfg(binary.Left, builder);
            var evaluateRight = builder.CreateBlock();
            var shortCircuit = builder.CreateBlock();
            var done = builder.CreateBlock();
            var result = builder.CreateValue(typeof(bool));
            if (binary.Operation == "&&") builder.Branch(left, evaluateRight, shortCircuit, origin);
            else builder.Branch(left, shortCircuit, evaluateRight, origin);

            builder.Enter(shortCircuit);
            builder.MoveTo(result, left, origin);
            builder.Jump(done, origin);

            builder.Enter(evaluateRight);
            var right = EmitSimpleCfg(binary.Right, builder);
            builder.MoveTo(result, right, origin);
            builder.Jump(done, origin);

            builder.Enter(done);
            return result;
        }

        private CoflowVirtualValue EmitIfCfg(
            IfExpr conditional,
            CoflowVirtualProgramBuilder builder,
            CoflowSourceOrigin origin)
        {
            var condition = EmitSimpleCfg(conditional.Condition, builder);
            var whenTrue = builder.CreateBlock();
            var whenFalse = builder.CreateBlock();
            var done = builder.CreateBlock();
            var result = builder.CreateValue(conditional.Type);
            builder.Branch(condition, whenTrue, whenFalse, origin);

            builder.Enter(whenTrue);
            var trueValue = EmitSimpleCfg(conditional.WhenTrue, builder);
            builder.MoveTo(result, trueValue, origin);
            builder.Jump(done, origin);

            builder.Enter(whenFalse);
            var falseValue = EmitSimpleCfg(conditional.WhenFalse, builder);
            builder.MoveTo(result, falseValue, origin);
            builder.Jump(done, origin);

            builder.Enter(done);
            return result;
        }

        private CoflowSourceOrigin Origin(Expr expression) => new(
            _entry.SourcePath,
            expression.SourceOffset < 0 || _entry.Source is null
                ? null
                : CoflowCompilationPipeline.FunctionSpan(_entry.Source, expression.SourceOffset));

        }
}
}
