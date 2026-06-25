/**
 * Floating Clock Widget for Wayland/Hyprland
 * 
 * ============================================================================
 * EDUCATIONAL DOCUMENTATION: WINDOWING SYSTEMS, PROTOCOLS & GRAPHICS
 * ============================================================================
 * 
 * 1. WAYLAND DISPLAY SERVER PROTOCOL
 * ---------------------------------
 * Wayland is a modern display server protocol for Linux designed as a secure,
 * performant replacement for the legacy X11 windowing system. Under Wayland,
 * the architecture enforces strict "client isolation":
 * - Applications (clients) cannot see or modify the window states of other clients.
 * - Clients cannot query global coordinates (e.g., where the mouse pointer is on
 *   the absolute screen).
 * - Clients cannot place themselves at absolute screen coordinates. Window
 *   placement is controlled entirely by the compositor (e.g., Mutter, KWin, Sway).
 * 
 * 2. THE LAYER SHELL PROTOCOL (wlr-layer-shell-unstable-v1)
 * ---------------------------------------------------------
 * Standard Wayland windows use the `xdg-shell` protocol for floating, draggable
 * application windows. However, desktop components like status bars, panels,
 * wallpapers, or widgets need to stay anchored to specific screen coordinates
 * and depths.
 * This clock widget uses `gtk-layer-shell`, which implements the Wayland
 * Layer Shell protocol. Instead of absolute positioning, layer surfaces are:
 * - Positioned at one of four depth layers (Background, Bottom, Top, Overlay).
 * - Anchored to specific edges (Top, Bottom, Left, Right).
 * - Positioned relative to these anchors using margin values (e.g., 50px left margin).
 * Because layer shell surfaces are managed strictly by the compositor as layout elements,
 * compositor-driven dragging methods (like `gdk::Window::begin_move_drag`) are typically
 * ignored. Dragging must be handled manually by the client by dynamically modifying margins.
 * 
 * 3. CAIRO VECTOR GRAPHICS
 * ------------------------
 * Cairo is a 2D vector graphics library designed to draw crisp vector paths
 * onto various backends (Xlib, Wayland, PDFs, Image surfaces). In this widget,
 * we use the Cairo context (`cairo::Context`) to perform custom rendering:
 * - Converting styled text into mathematical outlines (paths).
 * - Stroking (drawing the border outline) with a specified width and color.
 * - Filling (painting the inner area) of the path with the clock's theme color.
 * 
 * 4. PANGO TEXT LAYOUT ENGINE
 * ---------------------------
 * Pango is the standard library used by GTK for layout and rendering of text.
 * Pango integrates with Cairo (`pangocairo`) to:
 * - Load system fonts and support internationalized text.
 * - Measure text layout geometries (width/height in pixels) to inform GTK of
 *   the widget's sizing requirements (`set_size_request`).
 * 
 * 5. CONTROL THEORY MATH FOR DRAG COMPENSATED FALLBACK (GENERIC BACKEND)
 * ----------------------------------------------------------------------
 * Under Wayland, when using the generic drag fallback, mouse motion coordinates
 * returned by GDK are relative to the window surface (`event.root()` maps to 
 * local surface space, not global screen space).
 * 
 * Let:
 *   W_i = Physical margin position of the window on the screen at step i.
 *   C_i = Coordinates of the mouse relative to the window boundary at step i.
 *   W_req = The margin value we request the compositor to set.
 * 
 * When a user moves the mouse globally by Δm, the cursor moves in coordinate space.
 * Because we change the window margin to follow the mouse, the window moves.
 * When the window moves by Δw, the relative coordinates returned by GDK are shifted
 * in the opposite direction (-Δw).
 * 
 * If we calculate mouse movement simply as `C_current - C_initial` and add that to 
 * the starting margin, compositor latency causes a feedback loop:
 *   1. Mouse moves globally -> GDK registers motion -> We increase margins.
 *   2. Window moves physically -> GDK registers coordinate shift ->
 *      Coordinates relative to window decrease -> We decrease margins.
 *   3. This causes the window to oscillate/jitter violently at the click spot.
 * 
 * To solve this:
 * Every time we adjust the window margin, we update our reference baseline `C_initial`:
 *   C_initial = C_initial - Δw_actual
 * Where `Δw_actual` is the change in margins we just commanded.
 * This mathematically cancels out the compositor-induced coordinate shift, keeping the
 * baseline coordinate aligned with the global motion, producing smooth, stable dragging
 * on any compositor (GNOME, Sway, KDE, etc.) without needing compositor-specific sockets.
 */

