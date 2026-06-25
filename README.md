# float-clock-wayland

An **always-on-top**, floating desktop clock widget and overlay for Wayland compositors (such as Hyprland, Sway, GNOME, and KDE). Built in Rust using GTK 3, Pango, Cairo, and `gtk-layer-shell`.

This lightweight Wayland clock widget is designed to function as an overlay that **always stays in front of all windows**, serving as a simple Linux/Wayland alternative inspired by the classic Windows utility **DS Clock**. It makes a perfect addition to custom Linux desktop configurations, status bars, and gaming setups.

> [!NOTE]
> This is a personal utility project created for custom Linux desktop configs. It is not a professional or production-ready application and may contain minor bugs depending on your compositor setup. Feel free to use or modify it for your own personal setups!

## Preview

![Demostration image of the float-clock-wayland widget on a Wayland desktop](demostration.png)

### Screenshot
![Screenshot of the feature goes here… assuming Future Me remembers to upload it.](screenshot.png)


### Demonstration Video
<!--
You can embed a demo video here.
Example: <video src="demo.mp4" width="600" controls></video>
-->
_[ ] Demonstration video of the feature goes here… assuming Future Me remembers to upload it._

---

## Features

- **Always-on-Top Overlay**: Configured to stay anchored at the highest window layer (`Overlay` / `Top`), ensuring the clock is always visible and stays in front of all active windows.
- **Smooth Window Dragging on Wayland**:
  - **Hyprland Backend**: Connects directly to Hyprland's IPC socket to track the global cursor coordinates for perfect drag-and-drop movement.
  - **Generic Backend**: Implements a custom mathematical drag-compensation control loop. This stabilizes window movement on other Wayland compositors (Sway, GNOME, KDE) by eliminating margin-feedback loop oscillations.
- **Double-Buffered Vector Graphics**: Uses Cairo vector paths to render high-contrast text with thick outlines, preventing boundary clipping or pixelation.
- **Position Persistence**: Automatically writes to and reads from `~/.config/float-clock-wayland/position.txt` so the clock widget remembers its exact desktop coordinates.
- **Config file generation**: Automatically initializes a default `config.toml` on the first run.

---

## Prerequisites

This widget binds to the system's native GUI libraries. To build the project from source, you will need the development packages (headers and link libraries) for the following components installed on your system:

- **GTK 3** (with GDK Wayland support)
- **gtk-layer-shell** (for Wayland layer panel mapping)
- **Pango** (for font loading and layout sizing)
- **Cairo** (for custom vector drawing)
- **pkg-config** (to locate development files during compilation)

*Note: If you are already running a Wayland compositor along with custom status bars or desktop widgets, **it is highly likely that most of these libraries are already installed on your system**. Otherwise, they can easily be found in your package manager under names like `gtk3-devel`, `libgtk-3-dev`, `gtk-layer-shell-devel`, or similar.*


---

## Compilation & Run

1. **Build the project**:
   ```bash
   cargo build --release
   ```
   The compiled binary will be placed at `./target/release/float-clock-wayland`.

2. **Execute the widget**:
   ```bash
   ./target/release/float-clock-wayland
   ```

---

## Usage & Controls

- **Move Widget**: Click and hold **Left Mouse Button (LMB)** to drag the always-on-top clock to any screen position.
- **Close Widget**: Click **Right Mouse Button (RMB)** on the clock to terminate the process.

---

## Configuration

Settings are saved in the user's config directory:
`~/.config/float-clock-wayland/config.toml`

### Options

| Setting | Type | Default | Description |
|---|---|---|---|
| `size` | Integer | `11` | Font size in Pango points. |
| `color` | String | `"#a6afb2ff"` | HEX color code for text fill. Supports transparency (e.g. `#00b7ff88`) and shortcodes. |
| `border_color`| String | `"#000000"` | HEX color code for outline border. |
| `thickness` | Integer | `3` | Outline border thickness in pixels. |
| `format` | String | `"%H:%M:%S\n%d/%m/%Y"`| Time formatting layout (Chrono compatible). Use `\n` for line breaks. |
| `font_family` | String | `JetBrains Mono...`| System font family stack. |
| `font_weight` | String | `"heavy"` | Pango font weight (`thin`, `light`, `normal`, `medium`, `bold`, `heavy`). |
| `backend` | String | `"auto"` | Coordinate tracking backend (`auto`, `hyprland`, `generic`). |
| `save_position`| Boolean | `true` | Enables/disables saving window coordinates on drag. |
| `demo` | Boolean | `false` | If true, locks the clock at a static demo time (09:41:00 on Jan 9, 2007) and stops scheduling updates to save CPU. |

### CLI Overrides

You can temporarily override any TOML setting via CLI flags (for example, to freeze the time for a desktop screenshot):
```bash
./target/release/float-clock-wayland --demo
```
Or combine multiple options:
```bash
./target/release/float-clock-wayland --size 12 --color "#ffffff" --backend generic
```
Run with `--help` to list all arguments.

---

## Autostart Setup

### 1. Hyprland (Lua integration)
If you configure your desktop autostart using a Lua framework, add:

```lua
hl.on("hyprland.start", function ()
    -- [... other startup executions]
    hl.exec_cmd("/path/to/bin/float-clock-wayland &")
end)
```

### 2. Standard Hyprland (`hyprland.conf`)
Add to `~/.config/hypr/hyprland.conf`:
```ini
exec-once = /path/to/bin/float-clock-wayland &
```

### 3. Sway config
Add to `~/.config/sway/config`:
```ini
exec /path/to/bin/float-clock-wayland &
```

---

*Keywords: wayland-clock, hyprland-clock-widget, always-on-top-clock, floating-clock-linux, overlay-clock-wayland, rust-wayland-widget, linux-desktop-widget, status-bar-clock, floating-window-wayland*
