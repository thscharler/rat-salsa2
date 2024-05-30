#![allow(unused_variables)]

use crossbeam::channel::Sender;
use crossterm::event::Event;
use rat_salsa2::event::RepaintEvent;
use rat_salsa2::{flow, run_tui, AppContext, Control, RenderContext, RunConfig, TimeOut, TuiApp};
use rat_widget::button::ButtonStyle;
use rat_widget::event::{ct_event, FocusKeys, HandleEvent};
use rat_widget::input::TextInputStyle;
use rat_widget::masked_input::MaskedInputStyle;
use rat_widget::menuline::{MenuLine, MenuLineState, MenuOutcome, MenuStyle};
use rat_widget::msgdialog::{MsgDialog, MsgDialogState, MsgDialogStyle};
use rat_widget::statusline::{StatusLine, StatusLineState};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::{Color, Style};
use ratatui::style::Stylize;
use ratatui::widgets::StatefulWidget;
use std::time::{Duration, SystemTime};

fn main() -> Result<(), anyhow::Error> {
    setup_logging()?;

    let mut data = MinimalData::default();
    let mut state = MinimalState::default();

    run_tui(
        &MinimalApp,
        &mut data,
        &mut state,
        RunConfig {
            n_threats: 1,
            ..RunConfig::default()
        },
    )?;

    Ok(())
}

// -----------------------------------------------------------------------

type AppResult = Result<Control<MinimalAction>, anyhow::Error>;

#[derive(Debug, Default)]
pub struct MinimalData {}

#[derive(Debug)]
pub enum MinimalAction {}

#[derive(Debug, Default)]
pub struct MinimalState {
    pub mask0: Mask0,
}

#[derive(Debug)]
pub struct GlobalState {
    pub status: StatusLineState,
    pub error_dlg: MsgDialogState,
}

#[derive(Debug)]
pub struct Mask0 {
    pub menu: MenuLineState,
}

impl Default for GlobalState {
    fn default() -> Self {
        Self {
            status: Default::default(),
            error_dlg: Default::default(),
        }
    }
}

impl Default for Mask0 {
    fn default() -> Self {
        let s = Self {
            menu: Default::default(),
        };
        s.menu.focus.set();
        s
    }
}

// -----------------------------------------------------------------------

#[derive(Debug)]
pub struct MinimalApp;

#[derive(Debug, Clone, Copy)]
pub struct MinimalAppLayout {
    area: Rect,
    menu: Rect,
    status: Rect,
}

impl TuiApp for MinimalApp {
    type Data = MinimalData;
    type State = MinimalState;
    type Action = MinimalAction;
    type Error = anyhow::Error;
    type Theme = Theme;
    type Global = GlobalState;

    fn theme(&self) -> &Self::Theme {
        &ONEDARK
    }