use chrono::Local;
use clap::Parser;
use glib::clone;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, CssProvider, DrawingArea, StyleContext};
use gtk_layer_shell::LayerShell;
use serde::{Deserialize, Serialize};
use std::cell::{Cell, RefCell};
use std::fs;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use cairo;
use pango;
use pangocairo;

// =====================================================================
// COMPOSITOR BACKENDS & DETECTION
// =====================================================================

/// Supported Wayland compositor backends for absolute pointer tracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompositorBackend {
    /// Use Hyprland's IPC socket to query global cursor position.
    Hyprland,
    /// Use the generic, drag-compensated mathematical fallback.
    Generic,
}

/// Detects the target backend based on user settings and runtime environment.
fn detect_backend(backend_setting: &str) -> CompositorBackend {
    match backend_setting.to_lowercase().as_str() {
        "hyprland" => CompositorBackend::Hyprland,
        "generic" => CompositorBackend::Generic,
        _ => {
            // "auto" mode: check if we are running under Hyprland
            if std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok() {
                CompositorBackend::Hyprland
            } else {
                CompositorBackend::Generic
            }
        }
    }
}

// =====================================================================
// CONFIGURATION & COMMAND-LINE ARGS
// =====================================================================

/// Configuration structure parsed from/to TOML.
#[derive(Debug, Serialize, Deserialize, Clone)]
struct Config {
    /// Font size of the clock text (in Pango points).
    size: u32,
    /// Clock text color (HEX code, e.g., "#00b7ff").
    color: String,
    /// Outline border color (HEX code, e.g., "#000000").
    border_color: String,
    /// Thickness of the outline stroke in pixels.
    thickness: i32,
    /// Optional manual X margin offset. If set, disables position persistence loading.
    pos_x: Option<i32>,
    /// Optional manual Y margin offset. If set, disables position persistence loading.
    pos_y: Option<i32>,
    /// Date & time formatting string (compatible with Chrono formats).
    format: String,
    /// Font family to use for rendering (e.g. "JetBrains Mono, sans-serif").
    font_family: String,
    /// Font weight (e.g. "heavy", "bold", "normal", "light").
    font_weight: String,
    /// Pointer coordinate backend to use: "auto", "hyprland", or "generic".
    backend: String,
    /// Flag indicating if the window position should be loaded and saved automatically.
    save_position: bool,
    /// Show a static demo time (09:41:00 on Jan 9, 2007).
    demo: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            size: 11,
            color: "#00b7ff".to_string(),
            border_color: "#000000".to_string(),
            thickness: 3,
            pos_x: None,
            pos_y: None,
            format: "%H:%M:%S\n%d/%m/%Y".to_string(),
            font_family: "JetBrains Mono, monospace, sans-serif".to_string(),
            font_weight: "heavy".to_string(),
            backend: "auto".to_string(),
            save_position: true,
            demo: false,
        }
    }
}

/// Command-line arguments using clap-derive.
#[derive(Parser, Debug, Clone)]
#[command(author, version, about = "Floating Clock widget for Wayland/Hyprland", long_about = None)]
struct Args {
    #[arg(short, long, help = "Font size in Pango points")]
    size: Option<u32>,

    #[arg(short = 'c', long, help = "Clock text color (HEX code, e.g. #00b7ff)")]
    color: Option<String>,

    #[arg(short = 'b', long, help = "Text outline border color (HEX code, e.g. #000000)")]
    border_color: Option<String>,

    #[arg(short = 't', long, help = "Text outline thickness in pixels")]
    thickness: Option<i32>,

