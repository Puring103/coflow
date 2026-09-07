namespace Coflow.Runtime.CompilerServices;

/// <summary>唯一维护一次执行中的预算占用，保证帧和寄存器配额成对释放。</summary>
internal sealed class CoflowExecutionBudgetLease
{
    private CoflowExecutionBudget? _budget;
    private int _frames;
    private int _integerRegisters;
    private int _floatRegisters;
    private int _referenceRegisters;

    internal CoflowExecutionBudget Budget => _budget ??
        throw new InvalidOperationException("The Coflow execution budget is not active.");

    internal void Start(CoflowExecutionBudget budget)
    {
        if (budget is null) throw new ArgumentNullException(nameof(budget));
        if (_budget is not null) throw new InvalidOperationException("The Coflow execution budget is already active.");
        _budget = budget;
        EnterFrame();
    }

    internal void EnterFrame()
    {
        Budget.EnterFrame();
        _frames++;
    }

    internal void ExitFrame()
    {
        if (_frames == 0) throw new InvalidOperationException("No Coflow frame budget is active.");
        Budget.ExitFrame();
        _frames--;
    }

    internal void AcquireRegisters(int integerTop, int floatTop, int referenceTop)
    {
        var integerHighWater = Math.Max(_integerRegisters, integerTop);
        var floatHighWater = Math.Max(_floatRegisters, floatTop);
        var referenceHighWater = Math.Max(_referenceRegisters, referenceTop);
        Budget.AcquireRegisters(
            integerHighWater - _integerRegisters,
            floatHighWater - _floatRegisters,
            referenceHighWater - _referenceRegisters);
        _integerRegisters = integerHighWater;
        _floatRegisters = floatHighWater;
        _referenceRegisters = referenceHighWater;
    }

    internal void Release()
    {
        if (_budget is null) return;
        _budget.ReleaseRegisters(_integerRegisters, _floatRegisters, _referenceRegisters);
        while (_frames > 0)
        {
            _budget.ExitFrame();
            _frames--;
        }
        _integerRegisters = 0;
        _floatRegisters = 0;
        _referenceRegisters = 0;
        _budget = null;
    }
}