    fn init(
        &self,
        ctx: &mut AppContext<'_, Self>,
        data: &mut Self::Data,
        uistate: &mut Self::State,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    fn repaint(
        &self,
        ctx: &mut RenderContext<'_, MinimalApp>,
        event: RepaintEvent,
        data: &mut Self::Data,
        uistate: &mut Self::State,
    ) -> Result<(), Self::Error> {
        let t0 = SystemTime::now();

        let layout = {
            let r = Layout::new(
                Direction::Vertical,
                [
                    Constraint::Fill(1),
                    Constraint::Length(1),
                    Constraint::Length(1),
                ],
            )
            .split(ctx.area);

            MinimalAppLayout {
                area: r[0],
                menu: r[1],
                status: r[2],
            }
        };

        repaint_mask0(&event, ctx, layout, data, uistate)?;

        if ctx.g.error_dlg.active {
            let err = MsgDialog::new().styles(ctx.theme.status_dialog_style());
            err.render(layout.area, ctx.buffer, &mut ctx.g.error_dlg);
        }

        let el = t0.elapsed().unwrap_or(Duration::from_nanos(0));
        ctx.g.status.status(1, format!("R {:.3?}", el).to_string());

        let status = StatusLine::new()
            .layout([
                Constraint::Fill(1),
                Constraint::Length(12),
                Constraint::Length(12),
                Constraint::Length(12),
            ])
            .styles(ctx.theme.statusline_style());
        status.render(layout.status, ctx.buffer, &mut ctx.g.status);

        Ok(())
    }

    fn timer(
        &self,
        ctx: &mut AppContext<'_, MinimalApp>,
        event: TimeOut,
        data: &mut Self::Data,
        uistate: &mut Self::State,
    ) -> AppResult {
        Ok(Control::Continue)
    }

    fn crossterm(
        &self,
        ctx: &mut AppContext<'_, MinimalApp>,
        event: Event,
        data: &mut Self::Data,
        uistate: &mut Self::State,
    ) -> AppResult {
        let t0 = SystemTime::now();

        use crossterm::event::*;

        flow!(match &event {
            Event::Resize(_, _) => Control::Repaint,
            ct_event!(key press CONTROL-'q') => Control::Quit,
            _ => Control::Continue,
        });

        flow!({
            if ctx.g.error_dlg.active {
                ctx.g.error_dlg.handle(&event, FocusKeys).into()
            } else {
                Control::Continue
            }
        });

        flow!(handle_mask0(ctx, &event, data, uistate)?);

        let el = t0.elapsed().unwrap_or(Duration::from_nanos(0));
        ctx.g.status.status(2, format!("H {:.3?}", el).to_string());

        Ok(Control::Continue)
    }

    fn action(
        &self,
        ctx: &mut AppContext<'_, MinimalApp>,
        event: Self::Action,
        data: &mut Self::Data,
        uistate: &mut Self::State,
    ) -> AppResult {
        let t0 = SystemTime::now();

        // TODO: actions

        let el = t0.elapsed().unwrap_or(Duration::from_nanos(0));
        ctx.g.status.status(3, format!("A {:.3?}", el).to_string());

        Ok(Control::Continue)
    }

    fn task(
        &self,
        event: Self::Action,
        results: &Sender<Result<Control<Self::Action>, Self::Error>>,
    ) -> AppResult {
        // TODO: tasks
        Ok(Control::Continue)
    }

    fn error(
        &self,
        ctx: &mut AppContext<'_, Self>,
        event: Self::Error,
        data: &mut Self::Data,
        uistate: &mut Self::State,
    ) -> AppResult {
        ctx.g.error_dlg.append(format!("{:?}", &*event).as_str());
        Ok(Control::Repaint)
    }

    fn shutdown(&self, data: &mut Self::Data) -> Result<(), Self::Error> {
        Ok(())
    }
}

fn repaint_mask0(
    event: &RepaintEvent,
    ctx: &mut RenderContext<'_, MinimalApp>,
    layout: MinimalAppLayout,
    data: &mut MinimalData,
    uistate: &mut MinimalState,
) -> Result<(), anyhow::Error> {
    // TODO: repaint_mask

    let menu = MenuLine::new().styles(ctx.theme.menu_style()).add("_Quit");
    menu.render(layout.menu, ctx.buffer, &mut uistate.mask0.menu);

    Ok(())
}

fn handle_mask0(
    ctx: &mut AppContext<'_, MinimalApp>,
    event: &Event,
    data: &mut MinimalData,
    uistate: &mut MinimalState,
) -> AppResult {
    let mask0 = &mut uistate.mask0;

    // TODO: handle_mask
    flow!(match mask0.menu.handle(event, FocusKeys) {
        MenuOutcome::Activated(0) => {
            Control::Quit
        }
        v => v.into(),
    });

    Ok(Control::Continue)
}

// -----------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct Theme {
    pub name: &'static str,
    pub dark_theme: bool,

    pub white: Color,
    pub darker_black: Color,
    pub black: Color,
    pub black2: Color,
    pub one_bg: Color,
    pub one_bg2: Color,
    pub one_bg3: Color,
    pub grey: Color,
    pub grey_fg: Color,
    pub grey_fg2: Color,
    pub light_grey: Color,
    pub red: Color,
    pub baby_pink: Color,
    pub pink: Color,
    pub line: Color,
    pub green: Color,
    pub vibrant_green: Color,
    pub nord_blue: Color,
    pub blue: Color,
    pub yellow: Color,
    pub sun: Color,
    pub purple: Color,
    pub dark_purple: Color,
    pub teal: Color,
    pub orange: Color,
    pub cyan: Color,
    pub statusline_bg: Color,
    pub lightbg: Color,
    pub pmenu_bg: Color,
    pub folder_bg: Color,

    pub base00: Color,
    pub base01: Color,
    pub base02: Color,
    pub base03: Color,
    pub base04: Color,
    pub base05: Color,
    pub base06: Color,
    pub base07: Color,
    pub base08: Color,
    pub base09: Color,
    pub base0a: Color,
    pub base0b: Color,
    pub base0c: Color,
    pub base0d: Color,
    pub base0e: Color,
    pub base0f: Color,
}

impl Theme {
    pub fn status_style(&self) -> Style {
        Style::default().fg(self.white).bg(self.one_bg3)
    }

    pub fn input_style(&self) -> TextInputStyle {
        TextInputStyle {
            style: Style::default().fg(self.black).bg(self.base05),
            focus: Some(Style::default().fg(self.black).bg(self.green)),
            select: Some(Style::default().fg(self.black).bg(self.base0e)),
            ..TextInputStyle::default()
        }
    }