    #[arg(short = 'x', long, help = "Manual X margin offset (ignores position file)")]
    pos_x: Option<i32>,

    #[arg(short = 'y', long, help = "Manual Y margin offset (ignores position file)")]
    pos_y: Option<i32>,

    #[arg(short = 'f', long, help = "Date & time format string (chrono layout)")]
    format: Option<String>,

    #[arg(long, help = "Font family name (e.g. 'JetBrains Mono, sans-serif')")]
    font_family: Option<String>,

    #[arg(long, help = "Font weight (e.g. 'heavy', 'bold', 'normal')")]
    font_weight: Option<String>,

    #[arg(long, help = "Compositor pointer tracking backend ('auto', 'hyprland', 'generic')")]
    backend: Option<String>,

    #[arg(long, help = "Disable persistence of window position")]
    no_save_position: bool,

    #[arg(long, help = "Show a static demo time (09:41:00 on Jan 9, 2007)")]
    demo: bool,
}

// =====================================================================
// UTILITIES
// =====================================================================

/// Load user configuration from XDG config directory, creating a default config if missing.
fn load_or_create_config() -> Config {
    let config_dir = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").expect("Could not find environment variable HOME");
            PathBuf::from(home).join(".config")
        })
        .join("float-clock-wayland");

    if let Err(e) = fs::create_dir_all(&config_dir) {
        eprintln!("Warning: could not create config directory: {}", e);
    }

    let config_path = config_dir.join("config.toml");
    if !config_path.exists() {
        let default_config = Config::default();
        if let Ok(toml_str) = toml::to_string_pretty(&default_config) {
            let mut file_content = String::new();
            file_content.push_str("# float-clock-wayland configuration file\n\n");
            file_content.push_str(&toml_str);
            if let Err(e) = fs::write(&config_path, file_content) {
                eprintln!("Warning: could not write default config file: {}", e);
            }
        }
        default_config
    } else {
        match fs::read_to_string(&config_path) {
            Ok(contents) => match toml::from_str::<Config>(&contents) {
                Ok(config) => config,
                Err(e) => {
                    eprintln!("Warning: Could not parse config.toml ({}), using default values", e);
                    Config::default()
                }
            },
            Err(e) => {
                eprintln!("Warning: Could not read config.toml ({}), using default values", e);
                Config::default()
            }
        }
    }
}

/// Merges command-line arguments into the configuration file settings.
fn merge_config_and_args(mut config: Config, args: Args) -> Config {
    if let Some(s) = args.size { config.size = s; }
    if let Some(c) = args.color { config.color = c; }
    if let Some(bc) = args.border_color { config.border_color = bc; }
    if let Some(t) = args.thickness { config.thickness = t; }
    if args.pos_x.is_some() { config.pos_x = args.pos_x; }
    if args.pos_y.is_some() { config.pos_y = args.pos_y; }
    if let Some(f) = args.format { config.format = f; }
    if let Some(ff) = args.font_family { config.font_family = ff; }
    if let Some(fw) = args.font_weight { config.font_weight = fw; }
    if let Some(b) = args.backend { config.backend = b; }
    if args.no_save_position { config.save_position = false; }
    if args.demo { config.demo = true; }
    config
}

