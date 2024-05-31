use crate::_private::NonExhaustive;
use crate::{Control, RepaintEvent, TimeOut, TimerEvent, Timers};
use crossbeam::channel::{bounded, unbounded, Receiver, SendError, Sender, TryRecvError};
use crossterm::cursor::{DisableBlinking, EnableBlinking, SetCursorStyle};
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use log::debug;
use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::Terminal;
use std::cmp::min;
use std::collections::VecDeque;
use std::fmt::Debug;
use std::io::{stdout, Stdout};
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::thread::{sleep, JoinHandle};
use std::time::Duration;
use std::{io, mem, thread};

/// A trait for a TUI-App.
///
/// The trait is only needed to collect all the involved types.
/// It can be easily implemented for a unit-struct.
///
/// ```rust ignore
/// struct MyApp;
///
/// impl TuiApp for MyApp {
///     // ...
/// }
/// ```
///
/// All operations work with the [AppWidget] and [AppEvents] traits.
///
pub trait TuiApp: AppWidget<Self> + Sync + Debug {
    /// Type for some global ui state.
    /// The example uses this for the statusbar and a message-dialog.
    /// Or any other uistate that must be reachable from every app-widget.
    type Global: Debug;

    /// Application data.
    type Data: Debug;

    /// Type for actions
    type Action: Debug;
    /// Error type.
    type Error: Debug;

    /// Type for the theme.
    type Theme: Debug;
}

/// Type for a background task.
type Task<Action, Error> = Box<
    dyn FnOnce(&Sender<Result<Control<Action>, Error>>) -> Result<Control<Action>, Error> + Send,
>;

/// A trait for application level widgets.
/// Implement on a uistate struct.
#[allow(unused_variables)]
pub trait AppWidget<App: TuiApp + Sync + ?Sized> {
    type State: AppEvents<App> + Debug;

    /// Renders an application widget.
    fn render(
        &mut self,
        ctx: &mut RenderContext<'_, App>,
        event: &RepaintEvent,
        area: Rect,
        data: &mut App::Data,
        state: &mut Self::State,
    ) -> Result<(), App::Error> {
        Ok(())
    }
}

#[allow(unused_variables)]
pub trait AppEvents<App: TuiApp + Sync + ?Sized> {
    /// Initialize the application. Runs before the first repaint.
    fn init(
        &mut self,
        ctx: &mut AppContext<'_, App>,
        data: &mut App::Data,
    ) -> Result<(), App::Error> {
        Ok(())
    }

    /// Timeout event.
    fn timer(
        &mut self,
        ctx: &mut AppContext<'_, App>,
        event: &TimeOut,
        data: &mut App::Data,
    ) -> Result<Control<App::Action>, App::Error> {
        Ok(Control::Continue)
    }

    /// Crossterm event.
    fn crossterm(
        &mut self,
        ctx: &mut AppContext<'_, App>,
        event: &crossterm::event::Event,
        data: &mut App::Data,
    ) -> Result<Control<App::Action>, App::Error> {
        Ok(Control::Continue)
    }

    /// Run an action.
    fn action(
        &mut self,
        ctx: &mut AppContext<'_, App>,
        event: App::Action,
        data: &mut App::Data,
    ) -> Result<Control<App::Action>, App::Error> {
        Ok(Control::Continue)
    }

    /// Do error handling.
    fn error(
        &self,
        ctx: &mut AppContext<'_, App>,
        event: App::Error,
        data: &mut App::Data,
    ) -> Result<Control<App::Action>, App::Error> {
        Ok(Control::Continue)
    }
}

/// A collection of context data used by the application.
#[derive(Debug)]
pub struct AppContext<'a, App: TuiApp + Sync + ?Sized> {
    /// Some global state for the application.
    pub g: &'a mut App::Global,
    /// Theme data.
    pub theme: &'a App::Theme,
    /// Application timers.
    pub timers: &'a Timers,
    /// Start background tasks.
    pub tasks: &'a Sender<Task<App::Action, App::Error>>,

    pub non_exhaustive: NonExhaustive,
}