    pub fn input_mask_style(&self) -> MaskedInputStyle {
        MaskedInputStyle {
            style: Style::default().fg(self.black).bg(self.base05),
            focus: Some(Style::default().fg(self.black).bg(self.green)),
            select: Some(Style::default().fg(self.black).bg(self.base0e)),
            invalid: Some(Style::default().bg(Color::Red)),
            ..Default::default()
        }
    }

    pub fn button_style(&self) -> ButtonStyle {
        ButtonStyle {
            style: Style::default().fg(self.black).bg(self.purple).bold(),
            focus: Some(Style::default().fg(self.black).bg(self.green).bold()),
            armed: Some(Style::default().fg(self.black).bg(self.orange).bold()),
            ..Default::default()
        }
    }

    pub fn statusline_style(&self) -> Vec<Style> {
        vec![
            self.status_style(),
            Style::default().white().on_blue(),
            Style::default().white().on_light_blue(),
            Style::default().white().on_gray(),
        ]
    }

    pub fn status_dialog_style(&self) -> MsgDialogStyle {
        MsgDialogStyle {
            style: self.status_style(),
            button: self.button_style(),
            ..Default::default()
        }
    }

    pub fn menu_style(&self) -> MenuStyle {
        MenuStyle {
            style: Style::default().fg(self.white).bg(self.one_bg3).bold(),
            title: Some(Style::default().fg(self.black).bg(self.base0a).bold()),
            select: Some(Style::default().fg(self.black).bg(self.base0e).bold()),
            focus: Some(Style::default().fg(self.black).bg(self.green).bold()),
            ..Default::default()
        }
    }
}

pub static ONEDARK: Theme = Theme {
    name: "onedark",
    dark_theme: false,

    white: Color::from_u32(0xabb2bf),
    darker_black: Color::from_u32(0x1b1f27),
    black: Color::from_u32(0x1e222a), //  nvim bg
    black2: Color::from_u32(0x252931),
    one_bg: Color::from_u32(0x282c34), // real bg of onedark
    one_bg2: Color::from_u32(0x353b45),
    one_bg3: Color::from_u32(0x373b43),
    grey: Color::from_u32(0x42464e),
    grey_fg: Color::from_u32(0x565c64),
    grey_fg2: Color::from_u32(0x6f737b),
    light_grey: Color::from_u32(0x6f737b),
    red: Color::from_u32(0xe06c75),
    baby_pink: Color::from_u32(0xDE8C92),
    pink: Color::from_u32(0xff75a0),
    line: Color::from_u32(0x31353d), // for lines like vertsplit
    green: Color::from_u32(0x98c379),
    vibrant_green: Color::from_u32(0x7eca9c),
    nord_blue: Color::from_u32(0x81A1C1),
    blue: Color::from_u32(0x61afef),
    yellow: Color::from_u32(0xe7c787),
    sun: Color::from_u32(0xEBCB8B),
    purple: Color::from_u32(0xde98fd),
    dark_purple: Color::from_u32(0xc882e7),
    teal: Color::from_u32(0x519ABA),
    orange: Color::from_u32(0xfca2aa),
    cyan: Color::from_u32(0xa3b8ef),
    statusline_bg: Color::from_u32(0x22262e),
    lightbg: Color::from_u32(0x2d3139),
    pmenu_bg: Color::from_u32(0x61afef),
    folder_bg: Color::from_u32(0x61afef),

    base00: Color::from_u32(0x1e222a),
    base01: Color::from_u32(0x353b45),
    base02: Color::from_u32(0x3e4451),
    base03: Color::from_u32(0x545862),
    base04: Color::from_u32(0x565c64),
    base05: Color::from_u32(0xabb2bf),
    base06: Color::from_u32(0xb6bdca),
    base07: Color::from_u32(0xc8ccd4),
    base08: Color::from_u32(0xe06c75),
    base09: Color::from_u32(0xd19a66),
    base0a: Color::from_u32(0xe5c07b),
    base0b: Color::from_u32(0x98c379),
    base0c: Color::from_u32(0x56b6c2),
    base0d: Color::from_u32(0x61afef),
    base0e: Color::from_u32(0xc678dd),
    base0f: Color::from_u32(0xbe5046),
};

fn setup_logging() -> Result<(), anyhow::Error> {
    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{} {} {}]\n        {}",
                humantime::format_rfc3339_seconds(SystemTime::now()),
                record.level(),
                record.target(),
                message
            ))
        })
        .level(log::LevelFilter::Debug)
        .chain(fern::log_file("log.log")?)
        .apply()?;
    Ok(())
}