/// Converts a hex color string (e.g. "#00b7ff", "#0bf", or with alpha "#00b7ffff") into RGBA f64 components.
fn parse_hex(hex: &str) -> (f64, f64, f64, f64) {
    let hex = hex.trim_start_matches('#');
    if hex.len() == 3 || hex.len() == 4 {
        // Handle short HEX codes (e.g. #fff, #0bf, #0bfa)
        let r_char = hex.chars().nth(0).unwrap_or('F');
        let g_char = hex.chars().nth(1).unwrap_or('F');
        let b_char = hex.chars().nth(2).unwrap_or('F');
        let a_char = hex.chars().nth(3).unwrap_or('F');
        
        let r = u8::from_str_radix(&format!("{}{}", r_char, r_char), 16).unwrap_or(255) as f64 / 255.0;
        let g = u8::from_str_radix(&format!("{}{}", g_char, g_char), 16).unwrap_or(255) as f64 / 255.0;
        let b = u8::from_str_radix(&format!("{}{}", b_char, b_char), 16).unwrap_or(255) as f64 / 255.0;
        let a = if hex.len() == 4 {
            u8::from_str_radix(&format!("{}{}", a_char, a_char), 16).unwrap_or(255) as f64 / 255.0
        } else {
            1.0
        };
        (r, g, b, a)
    } else {
        // Handle standard 6 or 8 char HEX codes
        let r = u8::from_str_radix(hex.get(0..2).unwrap_or("FF"), 16).unwrap_or(255) as f64 / 255.0;
        let g = u8::from_str_radix(hex.get(2..4).unwrap_or("FF"), 16).unwrap_or(255) as f64 / 255.0;
        let b = u8::from_str_radix(hex.get(4..6).unwrap_or("FF"), 16).unwrap_or(255) as f64 / 255.0;
        let a = if hex.len() >= 8 {
            u8::from_str_radix(hex.get(6..8).unwrap_or("FF"), 16).unwrap_or(255) as f64 / 255.0
        } else {
            1.0
        };
        (r, g, b, a)
    }
}

/// Convert font weight configuration string into Pango Weight enum.
fn parse_font_weight(weight: &str) -> pango::Weight {
    match weight.to_lowercase().as_str() {
        "thin" => pango::Weight::Thin,
        "ultralight" | "ultra-light" => pango::Weight::Ultralight,
        "light" => pango::Weight::Light,
        "semilight" | "semi-light" => pango::Weight::Semilight,
        "book" => pango::Weight::Book,
        "normal" => pango::Weight::Normal,
        "medium" => pango::Weight::Medium,
        "semibold" | "semi-bold" => pango::Weight::Semibold,
        "bold" => pango::Weight::Bold,
        "ultrabold" | "ultra-bold" => pango::Weight::Ultrabold,
        "heavy" | "black" => pango::Weight::Heavy,
        "ultraheavy" | "ultra-heavy" => pango::Weight::Ultraheavy,
        _ => pango::Weight::Heavy,
    }
}

// =====================================================================
// INTEGRACIÓN CON HYPRLAND
// =====================================================================

/// Queries the cursor coordinates directly from Hyprland IPC socket or CLI client.
fn get_hyprland_cursor_pos() -> Option<(i32, i32)> {
    if let Ok(signature) = std::env::var("HYPRLAND_INSTANCE_SIGNATURE") {
        let path = format!("/tmp/hypr/{}/.socket.sock", signature);
        if let Ok(mut stream) = UnixStream::connect(path) {
            if stream.write_all(b"cursorpos").is_ok() {
                let mut buf = String::new();
                if stream.read_to_string(&mut buf).is_ok() {
                    let parts: Vec<&str> = buf.trim().split(',').collect();
                    if parts.len() >= 2 {
                        if let (Ok(x), Ok(y)) = (parts[0].trim().parse::<i32>(), parts[1].trim().parse::<i32>()) {
                            return Some((x, y));
                        }
                    }
                }
            }
        }
    }
    
    // Command-line fallback if socket connection fails
    if let Ok(output) = std::process::Command::new("hyprctl").arg("cursorpos").output() {
        if let Ok(stdout) = String::from_utf8(output.stdout) {
            let parts: Vec<&str> = stdout.trim().split(',').collect();
            if parts.len() >= 2 {
                if let (Ok(x), Ok(y)) = (parts[0].trim().parse::<i32>(), parts[1].trim().parse::<i32>()) {
                    return Some((x, y));
                }
            }
        }
    }
    None
}

// =====================================================================
// APPLICATION MAIN ENTRY
// =====================================================================

fn main() {
    let args = Args::parse();
    let file_config = load_or_create_config();
    let config = merge_config_and_args(file_config, args);

    let app = Application::builder()
        .application_id("com.github.reloj-wayland")
        .build();

    app.connect_activate(move |app| build_ui(app, config.clone()));
    app.run_with_args::<&str>(&[]);
}

