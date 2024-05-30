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
pub trait TuiApp {
    /// Application data.
    type Data;
    /// UI state structs.
    type State;
    /// Type for actions.
    type Action;
    /// Error type.
    type Error;
    /// Type for the theme.
    type Theme;
    /// Type for some global ui state.
    /// The example uses this for the statusbar and a message-dialog.
    /// Separating this from the ui-state helps with borrowing.
    type Global;

    /// Return a reference to the active theme.
    /// This is added to the AppContext.
    fn theme(&self) -> &Self::Theme;

    /// Initialize the application. Runs before the first repaint.
    fn init(
        &self,
        ctx: &mut AppContext<'_, Self>,
        data: &mut Self::Data,
        uistate: &mut Self::State,
    ) -> Result<(), Self::Error>;

    /// Repaint everything.
    fn repaint(
        &self,
        ctx: &mut RenderContext<'_, Self>,
        event: RepaintEvent,
        data: &mut Self::Data,
        uistate: &mut Self::State,
    ) -> Result<(), Self::Error>;

    /// Timeout event.
    fn timer(
        &self,
        ctx: &mut AppContext<'_, Self>,
        event: TimeOut,
        data: &mut Self::Data,
        uistate: &mut Self::State,
    ) -> Result<Control<Self::Action>, Self::Error>;

    /// Crossterm event.
    fn crossterm(
        &self,
        ctx: &mut AppContext<'_, Self>,
        event: crossterm::event::Event,
        data: &mut Self::Data,
        uistate: &mut Self::State,
    ) -> Result<Control<Self::Action>, Self::Error>;

    /// Run an action.
    fn action(
        &self,
        ctx: &mut AppContext<'_, Self>,
        event: Self::Action,
        data: &mut Self::Data,
        uistate: &mut Self::State,
    ) -> Result<Control<Self::Action>, Self::Error>;

    /// Run a background task.
    fn task(
        &self,
        event: Self::Action,
        results: &Sender<Result<Control<Self::Action>, Self::Error>>,
    ) -> Result<Control<Self::Action>, Self::Error>;

    /// Do error handling.
    fn error(
        &self,
        ctx: &mut AppContext<'_, Self>,
        event: Self::Error,
        data: &mut Self::Data,
        uistate: &mut Self::State,
    ) -> Result<Control<Self::Action>, Self::Error>;

    /// Runs some shutdown operations after the tui has quit.
    fn shutdown(&self, data: &mut Self::Data) -> Result<(), Self::Error>;
}

/// A collection of context data used by the application.
#[derive(Debug)]
pub struct AppContext<'a, App: TuiApp + ?Sized> {
    /// Some global data for the application.
    pub g: &'a mut App::Global,
    /// Theme data.
    pub theme: &'a App::Theme,
    /// Application timers.
    pub timers: &'a Timers,
    /// Start background tasks.
    pub tasks: &'a Sender<App::Action>,

    pub non_exhaustive: NonExhaustive,
}

/// A collection of context data used for rendering.
#[derive(Debug)]
pub struct RenderContext<'a, App: TuiApp + ?Sized> {
    /// Some global data for the application.
    pub g: &'a mut App::Global,
    /// Theme data.
    pub theme: &'a App::Theme,
    /// Application timers.
    pub timers: &'a Timers,
    /// Start background tasks.
    pub tasks: &'a Sender<App::Action>,

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
pub fn run_tui<App>(
    app: &'static App,
    data: &mut App::Data,
    uistate: &mut App::State,
    cfg: RunConfig,
) -> Result<(), App::Error>
where
    App::Action: Send + 'static,
    App::Error: Send + 'static + From<TryRecvError> + From<io::Error> + From<SendError<()>>,
    App::Global: Default,
    App: TuiApp + Sync,
{
    stdout().execute(EnterAlternateScreen)?;
    stdout().execute(EnableMouseCapture)?;
    stdout().execute(EnableBracketedPaste)?;
    stdout().execute(EnableBlinking)?;
    stdout().execute(SetCursorStyle::BlinkingBar)?;
    enable_raw_mode()?;

    let r = match catch_unwind(AssertUnwindSafe(|| _run_tui(app, data, uistate, cfg))) {
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

    app.shutdown(data)?;

    r
}

/// Run the event-loop.
fn _run_tui<App>(
    app: &'static App,
    data: &mut App::Data,
    uistate: &mut App::State,
    cfg: RunConfig,
) -> Result<(), App::Error>
where
    App::Action: Send + 'static,
    App::Error: Send + 'static + From<TryRecvError> + From<io::Error> + From<SendError<()>>,
    App::Global: Default,
    App: TuiApp + Sync,
{
    let mut term = Terminal::new(CrosstermBackend::new(stdout()))?;
    term.clear()?;

    let mut global = App::Global::default();
    let timers = Timers::default();
    let worker = ThreadPool::<App>::build(app, cfg.n_threats);

    let mut appctx = AppContext {
        g: &mut global,
        theme: app.theme(),
        timers: &timers,
        tasks: &worker.send,
        non_exhaustive: NonExhaustive,
    };

    // to not starve any event source everyone is polled and put in this queue.
    // they are not polled again before the queue is not empty.
    let mut poll_queue = VecDeque::new();
    let mut poll_sleep = Duration::from_millis(10);

    // init
    app.init(&mut appctx, data, uistate)?;

    // initial repaint.
    _ = repaint_tui(
        app,
        &mut appctx,
        &mut term,
        RepaintEvent::Repaint,
        data,
        uistate,
    )?;

    let mut flow = Ok(Control::Continue);
    let nice = 'ui: loop {
        // panic on worker panic
        if !worker.check_liveness() {
            break 'ui false;
        }

        appctx.theme = app.theme();

        flow = if matches!(flow, Ok(Control::Continue)) {
            if poll_queue.is_empty() {
                if poll_timers(app, &mut appctx) {
                    poll_queue.push_back(PollNext::Timers);
                }
                if poll_workers(app, &worker) {
                    poll_queue.push_back(PollNext::Workers);
                }
                if poll_crossterm(app)? {
                    poll_queue.push_back(PollNext::Crossterm);
                }
            }
            if poll_queue.is_empty() {
                let t = calculate_sleep(app, &mut appctx, poll_sleep);
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
                        read_timers(app, &mut appctx, &mut term, data, uistate)
                    }
                    Some(PollNext::Workers) => read_workers(app, &worker),
                    Some(PollNext::Crossterm) => read_crossterm(app, &mut appctx, data, uistate),
                }
            }
        } else {
            flow
        };

        flow = match flow {
            Ok(Control::Continue) => Ok(Control::Continue),
            Ok(Control::Break) => Ok(Control::Continue),
            Ok(Control::Repaint) => repaint_tui(
                app,
                &mut appctx,
                &mut term,
                RepaintEvent::Repaint,
                data,
                uistate,
            ),
            Ok(Control::Action(action)) => app.action(&mut appctx, action, data, uistate),
            Ok(Control::Quit) => break 'ui true,
            Err(e) => app.error(&mut appctx, e, data, uistate),
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
    app: &App,
    ctx: &mut AppContext<'_, App>,
    term: &mut Terminal<CrosstermBackend<Stdout>>,
    reason: RepaintEvent,
    data: &mut App::Data,
    uistate: &mut App::State,
) -> Result<Control<App::Action>, App::Error>
where
    App::Error: From<io::Error>,
{
    let mut res = Ok(Control::Continue);

    _ = term.hide_cursor();

    term.draw(|frame| {
        let mut ctx = RenderContext {
            g: ctx.g,
            theme: ctx.theme,
            timers: ctx.timers,
            tasks: ctx.tasks,
            counter: frame.count(),
            area: frame.size(),
            buffer: frame.buffer_mut(),
            cursor: None,
            non_exhaustive: NonExhaustive,
        };

        res = app
            .repaint(&mut ctx, reason, data, uistate)
            .map(|_| Control::Continue);

        if let Some(cursor) = ctx.cursor {
            frame.set_cursor(cursor.x, cursor.y);
        }
    })?;

    res
}

