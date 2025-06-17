use std::{
    io, time::Instant
};

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use md_hardware::{CpuExplosion, CpuUsage, SystemUsage};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span, Text},
    widgets::{Axis, Block, Borders, Chart, Dataset, GraphType, Paragraph},
    Frame, Terminal,
};
use tokio::{time::Duration, task::JoinHandle};

enum Mode {
    Input,
    Chart,
    Finished,
}

enum TimeUnit {
    Seconds,
    Minutes,
}

enum InputFocusElement {
    ValueInput,
    UnitSelection,
    CpuCountSelection,
    OkButton,
}

struct App {
    mode: Mode,
    input_text: String,
    selected_unit: TimeUnit,
    chart_data: Vec<(f64, f64)>, // (time_elapsed, value)
    start_time: Option<Instant>,
    total_duration_secs: u64,
    elapsed_secs: u64,
    current_input_focus: InputFocusElement, // Changed from input_focus_on_value
    finished_popup_selected_option: PopupOption, // New field for popup navigation
    system_usage: SystemUsage,              // Added SystemUsage instance
    last_cpu_refresh: Instant,              // Track last CPU refresh time
    cpu_refresh_interval: Duration,         // Interval for CPU refresh
    cpu_info_cached: Vec<CpuUsage>,         // Cache for CPU info (now custom CpuInfo)
    total_logical_cores: usize,             // Total logical cores available
    selected_cpu_count: usize,              // Number of CPU cores selected by the user
    stress_test: md_hardware::CpuExplosion,
    stress_test_handle: Option<JoinHandle<u64>>,
}

/// Options available in the "Time's Up!" popup.
enum PopupOption {
    RunAgain,
    Exit,
}

impl App {
    /// Creates a new App instance with default values.
    fn new() -> App {
        let mut system_usage_instance = SystemUsage::new();
        let (total_logical_cores, initial_cpus) = system_usage_instance.get_cpu_info();

        App {
            mode: Mode::Input,
            input_text: String::new(),
            selected_unit: TimeUnit::Seconds,
            chart_data: Vec::new(),
            start_time: None,
            total_duration_secs: 0,
            elapsed_secs: 0,
            current_input_focus: InputFocusElement::UnitSelection, // Default focus
            finished_popup_selected_option: PopupOption::RunAgain, // Default selection for popup
            system_usage: system_usage_instance,                   // Initialize SystemUsage
            last_cpu_refresh: Instant::now(),
            cpu_refresh_interval: Duration::from_secs(1), // Refresh CPU every 1 second
            cpu_info_cached: initial_cpus,                // Store initial CPU info
            total_logical_cores,                          // Initialize with actual core count
            selected_cpu_count: 1,                        // Default to 1 selected core
            stress_test: md_hardware::CpuExplosion::new(),
            stress_test_handle: None
        }
    }

    /// Resets the application state to prepare for new input.
    fn reset_for_input(&mut self) {
        self.mode = Mode::Input;
        self.input_text.clear();
        self.chart_data.clear();
        self.start_time = None;
        self.total_duration_secs = 0;
        self.elapsed_secs = 0;
        self.current_input_focus = InputFocusElement::ValueInput; // Reset focus
        self.finished_popup_selected_option = PopupOption::RunAgain; // Reset popup selection
        self.stress_test_handle = None;
        self.stress_test = CpuExplosion::new();
                                                                     // Re-initialize SystemUsage to clear previous data and get fresh system info
        self.system_usage = SystemUsage::new();
        let (_, initial_cpus) = self.system_usage.get_cpu_info();
        self.cpu_info_cached = initial_cpus;
        self.last_cpu_refresh = Instant::now();
        self.selected_cpu_count = 1; // Reset selected CPU count
    }

    /// Parses the input text and selected unit to set the total duration.
    fn set_total_duration(&mut self) {
        if let Ok(value) = self.input_text.parse::<u64>() {
            self.total_duration_secs = match self.selected_unit {
                TimeUnit::Seconds => value,
                TimeUnit::Minutes => value * 60,
            };
            self.start_time = Some(Instant::now());
            self.elapsed_secs = 0;
            let duration_for_stress_test = self.total_duration_secs;
            let cores_for_stress_test = self.selected_cpu_count; // Use selected_cpu_count for the test
            let stress_tester = self.stress_test.clone(); // Clone if CpuExplosion can be cloned, or pass by Arc/Rc

            self.stress_test_handle = Some(tokio::spawn(async move {
                stress_tester.stress_test_cpu(duration_for_stress_test, cores_for_stress_test).await
            }));
           self.mode = Mode::Chart;
        } else {
            self.input_text = "Invalid input".to_string(); // Simple error feedback
        }
    }

