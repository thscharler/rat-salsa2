use rat_event::util::Outcome;

mod framework;
mod timer;

pub use framework::{run_tui, RunConfig, TuiApp};
use rat_widget::menuline::MenuOutcome;
pub use timer::{TimeOut, TimerDef, TimerEvent, Timers};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[must_use]
pub enum Control<Action> {
    /// Continue operations.
    Continue,
    /// Break handling the current event.
    Break,
    /// Trigger a repaint.
    Repaint,
    /// Trigger an action.
    Action(Action),
    /// Quit the application.
    Quit,
}

/// Breaks the control-flow.
/// If the value is not [Control::Continue] this returns early.
#[macro_export]
macro_rules! flow {
    ($x:expr) => {{
        let r = $x;
        if !matches!(r, Control::Continue) {
            return Ok(r);
        } else {
            _ = r;
        }
    }};
}

impl<Action> From<Outcome> for Control<Action> {
    fn from(value: Outcome) -> Self {
        match value {
            Outcome::NotUsed => Control::Continue,
            Outcome::Unchanged => Control::Break,
            Outcome::Changed => Control::Repaint,
        }
    }
}

impl<Action> From<MenuOutcome> for Control<Action> {
    fn from(value: MenuOutcome) -> Self {
        match value {
            MenuOutcome::NotUsed => Control::Continue,
            MenuOutcome::Unchanged => Control::Break,
            MenuOutcome::Changed => Control::Repaint,
            MenuOutcome::Selected(_) => Control::Repaint,
            MenuOutcome::Activated(_) => Control::Repaint,
        }
    }
}

/// Gives some extra information why a repaint was triggered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepaintEvent {
    /// There was a [ControlUI::Change](crate::ControlUI::Change) or the change flag has been set.
    Change,
    /// A timer triggered this.
    Timer(TimeOut),
}