/// A collection of context data used for rendering.
#[derive(Debug)]
pub struct RenderContext<'a, App: TuiApp + Sync + ?Sized> {
    /// Some global state for the application.
    ///
    /// This needs to be a plain & for [clone_with](RenderContext::clone_with).
    /// If you need to modify it, use Cell or RefCell.
    pub g: &'a App::Global,
    /// Theme data.
    pub theme: &'a App::Theme,
    /// Application timers.
    pub timers: &'a Timers,
    /// Start background tasks.
    pub tasks: &'a Sender<Task<App::Action, App::Error>>,

    /// Frame counter.
    pub counter: usize,
    /// Frame area.
    pub area: Rect,
    /// Buffer.
    pub buffer: &'a mut Buffer,
    /// Output cursor position. Set after rendering is complete.
    pub cursor: Option<Position>,

    pub non_exhaustive: NonExhaustive,
}

impl<'a, App: TuiApp + Sync + ?Sized> RenderContext<'a, App> {
    /// Clone with a new buffer.
    pub fn clone_with(&self, buffer: &'a mut Buffer) -> Self {
        Self {
            g: self.g,
            theme: self.theme,
            timers: self.timers,
            tasks: self.tasks,
            counter: 0,
            area: buffer.area,
            buffer,
            cursor: None,
            non_exhaustive: NonExhaustive,
        }
    }
}

enum PollNext {
    Timers,
    Workers,
    Crossterm,
}

/// Captures some parameters for [run_tui()].
#[derive(Debug)]
pub struct RunConfig {
    /// How many worker threads are wanted?
    /// Most of the time 1 should be sufficient to offload any gui-blocking tasks.
    pub n_threats: usize,
}

impl Default for RunConfig {
    fn default() -> Self {
        Self { n_threats: 1 }
    }
}

/// Run the event-loop
pub fn run_tui<App: TuiApp>(
    app: App,
    global: &mut App::Global,
    theme: &App::Theme,
    data: &mut App::Data,
    state: &mut App::State,
    cfg: RunConfig,
) -> Result<(), App::Error>
where
    App::Action: Send + 'static,
    App::Error: Send + 'static + From<TryRecvError> + From<io::Error> + From<SendError<()>>,
{
    stdout().execute(EnterAlternateScreen)?;
    stdout().execute(EnableMouseCapture)?;
    stdout().execute(EnableBracketedPaste)?;
    stdout().execute(EnableBlinking)?;
    stdout().execute(SetCursorStyle::BlinkingBar)?;
    enable_raw_mode()?;

    let r = match catch_unwind(AssertUnwindSafe(|| {
        _run_tui(app, global, theme, data, state, cfg)
    })) {
        Ok(v) => v,
        Err(e) => {
            _ = disable_raw_mode();
            _ = stdout().execute(SetCursorStyle::DefaultUserShape);
            _ = stdout().execute(DisableBlinking);
            _ = stdout().execute(DisableBracketedPaste);
            _ = stdout().execute(DisableMouseCapture);
            _ = stdout().execute(LeaveAlternateScreen);

            resume_unwind(e);
        }
    };

    disable_raw_mode()?;
    stdout().execute(SetCursorStyle::DefaultUserShape)?;
    stdout().execute(DisableBlinking)?;
    stdout().execute(DisableBracketedPaste)?;
    stdout().execute(DisableMouseCapture)?;
    stdout().execute(LeaveAlternateScreen)?;

    r
}

