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
use ratatui::{Frame, Terminal};
use std::cmp::min;
use std::collections::VecDeque;
use std::fmt::Debug;
use std::io::{stdout, Stdout};
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::thread::{sleep, JoinHandle};
use std::time::Duration;
use std::{io, mem, thread};

pub trait TuiApp {
    type Data;
    type State;
    type Action;
    type Error;

    /// Get the timer state for this uistate.
    #[allow(unused_variables)]
    fn get_timers<'b>(&self, uistate: &'b Self::State) -> Option<&'b Timers> {
        None
    }

    /// Initialize the application. Runs before the first repaint.
    fn init(
        &self,
        data: &mut Self::Data,
        uistate: &mut Self::State,
        send: &Sender<Self::Action>,
    ) -> Result<(), Self::Error>;

    /// Repaint everything.
    fn repaint(
        &self,
        frame: &mut Frame<'_>,
        event: RepaintEvent,
        data: &mut Self::Data,
        uistate: &mut Self::State,
    ) -> Result<(), Self::Error>;

    /// Timeout event.
    fn timer(
        &self,
        event: TimeOut,
        data: &mut Self::Data,
        uistate: &mut Self::State,
        send: &Sender<Self::Action>,
    ) -> Result<Control<Self::Action>, Self::Error>;

    /// Crossterm event.
    fn crossterm(
        &self,
        event: crossterm::event::Event,
        data: &mut Self::Data,
        uistate: &mut Self::State,
        send: &Sender<Self::Action>,
    ) -> Result<Control<Self::Action>, Self::Error>;

    /// Run an action.
    fn action(
        &self,
        event: Self::Action,
        data: &mut Self::Data,
        uistate: &mut Self::State,
        send: &Sender<Self::Action>,
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
        event: Self::Error,
        data: &mut Self::Data,
        uistate: &mut Self::State,
        send: &Sender<Self::Action>,
    ) -> Result<Control<Self::Action>, Self::Error>;

    /// Runs some shutdown operations after the tui has quit.
    fn shutdown(&self, data: &mut Self::Data) -> Result<(), Self::Error>;
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
    app: &'static App,
    data: &mut App::Data,
    uistate: &mut App::State,
    cfg: RunConfig,
) -> Result<(), App::Error>
where
    App::Action: Send + 'static,
    App::Error: Send + 'static + From<TryRecvError> + From<io::Error> + From<SendError<()>>,
    App: Sync,
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
fn _run_tui<App: TuiApp>(
    app: &'static App,
    data: &mut App::Data,
    uistate: &mut App::State,
    cfg: RunConfig,
) -> Result<(), App::Error>
where
    App::Action: Send + 'static,
    App::Error: Send + 'static + From<TryRecvError> + From<io::Error> + From<SendError<()>>,
    App: Sync,
{
    let mut term = Terminal::new(CrosstermBackend::new(stdout()))?;
    term.clear()?;

    let mut worker = ThreadPool::<App>::build(app, cfg.n_threats);

    // to not starve any event source everyone is polled and put in this queue.
    // they are not polled again before the queue is not empty.
    let mut poll_queue = VecDeque::new();
    let mut poll_sleep = 10;

    // init
    match app.init(data, uistate, &worker.send) {
        Err(err) => {
            return Err(err);
        }
        _ => {}
    };

    // initial repaint.
    _ = repaint_tui(&mut term, app, data, uistate, RepaintEvent::Change)?;

    let mut flow = Ok(Control::Continue);
    'ui: loop {
        // panic on worker panic
        worker.check_liveness();

        flow = if matches!(flow, Ok(Control::Continue)) {
            if poll_queue.is_empty() {
                if poll_timers(app, uistate) {
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
                let t = calculate_sleep(app, uistate, Duration::from_millis(poll_sleep));
                sleep(t);
                if poll_sleep < 10 {
                    poll_sleep = 10;
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
                poll_sleep = 0;

                match poll_queue.pop_front() {
                    None => Ok(Control::Continue),
                    Some(PollNext::Timers) => {
                        read_timers(&mut term, app, data, uistate, &worker.send)
                    }
                    Some(PollNext::Workers) => read_workers(app, &worker),
                    Some(PollNext::Crossterm) => read_crossterm(app, data, uistate, &worker.send),
                }
            }
        } else {
            flow
        };

        flow = match flow {
            Ok(Control::Continue) => Ok(Control::Continue),
            Ok(Control::Break) => Ok(Control::Continue),
            Ok(Control::Repaint) => {
                repaint_tui(&mut term, app, data, uistate, RepaintEvent::Change)
            }
            Ok(Control::Action(action)) => app.action(action, data, uistate, &worker.send),
            Ok(Control::Quit) => break 'ui,
            Err(e) => app.error(e, data, uistate, &worker.send),
        }
    }

    worker.stop_and_join();

    Ok(())
}

fn repaint_tui<App: TuiApp>(
    term: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &App,
    data: &mut App::Data,
    uistate: &mut App::State,
    reason: RepaintEvent,
) -> Result<Control<App::Action>, App::Error>
where
    App::Error: From<io::Error>,
{
    let mut res = Ok(Control::Continue);

    _ = term.hide_cursor();

    term.draw(|frame| {
        res = app
            .repaint(frame, reason, data, uistate)
            .map(|_| Control::Continue);
    })?;

    res
}

fn poll_timers<App: TuiApp>(app: &App, uistate: &mut App::State) -> bool {
    if let Some(timers) = app.get_timers(uistate) {
        timers.poll()
    } else {
        false
    }
}

fn read_timers<App: TuiApp>(
    term: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &App,
    data: &mut App::Data,
    uistate: &mut App::State,
    send: &Sender<App::Action>,
) -> Result<Control<App::Action>, App::Error>
where
    App::Error: From<io::Error>,
{
    if let Some(timers) = app.get_timers(uistate) {
        match timers.read() {
            Some(TimerEvent::Repaint(evt)) => {
                repaint_tui(term, app, data, uistate, RepaintEvent::Timer(evt))
            }
            Some(TimerEvent::Application(evt)) => app.timer(evt, data, uistate, send),
            None => Ok(Control::Continue),
        }
    } else {
        Ok(Control::Continue)
    }
}

fn poll_workers<App: TuiApp>(_app: &App, worker: &ThreadPool<App>) -> bool
where
    App::Action: Send + 'static,
    App::Error: Send + 'static + From<TryRecvError>,
    App: Sync,
{
    !worker.is_empty()
}

fn read_workers<App: TuiApp>(
    _app: &App,
    worker: &ThreadPool<App>,
) -> Result<Control<App::Action>, App::Error>
where
    App::Action: Send + 'static,
    App::Error: Send + 'static + From<TryRecvError>,
    App: Sync,
{
    worker.try_recv()
}

fn poll_crossterm<App: TuiApp>(_app: &App) -> Result<bool, io::Error> {
    crossterm::event::poll(Duration::from_millis(0))
}

fn read_crossterm<App: TuiApp>(
    app: &App,
    data: &mut App::Data,
    uistate: &mut App::State,
    send: &Sender<App::Action>,
) -> Result<Control<App::Action>, App::Error>
where
    App::Error: From<io::Error>,
{
    match crossterm::event::read() {
        Ok(evt) => app.crossterm(evt, data, uistate, send),
        Err(err) => Err(err.into()),
    }
}

fn calculate_sleep<App: TuiApp>(app: &App, uistate: &mut App::State, max: Duration) -> Duration {
    if let Some(timers) = app.get_timers(uistate) {
        if let Some(sleep) = timers.sleep_time() {
            min(sleep, max)
        } else {
            max
        }
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

impl<App: TuiApp> ThreadPool<App>
where
    App: Sync,
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
    fn check_liveness(&mut self) {
        let mut all_alive = true;
        for h in &self.handles {
            if h.is_finished() {
                all_alive = false;
            }
        }

        if !all_alive {
            shutdown_thread_pool(self);
            panic!("worker panicked");
        }
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

impl<App: TuiApp> Drop for ThreadPool<App> {
    fn drop(&mut self) {
        shutdown_thread_pool(self);
    }
}

fn shutdown_thread_pool<App: TuiApp>(t: &mut ThreadPool<App>) {
    drop(mem::replace(&mut t.send, bounded(0).0));
    for h in t.handles.drain(..) {
        _ = h.join();
    }
}