    fn update_data(&mut self) {
        if let Some(start) = self.start_time {
            let now = Instant::now();
            let new_elapsed = (now - start).as_secs();

            if new_elapsed > self.elapsed_secs {
                let (_, cpus) = self.system_usage.get_cpu_info();
                let chart_value = avg_percent_usage_cpu(&cpus);
                self.elapsed_secs = new_elapsed;
                self.chart_data
                    .push((self.elapsed_secs as f64, chart_value));

                let max_data_points = 100;
                if self.chart_data.len() > max_data_points {
                    self.chart_data.remove(0);
                }
            }
        }

        // Update CPU info only if refresh interval has passed
        if self.last_cpu_refresh.elapsed() >= self.cpu_refresh_interval {
            let (_, cpus) = self.system_usage.get_cpu_info();
            self.cpu_info_cached = cpus;
            self.last_cpu_refresh = Instant::now();
        }
    }
}

fn avg_percent_usage_cpu(cpus: &Vec<CpuUsage>) -> f64 {
    let mut acc: f64 = 0.;
    for i in cpus {
        acc += i.usage as f64
    }
    acc / cpus.len() as f64
}

/// Draws the application UI in the input mode.
fn ui_input_mode(frame: &mut Frame, app: &mut App) {
    let size = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Added: For displaying selected unit name
            Constraint::Length(3), // Duration Input
            Constraint::Length(3), // Time Unit Selection
            Constraint::Length(3), // CPU Count Selection
            Constraint::Length(3), // OK Button (New)
            Constraint::Length(3), // Instructions
            Constraint::Min(0),    // Remaining space
        ])
        .split(size);

    // Display selected unit name
    let selected_unit_name = match app.selected_unit {
        TimeUnit::Seconds => "Selected: Seconds",
        TimeUnit::Minutes => "Selected: Minutes",
    };
    let selected_unit_paragraph = Paragraph::new(selected_unit_name)
        .style(Style::default().fg(Color::Yellow))
        .alignment(ratatui::layout::Alignment::Center);
    frame.render_widget(selected_unit_paragraph, chunks[0]); // Use the new chunk for this

    // Input field (Duration)
    let input_block_style = if matches!(app.current_input_focus, InputFocusElement::ValueInput) {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Reset)
    };

    // Time unit selection
    let seconds_style = if matches!(app.selected_unit, TimeUnit::Seconds) {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Reset)
    };
    let minutes_style = if matches!(app.selected_unit, TimeUnit::Minutes) {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Reset)
    };

    let unit_block = Block::default().borders(Borders::ALL).title("Select Unit");
    let unit_paragraph = Paragraph::new(Line::from(vec![
        Span::styled(
            "  Seconds  ",
            if matches!(app.current_input_focus, InputFocusElement::UnitSelection) {
                seconds_style
            } else {
                Style::default().fg(Color::Reset)
            },
        ),
        Span::raw("    "),
        Span::styled(
            "  Minutes  ",
            if matches!(app.current_input_focus, InputFocusElement::UnitSelection) {
                minutes_style
            } else {
                Style::default().fg(Color::Reset)
            },
        ),
    ]))
    .block(unit_block);
    frame.render_widget(unit_paragraph, chunks[1]);

    let input_block = Block::default()
        .borders(Borders::ALL)
        .title("Enter Duration");
    let input_paragraph = Paragraph::new(app.input_text.as_str())
        .style(input_block_style)
        .block(input_block);
    frame.render_widget(input_paragraph, chunks[2]);

    // CPU Count Selection
    let cpu_count_block_style = if matches!(
        app.current_input_focus,
        InputFocusElement::CpuCountSelection
    ) {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Reset)
    };

    let cpu_count_block = Block::default()
        .borders(Borders::ALL)
        .title(format!("Selected Cores (1-{})", app.total_logical_cores));
    let cpu_count_paragraph = Paragraph::new(app.selected_cpu_count.to_string())
        .style(cpu_count_block_style)
        .alignment(ratatui::layout::Alignment::Center)
        .block(cpu_count_block);
    frame.render_widget(cpu_count_paragraph, chunks[3]); // Adjusted chunk index

    // OK Button
    let ok_button_style = if matches!(app.current_input_focus, InputFocusElement::OkButton) {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
            .add_modifier(Modifier::REVERSED)
    } else {
        Style::default().fg(Color::White)
    };
    let ok_button = Paragraph::new("    OK    ")
        .style(ok_button_style)
        .alignment(ratatui::layout::Alignment::Center)
        .block(Block::default().borders(Borders::ALL));
    frame.render_widget(ok_button, chunks[4]); // Adjusted chunk index

    // Instructions
    let instructions_block = Block::default().borders(Borders::ALL).title("Instructions");
    let instructions_paragraph = Paragraph::new(
        "Type duration, TAB to cycle focus. Up/Down/Left/Right to select and change values. Up/Down for Cores. ENTER on OK to start. 'q' or 'Q' to quit.",
    )
    .block(instructions_block);
    frame.render_widget(instructions_paragraph, chunks[5]); // Adjusted chunk index

    // Position the cursor in the input field if it's focused
    if matches!(app.current_input_focus, InputFocusElement::ValueInput) {
        frame.set_cursor_position(Position {
            x: chunks[0].x + app.input_text.len() as u16 + 1,
            y: chunks[0].y + 5,
        });
    }
}

