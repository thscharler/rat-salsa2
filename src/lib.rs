use rat_event::util::Outcome;
use rat_event::UsedEvent;
use rat_widget::menuline::MenuOutcome;
use ratatui::layout::Rect;

mod framework;
mod timer;

pub use framework::{run_tui, AppContext, RenderContext, RunConfig, TuiApp};
use rat_widget::button::ButtonOutcome;
use rat_widget::event::TextOutcome;
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

impl<Action> UsedEvent for Control<Action> {
    fn used_event(&self) -> bool {
        true
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

/// Gives some extra information why a repaint was triggered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepaintEvent {
    /// There was a [ControlUI::Change](crate::ControlUI::Change) or the change flag has been set.
    Change,
    /// A timer triggered this.
    Timer(TimeOut),
}

/// I like traits. Therefore, one more for some application level widget.
pub trait AppWidget<App: TuiApp> {
    /// Renders an application widget.
    fn render<'a>(
        &self,
        ctx: &mut RenderContext<'a, App>,
        event: RepaintEvent,
        area: Rect,
        data: &mut App::Data,
        uistate: &mut App::State,
    ) -> Result<(), App::Error>;
}

mod _private {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct NonExhaustive;
}