fn poll_timers<App: TuiApp>(_app: &App, ctx: &mut AppContext<'_, App>) -> bool {
    ctx.timers.poll()
}

fn read_timers<App: TuiApp>(
    app: &App,
    ctx: &mut AppContext<'_, App>,
    term: &mut Terminal<CrosstermBackend<Stdout>>,
    data: &mut App::Data,
    uistate: &mut App::State,
) -> Result<Control<App::Action>, App::Error>
where
    App::Error: From<io::Error>,
{
    match ctx.timers.read() {
        Some(TimerEvent::Repaint(evt)) => {
            repaint_tui(app, ctx, term, RepaintEvent::Timer(evt), data, uistate)
        }
        Some(TimerEvent::Application(evt)) => app.timer(ctx, evt, data, uistate),
        None => Ok(Control::Continue),
    }
}

fn poll_workers<App>(_app: &App, worker: &ThreadPool<App>) -> bool
where
    App::Action: Send + 'static,
    App::Error: Send + 'static + From<TryRecvError>,
    App: TuiApp + Sync,
{
    !worker.is_empty()
}

fn read_workers<App>(
    _app: &App,
    worker: &ThreadPool<App>,
) -> Result<Control<App::Action>, App::Error>
where
    App::Action: Send + 'static,
    App::Error: Send + 'static + From<TryRecvError>,
    App: TuiApp + Sync,
{
    worker.try_recv()
}

fn poll_crossterm<App>(_app: &App) -> Result<bool, io::Error>
where
    App: TuiApp,
{
    crossterm::event::poll(Duration::from_millis(0))
}

fn read_crossterm<App>(
    app: &App,
    ctx: &mut AppContext<'_, App>,
    data: &mut App::Data,
    uistate: &mut App::State,
) -> Result<Control<App::Action>, App::Error>
where
    App::Error: From<io::Error>,
    App: TuiApp,
{
    match crossterm::event::read() {
        Ok(evt) => app.crossterm(ctx, evt, data, uistate),
        Err(err) => Err(err.into()),
    }
}

fn calculate_sleep<App>(_app: &App, ctx: &mut AppContext<'_, App>, max: Duration) -> Duration
where
    App: TuiApp,
{
    if let Some(sleep) = ctx.timers.sleep_time() {
        min(sleep, max)
    } else {
        max
    }
}

/// Basic threadpool
#[derive(Debug)]
struct ThreadPool<App: TuiApp> {
    send: Sender<App::Action>,
    recv: Receiver<Result<Control<App::Action>, App::Error>>,
    handles: Vec<JoinHandle<()>>,
}

impl<App> ThreadPool<App>
where
    App: TuiApp + Sync,
    App::Action: 'static + Send,
    App::Error: 'static + Send,
{
    /// New threadpool with the given task executor.
    fn build(app: &'static App, n_worker: usize) -> Self {
        let (send, t_recv) = unbounded::<App::Action>();
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
                            let flow = app.task(task, &t_send);
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

impl<App> Drop for ThreadPool<App>
where
    App: TuiApp,
{
    fn drop(&mut self) {
        shutdown_thread_pool(self);
    }
}

fn shutdown_thread_pool<App>(t: &mut ThreadPool<App>)
where
    App: TuiApp,
{
    drop(mem::replace(&mut t.send, bounded(0).0));
    for h in t.handles.drain(..) {
        _ = h.join();
    }
}