fn build_ui(app: &Application, config: Config) {
    let config = Rc::new(config);
    let window = ApplicationWindow::builder()
        .application(app)
        .app_paintable(true)
        .build();

    // 1. INITIALIZE LAYER SHELL PROTOCOL
    window.init_layer_shell();
    // Position layer above standard windows
    window.set_layer(gtk_layer_shell::Layer::Top);
    // Anchor top-left so that Left/Top margins correspond directly to screen coordinates
    window.set_anchor(gtk_layer_shell::Edge::Top, true);
    window.set_anchor(gtk_layer_shell::Edge::Left, true);

    // Load initial positions
    let (saved_x, saved_y) = load_position();
    let start_x = config.pos_x.unwrap_or(saved_x);
    let start_y = config.pos_y.unwrap_or(saved_y);

    window.set_layer_shell_margin(gtk_layer_shell::Edge::Left, start_x);
    window.set_layer_shell_margin(gtk_layer_shell::Edge::Top, start_y);

    // Enable alpha visual on screen for window transparency
    if let Some(screen) = gtk::prelude::WidgetExt::screen(&window) {
        if let Some(visual) = screen.rgba_visual() {
            window.set_visual(Some(&visual));
        }
    }

    // 2. WIDGET SETUP & STATE
    let drawing_area = DrawingArea::new();
    window.add(&drawing_area);

    // Shared state variables
    let margin_left = Rc::new(Cell::new(start_x));
    let margin_top = Rc::new(Cell::new(start_y));
    let is_dragging = Rc::new(Cell::new(false));
    let click_x = Rc::new(Cell::new(0));
    let click_y = Rc::new(Cell::new(0));
    let initial_margin_x = Rc::new(Cell::new(start_x));
    let initial_margin_y = Rc::new(Cell::new(start_y));
    
    // Clock formatted text reference shared with drawing closures
    let time_text = Rc::new(RefCell::new(String::new()));

    // Subscribe to GDK pointer events for dragging
    window.add_events(
        gtk::gdk::EventMask::BUTTON_PRESS_MASK
            | gtk::gdk::EventMask::BUTTON_RELEASE_MASK
            | gtk::gdk::EventMask::BUTTON1_MOTION_MASK,
    );

    // Cache the resolved backend once at UI build time to avoid slow lookups on mouse drag
    let resolved_backend = detect_backend(&config.backend);

    // 3. MOUSE PRESS HANDLER
    window.connect_button_press_event(clone!(
        @strong window, 
        @strong is_dragging, 
        @strong click_x, 
        @strong click_y, 
        @strong margin_left, 
        @strong margin_top, 
        @strong initial_margin_x, 
        @strong initial_margin_y,
        @strong config => move |_, event| {
            if event.button() == 1 {
                is_dragging.set(true);
                
                initial_margin_x.set(margin_left.get());
                initial_margin_y.set(margin_top.get());

                if resolved_backend == CompositorBackend::Hyprland {
                    if let Some((x, y)) = get_hyprland_cursor_pos() {
                        click_x.set(x);
                        click_y.set(y);
                    } else {
                        let (x, y) = event.root();
                        click_x.set(x as i32);
                        click_y.set(y as i32);
                    }
                } else {
                    // Generic fallback: event.root() provides window-relative coordinates.
                    let (x, y) = event.root();
                    click_x.set(x as i32);
                    click_y.set(y as i32);
                }
                
                gtk::glib::Propagation::Stop
            } else if event.button() == 3 {
                // Right click closes the widget
                window.close();
                gtk::glib::Propagation::Stop
            } else {
                gtk::glib::Propagation::Proceed
            }
        }
    ));

    // 4. MOUSE RELEASE HANDLER
    window.connect_button_release_event(clone!(
        @strong is_dragging, 
        @strong margin_left, 
        @strong margin_top, 
        @strong config => move |_, event| {
            if event.button() == 1 {
                is_dragging.set(false);
                if config.save_position {
                    save_position(margin_left.get(), margin_top.get());
                }
                gtk::glib::Propagation::Stop
            } else {
                gtk::glib::Propagation::Proceed
            }
        }
    ));

    // 5. MOUSE MOTION (DRAGGING) HANDLER
    window.connect_motion_notify_event(clone!(
        @strong window, 
        @strong is_dragging, 
        @strong click_x, 
        @strong click_y, 
        @strong margin_left, 
        @strong margin_top, 
        @strong initial_margin_x, 
        @strong initial_margin_y => move |_, event| {
            if is_dragging.get() {
                if resolved_backend == CompositorBackend::Hyprland {
                    if let Some((abs_x, abs_y)) = get_hyprland_cursor_pos() {
                        let delta_x = abs_x - click_x.get();
                        let delta_y = abs_y - click_y.get();
                        
                        let new_margin_left = initial_margin_x.get() + delta_x;
                        let new_margin_top = initial_margin_y.get() + delta_y;
                        
                        margin_left.set(new_margin_left);
                        margin_top.set(new_margin_top);
                        
                        window.set_layer_shell_margin(gtk_layer_shell::Edge::Left, new_margin_left);
                        window.set_layer_shell_margin(gtk_layer_shell::Edge::Top, new_margin_top);
                    }
                } else {
                    // Generic fallback: event.root() provides window-relative coordinates.
                    let (rel_x, rel_y) = event.root();
                    let abs_x = rel_x as i32;
                    let abs_y = rel_y as i32;

                    let old_margin_left = margin_left.get();
                    let old_margin_top = margin_top.get();
                    
                    // Deviation from initial click spot
                    let delta_x = abs_x - click_x.get();
                    let delta_y = abs_y - click_y.get();
                    
                    let new_margin_left = initial_margin_x.get() + delta_x;
                    let new_margin_top = initial_margin_y.get() + delta_y;
                    
                    margin_left.set(new_margin_left);
                    margin_top.set(new_margin_top);

                    // Compensate click reference point for physical window movement.
                    // Since GDK motion coordinate frame moves dynamically with the window,
                    // adjusting click baseline prevents visual feedback oscillation.
                    let moved_x = new_margin_left - old_margin_left;
                    let moved_y = new_margin_top - old_margin_top;
                    click_x.set(click_x.get() - moved_x);
                    click_y.set(click_y.get() - moved_y);
                    
                    window.set_layer_shell_margin(gtk_layer_shell::Edge::Left, new_margin_left);
                    window.set_layer_shell_margin(gtk_layer_shell::Edge::Top, new_margin_top);
                }
                gtk::glib::Propagation::Stop
            } else {
                gtk::glib::Propagation::Proceed
            }
        }
    ));

    // 6. CAIRO VECTORS AND PANGO TEXT DRAW HANDLER
    drawing_area.connect_draw(clone!(@strong time_text, @strong config => move |_, cr| {
        let text = time_text.borrow();
        if text.is_empty() { return gtk::glib::Propagation::Proceed; }

        let layout = pangocairo::create_layout(cr);
        layout.set_text(&text);
        
        let mut font_desc = pango::FontDescription::new();
        font_desc.set_family(&config.font_family);
        font_desc.set_size(config.size as i32 * pango::SCALE);
        font_desc.set_weight(parse_font_weight(&config.font_weight));
        layout.set_font_description(Some(&font_desc));
        layout.set_alignment(pango::Alignment::Center);

        // Translate context origin slightly to prevent boundary outline clipping
        cr.translate(config.thickness as f64, config.thickness as f64);

        let border_rgba = parse_hex(&config.border_color);
        let text_rgba = parse_hex(&config.color);

        // A) Build the math outline vector representation of the text layout
        pangocairo::functions::layout_path(cr, &layout);

        // B) Draw thick surrounding border stroke
        cr.set_source_rgba(border_rgba.0, border_rgba.1, border_rgba.2, border_rgba.3);
        cr.set_line_width(config.thickness as f64 * 2.0);
        cr.set_line_join(cairo::LineJoin::Round);
        let _ = cr.stroke_preserve(); // Preserve path for inner fill

        // C) Paint the inner path text area
        cr.set_source_rgba(text_rgba.0, text_rgba.1, text_rgba.2, text_rgba.3);
        let _ = cr.fill();

        gtk::glib::Propagation::Proceed
    }));

    let area_clone = drawing_area.clone();
    let format_string = Rc::new(config.format.replace("\\n", "\n"));
    
    // Initialize & query the time loop
    update_time(
        &area_clone, 
        &time_text, 
        &format_string, 
        config.size, 
        &config.font_family, 
        &config.font_weight, 
        config.thickness,
        config.demo
    );
    
    if !config.demo {
        glib::timeout_add_local(Duration::from_millis(1000), move || {
            update_time(
                &area_clone, 
                &time_text, 
                &format_string, 
                config.size, 
                &config.font_family, 
                &config.font_weight, 
                config.thickness,
                false
            );
            glib::ControlFlow::Continue
        });
    }

    apply_css();
    window.show_all();
}

