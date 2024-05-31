use crate::event::RepaintEvent;
use rat_widget::button::ButtonOutcome;
use rat_widget::event::{ConsumedEvent, Outcome, TextOutcome};
use rat_widget::menuline::MenuOutcome;
use std::mem;

mod framework;
mod timer;

pub use framework::{run_tui, AppContext, AppEvents, AppWidget, RenderContext, RunConfig, TuiApp};
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

impl<Action> ConsumedEvent for Control<Action> {
    fn is_consumed(&self) -> bool {
        mem::discriminant(self) != mem::discriminant(&Control::Continue)
    }
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

impl<Action> From<ButtonOutcome> for Control<Action> {
    fn from(value: ButtonOutcome) -> Self {
        match value {
            ButtonOutcome::NotUsed => Control::Continue,
            ButtonOutcome::Unchanged => Control::Break,
            ButtonOutcome::Changed => Control::Repaint,
            ButtonOutcome::Pressed => Control::Repaint,
        }
    }
}

impl<Action> From<TextOutcome> for Control<Action> {
    fn from(value: TextOutcome) -> Self {
        match value {
            TextOutcome::NotUsed => Control::Continue,
            TextOutcome::Unchanged => Control::Break,
            TextOutcome::Changed => Control::Repaint,
            TextOutcome::TextChanged => Control::Repaint,
        }
    }
}

pub mod event {
    //!
    //! Event-handler traits and Keybindings.
    //!

    use crate::TimeOut;
    pub use rat_widget::event::{
        crossterm, ct_event, util, ConsumedEvent, FocusKeys, HandleEvent, MouseOnly, Outcome,
    };

    /// Gives some extra information why a repaint was triggered.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum RepaintEvent {
        /// There was a [Repaint](crate::Control::Repaint) or the change flag has been set.
        Repaint,
        /// A timer triggered this.
        Timer(TimeOut),
    }
}

mod _private {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct NonExhaustive;
}