/// Run the event-loop.
fn _run_tui<App: TuiApp>(
    mut app: App,
    global: &mut App::Global,
    theme: &App::Theme,
    data: &mut App::Data,
    state: &mut App::State,
    cfg: RunConfig,
) -> Result<(), App::Error>
where
    App::Action: Send + 'static,
    App::Error: Send + 'static + From<TryRecvError> + From<io::Error> + From<SendError<()>>,
{
    let mut term = Terminal::new(CrosstermBackend::new(stdout()))?;
    term.clear()?;

    let timers = Timers::default();
    let worker = ThreadPool::<App>::build(cfg.n_threats);

    let mut appctx = AppContext {
        g: global,
        theme,
        timers: &timers,
        tasks: &worker.send,
        non_exhaustive: NonExhaustive,
    };

    // to not starve any event source everyone is polled and put in this queue.
    // they are not polled again before the queue is not empty.
    let mut poll_queue = VecDeque::new();
    let mut poll_sleep = Duration::from_millis(10);

    // init
    state.init(&mut appctx, data)?;

    // initial repaint.
    _ = repaint_tui(
        &mut app,
        &mut appctx,
        &mut term,
        RepaintEvent::Repaint,
        data,
        state,
    )?;

    let mut flow = Ok(Control::Continue);
    let nice = 'ui: loop {
        // panic on worker panic
        if !worker.check_liveness() {
            break 'ui false;
        }

        flow = if matches!(flow, Ok(Control::Continue)) {
            if poll_queue.is_empty() {
                if poll_timers(&mut appctx) {
                    poll_queue.push_back(PollNext::Timers);
                }
                if poll_workers(&worker) {
                    poll_queue.push_back(PollNext::Workers);
                }
                if poll_crossterm()? {
                    poll_queue.push_back(PollNext::Crossterm);
                }
            }
            if poll_queue.is_empty() {
                let t = calculate_sleep(&mut appctx, poll_sleep);
                sleep(t);
                if poll_sleep < Duration::from_millis(10) {
                    // Back off slowly.
                    poll_sleep += Duration::from_micros(100);
                }
                Ok(Control::Continue)
            } else {
                // short sleep after a series of successfull polls.
                //
                // there can be some delay before consecutive events are available to
                // poll_crossterm().
                // this happens with windows-terminal, which pastes by sending each char as a
                // key-event. the normal sleep interval is noticeable in that case. with
                // the shorter sleep it's still not instantaneous but ok-ish.
                // For all other cases 10ms seems to work fine.
                // Note: could make this configurable too.
                poll_sleep = Duration::from_micros(100);

                match poll_queue.pop_front() {
                    None => Ok(Control::Continue),
                    Some(PollNext::Timers) => {
                        read_timers(&mut app, &mut appctx, &mut term, data, state)
                    }
                    Some(PollNext::Workers) => read_workers(&worker),
                    Some(PollNext::Crossterm) => read_crossterm(&mut appctx, data, state),
                }
            }
        } else {
            flow
        };

        flow = match flow {
            Ok(Control::Continue) => Ok(Control::Continue),
            Ok(Control::Break) => Ok(Control::Continue),
            Ok(Control::Repaint) => repaint_tui(
                &mut app,
                &mut appctx,
                &mut term,
                RepaintEvent::Repaint,
                data,
                state,
            ),
            Ok(Control::Action(action)) => state.action(&mut appctx, action, data),
            Ok(Control::Quit) => break 'ui true,
            Err(e) => state.error(&mut appctx, e, data),
        }
    };

    if nice {
        worker.stop_and_join();
    } else {
        worker.stop_and_join();
        panic!("worker panicked");
    }

    Ok(())
}

fn repaint_tui<App: TuiApp>(
    app: &mut App,
    ctx: &mut AppContext<'_, App>,
    term: &mut Terminal<CrosstermBackend<Stdout>>,
    reason: RepaintEvent,
    data: &mut App::Data,
    state: &mut App::State,
) -> Result<Control<App::Action>, App::Error>
where
    App::Error: From<io::Error>,
{
    let mut res = Ok(Control::Continue);

    _ = term.hide_cursor();

    term.draw(|frame| {
        let frame_area = frame.size();
        let mut ctx = RenderContext {
            g: ctx.g,
            theme: ctx.theme,
            timers: ctx.timers,
            tasks: ctx.tasks,
            counter: frame.count(),
            area: frame_area,
            buffer: frame.buffer_mut(),
            cursor: None,
            non_exhaustive: NonExhaustive,
        };

        res = app
            .render(&mut ctx, &reason, frame_area, data, state)
            .map(|_| Control::Continue);

        if let Some(cursor) = ctx.cursor {
            frame.set_cursor(cursor.x, cursor.y);
        }
    })?;

    res
}

fn poll_timers<App: TuiApp>(ctx: &mut AppContext<'_, App>) -> bool {
    ctx.timers.poll()
}

fn read_timers<App: TuiApp>(
    app: &mut App,
    ctx: &mut AppContext<'_, App>,
    term: &mut Terminal<CrosstermBackend<Stdout>>,
    data: &mut App::Data,
    state: &mut App::State,
) -> Result<Control<App::Action>, App::Error>
where
    App::Error: From<io::Error>,
{
    match ctx.timers.read() {
        Some(TimerEvent::Repaint(evt)) => {
            repaint_tui(app, ctx, term, RepaintEvent::Timer(evt), data, state)
        }
        Some(TimerEvent::Application(evt)) => state.timer(ctx, &evt, data),
        None => Ok(Control::Continue),
    }
}

