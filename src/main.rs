extern crate tui;
extern crate termion;
extern crate futures;
extern crate tokio_core;
extern crate libpsensor;

use std::io;
use std::thread;
use std::time;
use std::sync::{mpsc, Arc, Mutex};

use termion::event;
use termion::input::TermRead;
use tui::Terminal;
use tui::backend::TermionBackend;
use tui::widgets::{Widget, Block, border, BarChart};
use tui::layout::{Group, Direction, Size, Rect};
use tui::style::{Style, Color};
use futures::Stream;
use tokio_core::reactor::Core;
use libpsensor::{Psensor, PsensorType};

struct App {
    size: Rect,
    data: Vec<(Arc<libpsensor::Psensor>, f64)>,
}

impl App {
    fn new() -> Arc<Mutex<App>> {
        let (tx, rx) = mpsc::sync_channel(1);
        thread::spawn(move || {
            let mut lp = Core::new().unwrap();
            let (sensors, stream) = libpsensor::new(time::Duration::from_millis(500), &lp.handle());
            let data = sensors
                .iter()
                .map(|sensor| (sensor.clone(), 1.0_f64))
                .collect();

            let app = App {
                size: Rect::default(),
                data: data,
            };
            let app = Arc::new(Mutex::new(app));
            tx.send(app.clone()).unwrap();
            lp.run(stream.for_each(|(sensor, new_value)| {
                    let mut app = app.lock().unwrap();
                    for &mut (ref mut s, ref mut value) in &mut app.data {
                        if sensor.id == s.id {
                            *value = new_value;
                            break;
                        }
                    }
                    Ok(())
                }))
                .unwrap();
        });
        rx.recv().unwrap()
    }
}

enum Event {
    Input(event::Key),
    Tick,
}

fn main() {
    // Terminal initialization
    let backend = TermionBackend::new().unwrap();
    let mut terminal = Terminal::new(backend).unwrap();

    // Channels
    let (tx, rx) = mpsc::channel();
    let input_tx = tx.clone();
    let clock_tx = tx.clone();

    // Input
    thread::spawn(move || {
        let stdin = io::stdin();
        for c in stdin.keys() {
            let evt = c.unwrap();
            input_tx.send(Event::Input(evt)).unwrap();
            if evt == event::Key::Char('q') {
                break;
            }
        }
    });

    // Tick
    thread::spawn(move || loop {
                      clock_tx.send(Event::Tick).unwrap();
                      thread::sleep(time::Duration::from_millis(500));
                  });

    // App
    let app = App::new();

    // First draw call
    terminal.clear().unwrap();
    terminal.hide_cursor().unwrap();
    app.lock().unwrap().size = terminal.size().unwrap();
    draw(&mut terminal, &app);

    loop {
        let size = terminal.size().unwrap();
        if app.lock().unwrap().size != size {
            terminal.resize(size).unwrap();
            app.lock().unwrap().size = size;
        }

        let evt = rx.recv().unwrap();
        match evt {
            Event::Input(input) => {
                if input == event::Key::Char('q') {
                    break;
                }
            }
            Event::Tick => {}
        }
        draw(&mut terminal, &app);
    }

    terminal.show_cursor().unwrap();
}

fn filter_sensor(sensors: &[(Arc<Psensor>, f64)],
                 sensor_type: PsensorType,
                 default_max: u64)
                 -> (Vec<(&str, u64)>, u64) {
    let tmp = sensors
        .iter()
        .filter_map(|&(ref sensor, value)| if sensor.sensor == sensor_type {
                        Some((sensor.max, (sensor.name.as_str(), value as u64)))
                    } else {
                        None
                    })
        .collect::<Vec<_>>();
    let cpus_max_temp = tmp.clone()
        .iter()
        .map(|&(max, _)| max)
        .filter(|max| !max.is_nan())
        .map(|max| max as u64)
        .max()
        .unwrap_or(default_max);
    let r = tmp.into_iter().map(|(_, v)| v).collect();
    (r, cpus_max_temp)
}

fn draw(t: &mut Terminal<TermionBackend>, app: &Arc<Mutex<App>>) {
    let app = app.lock().unwrap();
    let (cpus, cpus_max_temp) = filter_sensor(&app.data, PsensorType::Cpu, 80);
    let (gpus, gpus_max_temp) = filter_sensor(&app.data, PsensorType::Gpu, 90);
    let (hdds, hdds_max_temp) = filter_sensor(&app.data, PsensorType::Hdd, 60);
    let (fans, fans_max_temp) = filter_sensor(&app.data, PsensorType::Fan, 4000);
    let (others, others_max_temp) = filter_sensor(&app.data, PsensorType::Other(true), 80);
    Group::default()
        .direction(Direction::Vertical)
        .margin(1)
        .sizes(&[Size::Percent(60), Size::Percent(40)])
        .render(t, &app.size, |t, chunks| {
            Group::default()
                .direction(Direction::Horizontal)
                .sizes(&[Size::Percent(33), Size::Percent(33), Size::Percent(33)])
                .render(t, &chunks[0], |t, chunks| {
                    BarChart::default()
                        .block(Block::default().title("CPUs").borders(border::ALL))
                        .max(cpus_max_temp)
                        .data(&cpus)
                        .bar_width(9)
                        .style(Style::default().fg(Color::Green))
                        .value_style(Style::default().fg(Color::Black).bg(Color::Green))
                        .render(t, &chunks[0]);
                    BarChart::default()
                        .block(Block::default().title("GPUs").borders(border::ALL))
                        .max(gpus_max_temp)
                        .data(&gpus)
                        .bar_width(9)
                        .style(Style::default().fg(Color::Yellow))
                        .value_style(Style::default().fg(Color::Black).bg(Color::Yellow))
                        .render(t, &chunks[1]);
                    BarChart::default()
                        .block(Block::default().title("HDDs").borders(border::ALL))
                        .max(hdds_max_temp)
                        .data(&hdds)
                        .bar_width(9)
                        .style(Style::default().fg(Color::Cyan))
                        .value_style(Style::default().fg(Color::Black).bg(Color::Cyan))
                        .render(t, &chunks[2]);
                });
            Group::default()
                .direction(Direction::Horizontal)
                .sizes(&[Size::Percent(50), Size::Percent(50)])
                .render(t, &chunks[1], |t, chunks| {
                    BarChart::default()
                        .block(Block::default().title("Fans").borders(border::ALL))
                        .max(fans_max_temp)
                        .data(&fans)
                        .bar_width(9)
                        .style(Style::default().fg(Color::Magenta))
                        .value_style(Style::default().fg(Color::Black).bg(Color::Magenta))
                        .render(t, &chunks[0]);
                    BarChart::default()
                        .block(Block::default().title("Others").borders(border::ALL))
                        .max(others_max_temp)
                        .data(&others)
                        .bar_width(9)
                        .style(Style::default().fg(Color::White))
                        .value_style(Style::default().fg(Color::Black).bg(Color::White))
                        .render(t, &chunks[1]);
                });
        });

    t.draw().unwrap();
}