// 7. TICK HANDLER - UPDATE STRING AND RESIZE WIDGET REQUESTS
fn update_time(
    area: &DrawingArea, 
    text_ref: &Rc<RefCell<String>>, 
    format_str: &str, 
    size: u32,
    font_family: &str,
    font_weight: &str,
    thickness: i32,
    demo: bool
) {
    let time_str = if demo {
        // Steve Jobs' legendary iPhone introduction announcement time (09:41:00 on Jan 9, 2007)
        let demo_dt = chrono::NaiveDate::from_ymd_opt(2007, 1, 9)
            .unwrap()
            .and_hms_opt(9, 41, 0)
            .unwrap();
        demo_dt.format(format_str).to_string()
    } else {
        let now = Local::now();
        now.format(format_str).to_string()
    };
    
    if *text_ref.borrow() != time_str {
        *text_ref.borrow_mut() = time_str.clone();

        // Calculate layout geometry bounds to resize GtkDrawingArea requests
        let layout = area.create_pango_layout(Some(&time_str));
        let mut font_desc = pango::FontDescription::new();
        font_desc.set_family(font_family);
        font_desc.set_size(size as i32 * pango::SCALE);
        font_desc.set_weight(parse_font_weight(font_weight));
        layout.set_font_description(Some(&font_desc));

        let (width, height) = layout.pixel_size();
        
        let padding = thickness * 2;
        area.set_size_request(width + padding, height + padding);
        area.queue_draw();
    }
}