/// Draws the application UI in the chart mode.
fn ui_chart_mode(frame: &mut Frame, app: &mut App) {
    let size = frame.area();

    // Split screen into 75% for chart and 25% for empty block, horizontally
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(3, 4), Constraint::Ratio(1, 4)])
        .split(size);

    // Chart Block
    let chart_block = Block::default()
        .title(Line::from(vec![
            Span::styled(
                "Timer Chart",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!(
                " (Elapsed: {}s / {}s)",
                app.elapsed_secs, app.total_duration_secs
            )),
        ]))
        .borders(Borders::ALL);

    // Calculate max x and max y for chart scaling
    let max_x = app.total_duration_secs as f64;
    let max_y = 100.0; // Assuming chart values won't exceed 30 much based on our sine example

    let datasets = vec![Dataset::default()
        .name("Value over time")
        .marker(symbols::Marker::Dot)
        .style(Style::default().fg(Color::Green))
        .graph_type(GraphType::Line)
        .data(&app.chart_data)];

    let chart = Chart::new(datasets)
        .block(chart_block)
        .x_axis(
            Axis::default()
                .title("Time (s)")
                .style(Style::default().fg(Color::Gray))
                .bounds([0.0, max_x])
                .labels(vec![
                    Span::styled("0", Style::default().fg(Color::White)),
                    Span::styled(
                        format!("{}", max_x / 2.0),
                        Style::default().fg(Color::White),
                    ),
                    Span::styled(format!("{}", max_x), Style::default().fg(Color::White)),
                ]),
        )
        .y_axis(
            Axis::default()
                .title("Usage %")
                .style(Style::default().fg(Color::Gray))
                .bounds([0.0, max_y])
                .labels(vec![
                    Span::styled("0", Style::default().fg(Color::White)),
                    Span::styled(
                        format!("{}", max_y / 2.0),
                        Style::default().fg(Color::White),
                    ),
                    Span::styled(format!("{}", max_y), Style::default().fg(Color::White)),
                ]),
        );
    frame.render_widget(chart, chunks[0]);

    // System Info Block
    let (used_ram, total_ram) = app.system_usage.get_ram_info(); // Get fresh RAM info

    let mut system_info_text = Vec::new();

    // RAM Usage on top
    system_info_text.push(Line::from(format!(
        "RAM Usage: {} MB / {} MB",
        used_ram / 1024 / 1024,
        total_ram / 1024 / 1024
    )));
    system_info_text.push(Line::from("")); // Spacer

    // CPU Info (2 items per line with different colors, limited by selected_cpu_count)
    system_info_text.push(Line::from("CPU Usage:"));
    let cpu_colors = [
        Color::LightRed,
        Color::LightGreen,
        Color::LightBlue,
        Color::LightCyan,
        Color::LightMagenta,
        Color::Yellow,
        Color::Green,
        Color::Blue,
    ]; // Define a set of colors

    for i in 0..app.cpu_info_cached.len() {
        if let Some(cpu) = app.cpu_info_cached.get(i) {
            let color_index = i % cpu_colors.len(); // Cycle through colors
            let cpu_style = Style::default().fg(cpu_colors[color_index]);

            if i % 2 == 0 {
                let mut line_spans = vec![];
                line_spans.push(Span::styled(
                    format!("{}: {:.1}%", cpu.name, cpu.usage),
                    cpu_style,
                ));

                if let Some(next_cpu) = app.cpu_info_cached.get(i + 1) {
                    let next_color_index = (i + 1) % cpu_colors.len();
                    let next_cpu_style = Style::default().fg(cpu_colors[next_color_index]);
                    line_spans.push(Span::raw("    ")); // Spacer between two CPU infos
                    line_spans.push(Span::styled(
                        format!("{}: {:.1}%", next_cpu.name, next_cpu.usage),
                        next_cpu_style,
                    ));
                }
                system_info_text.push(Line::from(line_spans));
            }
        }
    }

    let system_info_block = Block::default()
        .title("System Info")
        .borders(Borders::ALL)
        .style(Style::default().bg(Color::Black));

    let system_info_paragraph =
        Paragraph::new(Text::from(system_info_text)).block(system_info_block);
    frame.render_widget(system_info_paragraph, chunks[1]);
}