fn poll_workers<App: TuiApp>(worker: &ThreadPool<App>) -> bool
where
    App::Action: Send + 'static,
    App::Error: Send + 'static + From<TryRecvError>,
{
    !worker.is_empty()
}

fn read_workers<App: TuiApp + Sync + ?Sized>(
    worker: &ThreadPool<App>,
) -> Result<Control<App::Action>, App::Error>
where
    App::Action: Send + 'static,
    App::Error: Send + 'static + From<TryRecvError>,
{
    worker.try_recv()
}

fn poll_crossterm() -> Result<bool, io::Error> {
    crossterm::event::poll(Duration::from_millis(0))
}

fn read_crossterm<App: TuiApp + Sync + ?Sized>(
    ctx: &mut AppContext<'_, App>,
    data: &mut App::Data,
    state: &mut App::State,
) -> Result<Control<App::Action>, App::Error>
where
    App::Error: From<io::Error>,
{
    match crossterm::event::read() {
        Ok(evt) => state.crossterm(ctx, &evt, data),
        Err(err) => Err(err.into()),
    }
}

fn calculate_sleep<App: TuiApp>(ctx: &mut AppContext<'_, App>, max: Duration) -> Duration {
    if let Some(sleep) = ctx.timers.sleep_time() {
        min(sleep, max)
    } else {
        max
    }
}

/// Basic threadpool
#[derive(Debug)]
struct ThreadPool<App: TuiApp + ?Sized> {
    send: Sender<Task<App::Action, App::Error>>,
    recv: Receiver<Result<Control<App::Action>, App::Error>>,
    handles: Vec<JoinHandle<()>>,
}

impl<App: TuiApp + Sync + ?Sized> ThreadPool<App>
where
    App::Action: 'static + Send,
    App::Error: 'static + Send,
{
    /// New threadpool with the given task executor.
    fn build(n_worker: usize) -> Self {
        let (send, t_recv) = unbounded::<Task<App::Action, App::Error>>();
        let (t_send, recv) = unbounded::<Result<Control<App::Action>, App::Error>>();

        let mut handles = Vec::new();

        for _ in 0..n_worker {
            let t_recv = t_recv.clone();
            let t_send = t_send.clone();

            let handle = thread::spawn(move || {
                let t_recv = t_recv;

                'l: loop {
                    match t_recv.recv() {
                        Ok(task) => {
                            let flow = task(&t_send);
                            if let Err(err) = t_send.send(flow) {
                                debug!("{:?}", err);
                                break 'l;
                            }
                        }
                        Err(err) => {
                            debug!("{:?}", err);
                            break 'l;
                        }
                    }
                }
            });

            handles.push(handle);
        }

        Self {
            send,
            recv,
            handles,
        }
    }

    /// Check the workers for liveness.
    ///
    /// Panic:
    /// Panics if any of the workers panicked themselves.
    fn check_liveness(&self) -> bool {
        for h in &self.handles {
            if h.is_finished() {
                return false;
            }
        }
        true
    }

    /// Is the channel empty?
    fn is_empty(&self) -> bool {
        self.recv.is_empty()
    }

    /// Receive a result.
    fn try_recv(&self) -> Result<Control<App::Action>, App::Error>
    where
        App::Error: From<TryRecvError>,
    {
        match self.recv.try_recv() {
            Ok(v) => v,
            Err(TryRecvError::Empty) => Ok(Control::Continue),
            Err(e) => Err(e.into()),
        }
    }

    /// Stop threads and join.
    fn stop_and_join(mut self) {
        shutdown_thread_pool(&mut self);
    }
}

impl<App: TuiApp + Sync + ?Sized> Drop for ThreadPool<App> {
    fn drop(&mut self) {
        shutdown_thread_pool(self);
    }
}

fn shutdown_thread_pool<App: TuiApp + Sync + ?Sized>(t: &mut ThreadPool<App>) {
    drop(mem::replace(&mut t.send, bounded(0).0));
    for h in t.handles.drain(..) {
        _ = h.join();
    }
}