// 8. SIMPLIFIED WIDGET CSS PROVIDERS
fn apply_css() {
    let css = "window { background-color: rgba(0, 0, 0, 0.01); }";

    let provider = CssProvider::new();
    provider.load_from_data(css.as_bytes()).expect("Failed loading CSS context");

    if let Some(screen) = gtk::gdk::Screen::default() {
        StyleContext::add_provider_for_screen(
            &screen,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

// =====================================================================
// RUNTIME STATE & DATA PERSISTENCE
// =====================================================================

/// Retrieves absolute path to position coordinate text file.
fn get_position_path() -> PathBuf {
    let config_dir = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").expect("Could not find environment variable HOME");
            PathBuf::from(home).join(".config")
        })
        .join("float-clock-wayland");
    
    std::fs::create_dir_all(&config_dir).ok();
    config_dir.join("position.txt")
}

/// Load position coordinates from persistence file.
fn load_position() -> (i32, i32) {
    let path = get_position_path();
    if let Ok(contents) = std::fs::read_to_string(path) {
        let parts: Vec<&str> = contents.trim().split(',').collect();
        if parts.len() == 2 {
            if let (Ok(x), Ok(y)) = (parts[0].parse::<i32>(), parts[1].parse::<i32>()) {
                return (x, y);
            }
        }
    }
    (50, 50)
}

/// Save position coordinates into persistence file.
fn save_position(x: i32, y: i32) {
    let path = get_position_path();
    let _ = std::fs::write(path, format!("{},{}", x, y));
}