/// Draws the application UI in the "Time's Up!" popup mode.
fn ui_finished_popup_mode(frame: &mut Frame, app: &mut App) {
    // Draw a semi-transparent background to make the popup stand out
    let area = frame.area();
    frame.render_widget(
        Block::default().style(
            Style::default()
                .bg(Color::Rgb(0, 0, 0))
                .add_modifier(Modifier::DIM),
        ),
        area,
    );

    // Calculate popup size and position (centered)
    let popup_width = 40;
    let popup_height = 10;
    let popup_area = Rect::new(
        (area.width.saturating_sub(popup_width)) / 2,
        (area.height.saturating_sub(popup_height)) / 2,
        popup_width,
        popup_height,
    );

    let popup_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Title
            Constraint::Length(1), // Message
            Constraint::Length(1), // Spacer
            Constraint::Length(1), // Run Again
            Constraint::Length(1), // Exit
            Constraint::Min(0),    // Spacer
        ])
        .margin(1)
        .split(popup_area);

    let popup_block = Block::default()
        .title(Line::from(vec![Span::styled(
            "Time's Up!",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )]))
        .borders(Borders::ALL)
        .style(Style::default().bg(Color::Black).fg(Color::White)); // Black background for the popup

    frame.render_widget(popup_block, popup_area);

    let message =
        Paragraph::new("Your timer has finished!").alignment(ratatui::layout::Alignment::Center);
    frame.render_widget(message, popup_chunks[1]);

    let run_again_style = if matches!(app.finished_popup_selected_option, PopupOption::RunAgain) {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let run_again_text = Paragraph::new("Run Again (Enter)")
        .style(run_again_style)
        .alignment(ratatui::layout::Alignment::Center);
    frame.render_widget(run_again_text, popup_chunks[3]);

    let exit_style = if matches!(app.finished_popup_selected_option, PopupOption::Exit) {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let exit_text = Paragraph::new("Exit (Q/Esc)")
        .style(exit_style)
        .alignment(ratatui::layout::Alignment::Center);
    frame.render_widget(exit_text, popup_chunks[4]);
}


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    let mut running = true;

    while running {
        terminal.draw(|frame| {
            match app.mode {
                Mode::Input => ui_input_mode(frame, &mut app),
                Mode::Chart => ui_chart_mode(frame, &mut app),
                Mode::Finished => ui_finished_popup_mode(frame, &mut app), // Draw popup
            }
        })?;

        // Event handling
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    // Universal quit handling for 'q', 'Q', and Ctrl+C
                    if key.code == KeyCode::Char('q')
                        || key.code == KeyCode::Char('Q')
                        || (key.code == KeyCode::Char('c')
                            && key.modifiers.contains(KeyModifiers::CONTROL))
                    {
                        running = false;
                        if let Some(val) = &app.stress_test_handle{
                            val.abort();
                        }
                    } else {
                        match app.mode {
                            Mode::Input => match key.code {
                                KeyCode::Char(c) => {
                                    // Only allow digits if focus is on value input
                                    if matches!(
                                        app.current_input_focus,
                                        InputFocusElement::ValueInput
                                    ) && c.is_ascii_digit()
                                    {
                                        app.input_text.push(c);
                                    }
                                }
                                KeyCode::Backspace => {
                                    // Only allow backspace if focus is on value input
                                    if matches!(
                                        app.current_input_focus,
                                        InputFocusElement::ValueInput
                                    ) {
                                        app.input_text.pop();
                                    }
                                }
                                KeyCode::Tab => {
                                    app.current_input_focus = match app.current_input_focus {
                                        InputFocusElement::ValueInput => {
                                            InputFocusElement::CpuCountSelection
                                        }
                                        InputFocusElement::UnitSelection => {
                                            InputFocusElement::ValueInput
                                        }
                                        InputFocusElement::CpuCountSelection => {
                                            InputFocusElement::OkButton
                                        } // Cycle to OK button
                                        InputFocusElement::OkButton => {
                                            InputFocusElement::UnitSelection
                                        } // Cycle back to ValueInput
                                    };
                                }
                                KeyCode::Down | KeyCode::Left => {
                                    // Navigate units only if focus is on unit selection
                                    if matches!(
                                        app.current_input_focus,
                                        InputFocusElement::UnitSelection
                                    ) {
                                        app.selected_unit = match app.selected_unit {
                                            TimeUnit::Seconds => TimeUnit::Minutes,
                                            TimeUnit::Minutes => TimeUnit::Seconds,
                                        };
                                    }

                                    if matches!(
                                        app.current_input_focus,
                                        InputFocusElement::CpuCountSelection
                                    ) {
                                        if app.selected_cpu_count > 1 {
                                            app.selected_cpu_count -= 1;
                                        }
                                    }
                                }
                                KeyCode::Up | KeyCode::Right => {
                                    if matches!(
                                        app.current_input_focus,
                                        InputFocusElement::UnitSelection
                                    ) {
                                        app.selected_unit = match app.selected_unit {
                                            TimeUnit::Seconds => TimeUnit::Minutes,
                                            TimeUnit::Minutes => TimeUnit::Seconds,
                                        };
                                    }

                                    if matches!(
                                        app.current_input_focus,
                                        InputFocusElement::CpuCountSelection
                                    ) {
                                        if app.selected_cpu_count < app.total_logical_cores {
                                            app.selected_cpu_count += 1;
                                        }
                                    }
                                }
                                KeyCode::Enter => {
                                    match app.current_input_focus {
                                        InputFocusElement::ValueInput => {
                                            app.current_input_focus =
                                                InputFocusElement::UnitSelection;
                                        }
                                        InputFocusElement::UnitSelection => {
                                            app.current_input_focus =
                                                InputFocusElement::CpuCountSelection;
                                        }
                                        InputFocusElement::CpuCountSelection => {
                                            app.current_input_focus = InputFocusElement::OkButton;
                                        }
                                        InputFocusElement::OkButton => {
                                            app.set_total_duration();
                                        }
                                    }
                                }
                                _ => {}
                            },
                            Mode::Chart => {
                                match key.code {
                                    KeyCode::Esc => {
                                        app.reset_for_input(); // Escape key to go back to input mode
                                    }
                                    _ => {}
                                }
                            }
                            Mode::Finished => match key.code {
                                KeyCode::Enter => match app.finished_popup_selected_option {
                                    PopupOption::RunAgain => app.reset_for_input(),
                                    PopupOption::Exit => running = false,
                                },
                                KeyCode::Up | KeyCode::Down | KeyCode::Tab => {
                                    app.finished_popup_selected_option =
                                        match app.finished_popup_selected_option {
                                            PopupOption::RunAgain => PopupOption::Exit,
                                            PopupOption::Exit => PopupOption::RunAgain,
                                        };
                                }
                                KeyCode::Esc => {
                                    running = false;
                                    if let Some(val) = &app.stress_test_handle{
                                        val.abort();
                                    }
                                }
                                _ => {}
                            },
                        }
                    }
                }
            }
        }

        if let Some(handle) = &app.stress_test_handle {    
            if handle.is_finished() && running {
                app.stress_test_handle = None;
                app.mode = Mode::Finished;
            }
        }
        if matches!(app.mode, Mode::Chart) && running {
            app.update_data();
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}
