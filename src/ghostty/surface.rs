use crate::ghostty::ffi;
use gtk4::glib;
use std::cell::RefCell;
use std::rc::Rc;

/// I/O mode for a Ghostty surface.
/// - `Exec`: normal mode — Ghostty spawns a local shell process.
/// - `Manual`: SSH remote mode — keystrokes route through io_write_cb to the SSH bridge.
#[derive(Clone)]
pub enum SurfaceIoMode {
    Exec,
    Command(String),
    Remote {
        bridge: std::sync::Arc<crate::ssh::bridge::SshBridge>,
        ssh_tx: crate::ssh::SshEventTx,
    },
    Manual {
        bridge: std::sync::Arc<crate::ssh::bridge::SshBridge>,
        io_write_ctx: std::sync::Arc<crate::ssh::bridge::IoWriteContext>,
    },
}

struct SurfaceInit {
    ghostty_app: ffi::ghostty_app_t,
    inherited_config: Option<super::inherited::InheritedConfig>,
    working_directory: Option<std::path::PathBuf>,
    pane_id: u64,
    io_mode: SurfaceIoMode,
    retired: Rc<std::cell::Cell<bool>>,
}

/// Initialize Ghostty only after GTK has assigned a non-zero GLArea size.
/// GtkNotebook realizes pages before allocating them, and creating Ghostty at
/// 0x0 loses the shell's startup output before the terminal grid is usable.
fn initialize_surface(
    area: &gtk4::GLArea,
    cell: &Rc<RefCell<Option<ffi::ghostty_surface_t>>>,
    init: &SurfaceInit,
    logical_width: i32,
    logical_height: i32,
) -> Option<ffi::ghostty_surface_t> {
    use gtk4::prelude::*;
    use std::sync::atomic::Ordering;

    if init.retired.get() {
        return None;
    }
    if let Some(surface) = *cell.borrow() {
        return Some(surface);
    }
    if logical_width <= 0 || logical_height <= 0 || !area.is_realized() {
        return None;
    }

    area.make_current();
    if let Some(err) = area.error() {
        eprintln!("cmux: GLArea initialization error: {err}");
        return None;
    }

    let scale = area.scale_factor() as f64;
    let surface =
        unsafe {
            let platform = ffi::ghostty_platform_u {
                opengl: ffi::ghostty_platform_opengl_s {
                    userdata: area.as_ptr() as *mut std::ffi::c_void,
                    make_current: Some(cmux_platform::opengl::make_current),
                    clear_current: Some(cmux_platform::opengl::clear_current),
                    get_proc_address: Some(cmux_platform::opengl::get_proc_address),
                    swap_buffers: Some(cmux_platform::opengl::swap_buffers),
                },
            };
            let mut config = init
                .inherited_config
                .as_ref()
                .map(super::inherited::InheritedConfig::config)
                .unwrap_or_else(|| ffi::ghostty_surface_config_new());
            config.platform_tag = ffi::ghostty_platform_e_GHOSTTY_PLATFORM_OPENGL;
            config.platform = platform;
            config.userdata = area.as_ptr() as *mut std::ffi::c_void;
            config.scale_factor = scale;

            let working_directory_c = init
                .working_directory
                .as_ref()
                .and_then(|path| std::ffi::CString::new(path.to_string_lossy().as_bytes()).ok());
            if let Some(ref cwd) = working_directory_c {
                config.working_directory = cwd.as_ptr();
            }
            let command_c = match &init.io_mode {
                SurfaceIoMode::Command(command) => std::ffi::CString::new(command.as_str()).ok(),
                _ => None,
            };
            if let Some(command) = &command_c {
                config.command = command.as_ptr();
            }
            // Keep owned strings and the merged array alive through ghostty_surface_new.
            // Appended entries replace inherited terminal identity in Ghostty's environment map.
            let identity = area
                .data::<uuid::Uuid>("cmux-surface-uuid")
                .map(|identity| identity.as_ref().to_string());
            let socket = cmux_platform::paths::socket_path()
                .to_string_lossy()
                .into_owned();
            let environment_strings: Vec<_> = identity
                .into_iter()
                .flat_map(|identity| {
                    [
                        ("CMUX_SURFACE_ID", identity),
                        ("CMUX_SOCKET_PATH", socket.clone()),
                        ("CMUX_SOCKET", socket.clone()),
                    ]
                })
                .map(|(key, value)| {
                    (
                        std::ffi::CString::new(key).unwrap(),
                        std::ffi::CString::new(value).unwrap(),
                    )
                })
                .collect();
            let mut environment = if config.env_vars.is_null() || config.env_var_count == 0 {
                Vec::new()
            } else {
                std::slice::from_raw_parts(config.env_vars, config.env_var_count).to_vec()
            };
            environment.extend(environment_strings.iter().map(|(key, value)| {
                ffi::ghostty_env_var_s {
                    key: key.as_ptr(),
                    value: value.as_ptr(),
                }
            }));
            config.env_vars = environment.as_mut_ptr();
            config.env_var_count = environment.len();
            if let SurfaceIoMode::Manual {
                ref io_write_ctx, ..
            } = init.io_mode
            {
                config.io_mode = ffi::ghostty_surface_io_mode_e_GHOSTTY_SURFACE_IO_MANUAL;
                config.io_write_cb = Some(crate::ssh::bridge::ssh_io_write_cb);
                config.io_write_userdata =
                    std::sync::Arc::as_ptr(io_write_ctx) as *mut std::ffi::c_void;
            }
            eprintln!(
                "cmux: initializing Ghostty surface at {}x{} logical pixels",
                logical_width, logical_height
            );
            if let Some(size) = crate::preferences::saved_font_size() {
                config.font_size = size;
            }
            let surface = ffi::ghostty_surface_new(init.ghostty_app, &config);
            // The embedded command is borrowed by Ghostty's shell argv. Keep its
            // C allocation alive until this surface has been freed, including when
            // the IO thread starts the subprocess after surface_new returns.
            if let Some(command) = command_c {
                area.set_data("cmux-launch-command", command);
            }
            if surface.is_null() {
                eprintln!("cmux: FATAL — ghostty_surface_new returned null");
                std::process::exit(1);
            }
            let phys_width = (logical_width as f64 * scale) as u32;
            let phys_height = (logical_height as f64 * scale) as u32;
            ffi::ghostty_surface_set_size(surface, phys_width, phys_height);
            ffi::ghostty_surface_set_content_scale(surface, scale, scale);
            ffi::ghostty_surface_set_focus(surface, true);
            surface
        };

    crate::ghostty::registry::register(
        surface as usize,
        init.pane_id,
        init.working_directory.as_deref(),
    );
    *cell.borrow_mut() = Some(surface);
    area.grab_focus();
    if let Ok(mut registry) = crate::ghostty::callbacks::GL_TO_SURFACE.lock() {
        registry.insert(area.as_ptr() as usize, surface as usize);
    }
    if let SurfaceIoMode::Manual {
        bridge,
        io_write_ctx,
    } = &init.io_mode
    {
        io_write_ctx
            .surface_ptr
            .store(surface as usize, Ordering::Release);
        let size = unsafe { ffi::ghostty_surface_size(surface) };
        io_write_ctx.resize(size.columns, size.rows);
        bridge.register_pane_placeholder(io_write_ctx.pane_id);
    }
    area.queue_render();
    Some(surface)
}

/// Create a terminal widget on the GTK main thread using an existing Ghostty app.
/// Native surface creation waits for realization and non-zero allocation. The widget
/// owns initialization and cleanup callbacks; the returned shared cell exposes the
/// current surface pointer and becomes empty when the native surface is released.
pub fn create_surface(
    ghostty_app: ffi::ghostty_app_t,
    inherited_config: Option<super::inherited::InheritedConfig>,
    working_directory: Option<std::path::PathBuf>,
    pane_id: u64,
    io_mode: SurfaceIoMode,
) -> (gtk4::GLArea, Rc<RefCell<Option<ffi::ghostty_surface_t>>>) {
    use gtk4::prelude::*;
    eprintln!(
        "cmux: create_surface called for pane_id={}, inherited_config={}",
        pane_id,
        inherited_config.is_some()
    );

    let gl_area = gtk4::GLArea::new();
    eprintln!(
        "cmux: created GLArea {:p} for pane_id={}",
        gl_area.as_ptr(),
        pane_id
    );
    // Ghostty's embedded renderer expects desktop OpenGL. GTK 4.8 has no
    // per-GLArea API selector, so application startup sets GDK's gl-prefer-gl.
    // Per Pitfall 1: require OpenGL 4.3 before the area is realized.
    gl_area.set_required_version(4, 3);
    // Manual render mode: only render when wakeup_cb schedules queue_render().
    // An independent render loop adds input latency (per CLAUDE.md pitfall).
    gl_area.set_auto_render(false);
    // Must be focusable to receive keyboard events via EventControllerKey.
    gl_area.set_focusable(true);
    // Grab keyboard focus when the user clicks inside the terminal.
    gl_area.set_focus_on_click(true);
    // Expand to fill available space — required for GtkPaned to distribute space evenly.
    // Without this, GLArea has natural size 0 and the Paned gives all space to end child.
    gl_area.set_hexpand(true);
    gl_area.set_vexpand(true);

    // Shared cell for the surface pointer — created once the GL context and a
    // non-zero allocation exist, then used by the remaining callbacks.
    // Rc<RefCell<...>> is safe here: all callbacks run on the GLib main thread.
    let surface_cell: Rc<RefCell<Option<ffi::ghostty_surface_t>>> = Rc::new(RefCell::new(None));
    unsafe {
        gl_area.set_data("cmux-surface-cell", surface_cell.clone());
    }
    let io_mode = match io_mode {
        SurfaceIoMode::Remote { bridge, ssh_tx } => {
            let io_write_ctx = bridge.create_context(ssh_tx);
            SurfaceIoMode::Manual {
                bridge,
                io_write_ctx,
            }
        }
        other => other,
    };
    if let SurfaceIoMode::Manual {
        bridge,
        io_write_ctx,
    } = &io_mode
    {
        unsafe {
            gl_area.set_data(
                "cmux-remote-context",
                (bridge.clone(), io_write_ctx.clone()),
            );
        }
    }
    let retired = Rc::new(std::cell::Cell::new(false));
    unsafe {
        gl_area.set_data("cmux-surface-retired", retired.clone());
    }
    let surface_init = Rc::new(SurfaceInit {
        ghostty_app,
        inherited_config,
        working_directory,
        pane_id,
        io_mode,
        retired,
    });

    // ── GtkGLArea::realize ───────────────────────────────────────────────────
    // The GL context is valid after realize. If GTK has already allocated a
    // usable size, create the terminal here; otherwise resize does it later.
    //
    // IMPORTANT: GTK may re-realize the widget when reparenting (e.g., moving from
    // GtkStack into GtkPaned during split). We must check if the surface already
    // exists and reuse it, otherwise we create orphaned surfaces that never render.
    let pane_id_for_log = pane_id;
    gl_area.connect_realize({
        let cell = surface_cell.clone();
        let init = surface_init.clone();
        move |area| {
            eprintln!(
                "cmux: GLArea {:p} realize for pane_id={} — making GL context current",
                area.as_ptr(),
                pane_id_for_log
            );
            area.make_current();
            if let Some(err) = area.error() {
                eprintln!("cmux: GLArea realize error: {err}");
                std::process::exit(1); // Per D-09: no GUI error dialog in Phase 1
            }

            // Check if surface already exists (re-realize after reparent).
            // If so, just update the size/scale and refresh — don't create a new surface.
            //
            // DO NOT restore focus here. During a split, the old pane is reparented into
            // a GtkPaned — it should NOT regain focus. The new pane gets focus via its own
            // fresh realize path (set_focus(true) + grab_focus). EventControllerFocus
            // handles focus restoration automatically when GTK gives this widget focus back
            // (via `enter` signal). Calling set_focus(true) here incorrectly marks the old
            // pane as focused, causing both panes to have focused=true simultaneously and
            // triggering Ghostty's early-return guard on the new pane's subsequent focus calls.
            if let Some(existing_surface) = *cell.borrow() {
                eprintln!(
                    "cmux: GLArea {:p} re-realized — reinitializing GL resources for surface {:p}",
                    area.as_ptr(),
                    existing_surface
                );
                let scale = area.scale_factor() as f64;
                let w = area.width();
                let h = area.height();
                unsafe {
                    ffi::ghostty_surface_display_realized(existing_surface);
                    let phys_w = (w as f64 * scale) as u32;
                    let phys_h = (h as f64 * scale) as u32;
                    if phys_w > 0 && phys_h > 0 {
                        ffi::ghostty_surface_set_size(existing_surface, phys_w, phys_h);
                    }
                    ffi::ghostty_surface_set_content_scale(existing_surface, scale, scale);
                    ffi::ghostty_surface_refresh(existing_surface);
                }
                area.queue_render();
                return;
            }

            let w = area.width();
            let h = area.height();
            if initialize_surface(area, &cell, &init, w, h).is_none() {
                eprintln!(
                    "cmux: deferring Ghostty initialization until non-zero resize ({}x{})",
                    w, h
                );
            }
        }
    });

    // ── GtkGLArea::unrealize — free renderer GL resources before context dies
    {
        let pane_id_unrealize = pane_id;
        let cell_unrealize = surface_cell.clone();
        gl_area.connect_unrealize(move |area| {
            eprintln!(
                "cmux: GLArea {:p} pane={} UNREALIZE — freeing GL resources",
                area.as_ptr(),
                pane_id_unrealize,
            );
            if let Some(surface) = *cell_unrealize.borrow() {
                let is_registered = crate::ghostty::callbacks::GL_TO_SURFACE
                    .lock()
                    .ok()
                    .and_then(|registry| registry.get(&(area.as_ptr() as usize)).copied())
                    == Some(surface as usize);
                if is_registered {
                    area.make_current();
                }
                if is_registered && area.error().is_none() {
                    unsafe { ffi::ghostty_surface_display_unrealized(surface) };
                }
            }
        });
    }

    // ── GtkGLArea::render ────────────────────────────────────────────────────
    // Called by GTK frame clock when queue_render() was requested.
    gl_area.connect_render({
        let cell = surface_cell.clone();
        move |_area, _ctx| {
            if let Some(surface) = *cell.borrow() {
                unsafe {
                    ffi::ghostty_surface_draw(surface);
                }
            }
            gtk4::glib::Propagation::Stop // suppress GTK default render
        }
    });

    // ── GtkGLArea::resize ────────────────────────────────────────────────────
    // GTK provides logical (CSS) pixels; Ghostty needs physical pixels (Pitfall 5).
    //
    // CRITICAL: ghostty_surface_set_size must be called SYNCHRONOUSLY in this
    // signal handler, not deferred to an idle. Ghostty's renderer anti-flicker
    // guard in drawFrame() compares GL_VIEWPORT (the actual widget size) against
    // the renderer's cached screen size. If we defer set_size to an idle, the
    // next drawFrame(true) — triggered by queue_render — sees a size mismatch
    // and re-presents the last frame forever (the guard returns before updating
    // the renderer's cached size). This matches Ghostty's own GTK apprt which
    // calls sizeCallback directly in glareaResize, not deferred.
    //
    // sizeCallback early-returns when the size hasn't changed, so redundant
    // calls during rapid drag are cheap (just a comparison, no reflow).
    //
    // Do NOT bounce focus (false→true) here. The cursor blink timer is
    // independent of resize — ghostty_surface_set_size only calls setScreenSize
    // on the renderer; it does not cancel the blink timer. A false→true bounce
    // can race the native renderer timer cancellation.
    {
        let cell = surface_cell.clone();
        let init = surface_init.clone();
        gl_area.connect_resize(move |area, _framebuffer_w, _framebuffer_h| {
            let scale = area.scale_factor();
            // GtkGLArea's resize signal reports framebuffer pixels while
            // Widget::width/height are logical pixels. Ghostty expects physical
            // pixels, so derive them from the widget allocation exactly once.
            let logical_w = area.width();
            let logical_h = area.height();
            let phys_w = (logical_w * scale) as u32;
            let phys_h = (logical_h * scale) as u32;

            if let Some(surface) = initialize_surface(area, &cell, &init, logical_w, logical_h) {
                unsafe {
                    ffi::ghostty_surface_set_size(surface, phys_w, phys_h);
                    if let SurfaceIoMode::Manual { io_write_ctx, .. } = &init.io_mode {
                        let size = ffi::ghostty_surface_size(surface);
                        io_write_ctx.resize(size.columns, size.rows);
                    }
                }
            }

            // Drive the render loop directly — wakeup idles can be starved during
            // rapid resize events (sustained mouse drag floods the GLib main loop
            // with motion events at DEFAULT priority, delaying DEFAULT_IDLE wakeup
            // idles). Calling app_tick + queue_render here ensures the terminal
            // reflows and re-renders even during a sustained resize drag.
            let app_ptr =
                crate::ghostty::callbacks::APP_PTR.load(std::sync::atomic::Ordering::SeqCst);
            if app_ptr != 0 {
                unsafe {
                    let app = app_ptr as ffi::ghostty_app_t;
                    ffi::ghostty_app_tick(app);
                }
            }
            if area.is_mapped() {
                area.queue_render();
                area.queue_draw();
            }
        });
    }

    // ── notify::scale-factor (GHOST-06) ─────────────────────────────────────
    // Fires when the window moves to a monitor with a different DPI.
    // Must use connect_notify_local: ghostty_surface_t is *mut c_void (not Send+Sync).
    // connect_notify_local only requires 'static, and runs on the GLib main thread.
    gl_area.connect_notify_local(Some("scale-factor"), {
        let cell = surface_cell.clone();
        move |widget, _| {
            if let Some(surface) = *cell.borrow() {
                // GdkSurface::scale() is GTK 4.12+, so retain compatibility with
                // Debian 12 by using the integer widget scale factor.
                let scale = widget.scale_factor() as f64;
                eprintln!(
                    "cmux: scale-factor changed to {} for surface {:p}",
                    scale, surface
                );
                unsafe {
                    ffi::ghostty_surface_set_content_scale(surface, scale, scale);
                    ffi::ghostty_surface_refresh(surface); // trigger redraw at new scale
                }
            }
        }
    });

    // ── Key input (GHOST-03) ─────────────────────────────────────────────────────
    // EventControllerKey fires key-pressed and key-released events.
    // CRITICAL: no allocations in this path — per CLAUDE.md typing-latency-sensitive paths.
    let key_controller = gtk4::EventControllerKey::new();
    key_controller.connect_key_pressed({
        let cell = surface_cell.clone();
        move |_ctrl, keyval, keycode, state| {
            use crate::ghostty::input::map_mods;

            let surface = match *cell.borrow() {
                Some(s) => s,
                None => return gtk4::glib::Propagation::Proceed,
            };

            // Handle Linux clipboard shortcuts at the terminal, leaving entries alone.
            let modifiers = state
                & (gtk4::gdk::ModifierType::CONTROL_MASK
                    | gtk4::gdk::ModifierType::SHIFT_MASK
                    | gtk4::gdk::ModifierType::ALT_MASK
                    | gtk4::gdk::ModifierType::SUPER_MASK);
            if modifiers
                == (gtk4::gdk::ModifierType::CONTROL_MASK | gtk4::gdk::ModifierType::SHIFT_MASK)
            {
                let action: Option<&[u8]> = match keyval.to_lower() {
                    gtk4::gdk::Key::c => Some(b"copy_to_clipboard"),
                    gtk4::gdk::Key::v => Some(b"paste_from_clipboard"),
                    _ => None,
                };
                if let Some(action) = action {
                    unsafe {
                        ffi::ghostty_surface_binding_action(
                            surface,
                            action.as_ptr().cast(),
                            action.len(),
                        );
                    }
                    return gtk4::glib::Propagation::Stop;
                }
            }

            // text field: UTF-8 from the keyval (what the key produces with modifiers applied).
            // Must be a C string. Use a stack-allocated buffer to avoid heap allocation.
            let unicode = keyval.to_unicode();
            let mut text_buf = [0u8; 8]; // UTF-8: max 4 bytes + null
            let text_ptr = if let Some(ch) = unicode {
                let mut s = [0u8; 5];
                let encoded = ch.encode_utf8(&mut s[..4]);
                let len = encoded.len();
                text_buf[..len].copy_from_slice(encoded.as_bytes());
                text_buf[len] = 0;
                text_buf.as_ptr() as *const i8
            } else {
                std::ptr::null()
            };

            let mut input = unsafe { std::mem::zeroed::<ffi::ghostty_input_key_s>() };
            // keycode must be the raw GTK hardware keycode (XKB scancode).
            // Ghostty looks this up in its own native keycodes table to resolve the physical key.
            // Do NOT translate to ghostty_input_key_e here — that is an entirely different type.
            input.keycode = keycode;
            input.mods = map_mods(state);
            input.action = ffi::ghostty_input_action_e_GHOSTTY_ACTION_PRESS;
            input.text = text_ptr;
            input.consumed_mods = 0; // Not used in Phase 1

            unsafe {
                ffi::ghostty_surface_key(surface, input);
            }
            gtk4::glib::Propagation::Stop // Inhibit: prevent GTK from handling the key
        }
    });
    key_controller.connect_key_released({
        let cell = surface_cell.clone();
        move |_ctrl, _keyval, keycode, state| {
            use crate::ghostty::input::map_mods;

            let surface = match *cell.borrow() {
                Some(s) => s,
                None => return,
            };

            let mut input = unsafe { std::mem::zeroed::<ffi::ghostty_input_key_s>() };
            input.keycode = keycode;
            input.mods = map_mods(state);
            input.action = ffi::ghostty_input_action_e_GHOSTTY_ACTION_RELEASE;
            input.text = std::ptr::null();
            input.consumed_mods = 0; // Not used in Phase 1
            unsafe {
                ffi::ghostty_surface_key(surface, input);
            }
        }
    });
    gl_area.add_controller(key_controller);

    // ── Mouse button input (GHOST-04) ────────────────────────────────────────────
    let click_gesture = gtk4::GestureClick::new();
    click_gesture.set_button(0); // 0 = listen to all mouse buttons
    click_gesture.connect_pressed({
        let cell = surface_cell.clone();
        let area = gl_area.downgrade();
        move |gesture, _n_press, _x, _y| {
            let Some(area) = area.upgrade() else {
                return;
            };
            let _ = area.activate_action("win.focus-pane", Some(&pane_id.to_variant()));
            area.grab_focus();
            let surface = match *cell.borrow() {
                Some(s) => s,
                None => return,
            };
            let button = match gesture.current_button() {
                1 => ffi::ghostty_input_mouse_button_e_GHOSTTY_MOUSE_LEFT,
                2 => ffi::ghostty_input_mouse_button_e_GHOSTTY_MOUSE_MIDDLE,
                3 => ffi::ghostty_input_mouse_button_e_GHOSTTY_MOUSE_RIGHT,
                _ => return,
            };
            let mods = crate::ghostty::input::map_mods(gesture.current_event_state());
            unsafe {
                ffi::ghostty_surface_mouse_button(
                    surface,
                    ffi::ghostty_input_mouse_state_e_GHOSTTY_MOUSE_PRESS,
                    button,
                    mods,
                );
            }
        }
    });
    click_gesture.connect_released({
        let cell = surface_cell.clone();
        move |gesture, _n_press, _x, _y| {
            let surface = match *cell.borrow() {
                Some(s) => s,
                None => return,
            };
            let button = match gesture.current_button() {
                1 => ffi::ghostty_input_mouse_button_e_GHOSTTY_MOUSE_LEFT,
                2 => ffi::ghostty_input_mouse_button_e_GHOSTTY_MOUSE_MIDDLE,
                3 => ffi::ghostty_input_mouse_button_e_GHOSTTY_MOUSE_RIGHT,
                _ => return,
            };
            let mods = crate::ghostty::input::map_mods(gesture.current_event_state());
            unsafe {
                ffi::ghostty_surface_mouse_button(
                    surface,
                    ffi::ghostty_input_mouse_state_e_GHOSTTY_MOUSE_RELEASE,
                    button,
                    mods,
                );
            }
        }
    });
    gl_area.add_controller(click_gesture);

    // ── Mouse motion ─────────────────────────────────────────────────────────────
    let motion_controller = gtk4::EventControllerMotion::new();
    motion_controller.connect_motion({
        let cell = surface_cell.clone();
        move |ctrl, x, y| {
            let surface = match *cell.borrow() {
                Some(s) => s,
                None => return,
            };
            let mods = crate::ghostty::input::map_mods(ctrl.current_event_state());
            unsafe {
                crate::ghostty::ffi::ghostty_surface_mouse_pos(surface, x, y, mods);
            }
        }
    });
    gl_area.add_controller(motion_controller);

    // ── Focus tracking (GHOST-05) ─────────────────────────────────────────────
    // EventControllerFocus fires enter/leave when GTK keyboard focus enters/leaves the widget.
    // This ensures ghostty_surface_set_focus() stays in sync with GTK focus routing —
    // critical after GtkPaned drags (separator steals focus) and sidebar show/hide.
    // Without this, Ghostty's internal focused flag diverges from GTK reality, and
    // subsequent set_focus(true) calls hit the early-return guard (if self.focused == focused { return; }).
    let focus_controller = gtk4::EventControllerFocus::new();
    focus_controller.connect_enter({
        let cell = surface_cell.clone();
        let gl_area_for_focus = gl_area.downgrade();
        move |_ctrl| {
            crate::diagnostics::event(format_args!("terminal focus entered pane={pane_id}"));
            if let Some(surface) = *cell.borrow() {
                unsafe {
                    ffi::ghostty_surface_set_focus(surface, true);
                    // Kick the render loop so the cursor becomes visible immediately
                    // rather than waiting up to one blink interval (~500ms). The
                    // renderer thread processes the focused=true message asynchronously;
                    // without a refresh+queue_render here, GTK renders that happen
                    // before the message is processed show the stale (invisible) cursor.
                    ffi::ghostty_surface_refresh(surface);
                }
                if let Some(area) = gl_area_for_focus.upgrade() {
                    area.queue_render();
                }
            }
        }
    });
    focus_controller.connect_leave({
        let cell = surface_cell.clone();
        move |_ctrl| {
            if let Some(surface) = *cell.borrow() {
                unsafe {
                    ffi::ghostty_surface_set_focus(surface, false);
                }
            }
        }
    });
    gl_area.add_controller(focus_controller);

    // ── Scroll input ─────────────────────────────────────────────────────────────
    let scroll_controller = gtk4::EventControllerScroll::new(
        gtk4::EventControllerScrollFlags::BOTH_AXES | gtk4::EventControllerScrollFlags::DISCRETE,
    );
    scroll_controller.connect_scroll({
        let cell = surface_cell.clone();
        move |ctrl, dx, dy| {
            let surface = match *cell.borrow() {
                Some(s) => s,
                None => return gtk4::glib::Propagation::Proceed,
            };
            // Detect if this is pixel-precise (touchpad) or discrete (mouse wheel)
            let is_pixel = ctrl
                .current_event()
                .and_then(|e| e.downcast::<gtk4::gdk::ScrollEvent>().ok())
                .map(|se| se.direction() == gtk4::gdk::ScrollDirection::Smooth)
                .unwrap_or(false);

            // ghostty_input_scroll_mods_t is a bitmask:
            // bit 0: scroll_is_pixel (1 if touchpad, 0 if mouse wheel)
            // bit 1: momentum (1 if momentum scrolling)
            let scroll_mods = if is_pixel { 1 } else { 0 };

            unsafe {
                ffi::ghostty_surface_mouse_scroll(surface, dx, dy, scroll_mods);
            }
            gtk4::glib::Propagation::Stop
        }
    });
    gl_area.add_controller(scroll_controller);

    (gl_area, surface_cell)
}

// ── Clipboard callbacks ──────────────────────────────────────────────────────

/// Read the requested clipboard asynchronously, completing only for the same live surface.
///
/// # Safety
/// Ghostty must call on the GTK thread with a live GLArea as userdata and its own
/// outstanding request token. Surface teardown must unregister the area before freeing it.
pub(crate) unsafe extern "C" fn read_clipboard_cb(
    userdata: *mut std::ffi::c_void,
    clipboard_type: crate::ghostty::ffi::ghostty_clipboard_e,
    request: *mut std::ffi::c_void,
) -> bool {
    use gtk4::prelude::*;
    let area_key = userdata as usize;
    let Some(surface_ptr) = clipboard_surface(area_key) else {
        return false;
    };
    let area: glib::translate::Borrowed<gtk4::GLArea> =
        glib::translate::from_glib_borrow(userdata.cast::<gtk4::ffi::GtkGLArea>());
    let Some(cell) = area.data::<Rc<RefCell<Option<ffi::ghostty_surface_t>>>>("cmux-surface-cell")
    else {
        return false;
    };
    let cell = cell.as_ref().clone();

    let display = match gtk4::gdk::Display::default() {
        Some(d) => d,
        None => return false,
    };
    let clipboard = if clipboard_type == ffi::ghostty_clipboard_e_GHOSTTY_CLIPBOARD_SELECTION {
        display.primary_clipboard()
    } else {
        display.clipboard()
    };

    glib::MainContext::default().spawn_local(async move {
        let result = clipboard.read_text_future().await;
        // The requesting pane may have closed while the clipboard owner replied.
        if cell.borrow().map(|surface| surface as usize) != Some(surface_ptr)
            || clipboard_surface(area_key) != Some(surface_ptr)
        {
            return;
        }
        let text = result
            .ok()
            .flatten()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let text = std::ffi::CString::new(text.replace('\0', "")).unwrap();
        unsafe {
            ffi::ghostty_surface_complete_clipboard_request(
                surface_ptr as ffi::ghostty_surface_t,
                text.as_ptr(),
                request,
                true,
            );
        }
    });
    true
}

/// Resolve a registered live surface without dereferencing its GLArea pointer.
fn clipboard_surface(area_key: usize) -> Option<usize> {
    crate::ghostty::callbacks::GL_TO_SURFACE
        .lock()
        .ok()?
        .get(&area_key)
        .copied()
}

/// Complete Ghostty's confirmation callback using the application's existing allow policy.
///
/// # Safety
/// Call on GTK's thread with Ghostty's readable C value and outstanding request token;
/// userdata identifies the registered surface that owns that request.
pub(crate) unsafe extern "C" fn confirm_read_clipboard_cb(
    userdata: *mut std::ffi::c_void,
    value: *const std::os::raw::c_char,
    request: *mut std::ffi::c_void,
    _request_type: crate::ghostty::ffi::ghostty_clipboard_request_e,
) {
    let Some(surface_ptr) = clipboard_surface(userdata as usize) else {
        return;
    };
    unsafe {
        crate::ghostty::ffi::ghostty_surface_complete_clipboard_request(
            surface_ptr as crate::ghostty::ffi::ghostty_surface_t,
            value,
            request,
            true,
        );
    }
}

/// Copy the first UTF-8 content item to the regular or primary selection clipboard.
///
/// # Safety
/// Call on GTK's thread. Non-null content must reference `len` readable entries;
/// each non-null data pointer must be a readable NUL-terminated string for this call.
pub(crate) unsafe extern "C" fn write_clipboard_cb(
    _userdata: *mut std::ffi::c_void,
    clipboard_type: crate::ghostty::ffi::ghostty_clipboard_e,
    content: *const crate::ghostty::ffi::ghostty_clipboard_content_s,
    len: usize,
    _confirm: bool,
) {
    use gtk4::prelude::*;

    if content.is_null() || len == 0 {
        return;
    }
    let item = &*content;
    let text = if !item.data.is_null() {
        match std::ffi::CStr::from_ptr(item.data).to_str() {
            Ok(s) => s.to_string(),
            Err(_) => return,
        }
    } else {
        return;
    };

    let display = match gtk4::gdk::Display::default() {
        Some(d) => d,
        None => return,
    };
    let clipboard = if clipboard_type == ffi::ghostty_clipboard_e_GHOSTTY_CLIPBOARD_SELECTION {
        display.primary_clipboard()
    } else {
        display.clipboard()
    };
    clipboard.set_text(&text);
}

#[cfg(test)]
mod clipboard_integration_tests {
    use super::*;
    use gtk4::prelude::*;
    use std::sync::atomic::Ordering;

    /// Service GTK until the asserted asynchronous state arrives or its deadline expires.
    fn pump_until(mut ready: impl FnMut() -> bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !ready() {
            assert!(
                std::time::Instant::now() < deadline,
                "GTK clipboard condition timed out"
            );
            while glib::MainContext::default().pending() {
                glib::MainContext::default().iteration(false);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// Give X11 input and clipboard events time to reach the application.
    fn settle() {
        let until = std::time::Instant::now() + std::time::Duration::from_millis(300);
        pump_until(|| std::time::Instant::now() >= until);
    }

    /// Inject real X11 input and drain the resulting GTK events.
    fn xdo(args: &[&str]) {
        assert!(std::process::Command::new("xdotool")
            .args(args)
            .status()
            .unwrap()
            .success());
        settle();
    }

    /// Verify shortcut, primary selection and asynchronous paste routing across live panes.
    #[test]
    #[ignore = "real X11/Ghostty clipboard integration; run under Xvfb in CI"]
    fn linux_clipboard_shortcuts_primary_and_surface_routing() {
        crate::ghostty::gtk_environment::configure();
        gtk4::init().unwrap();
        let root = std::env::temp_dir().join(format!("cmux-clipboard-{}", uuid::Uuid::new_v4()));
        let first = root.join("first");
        let second = root.join("second");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        let ghostty = unsafe {
            let arg = std::ffi::CString::new("cmux-clipboard-test").unwrap();
            let mut args = [arg.as_ptr() as *mut i8];
            ffi::ghostty_init(1, args.as_mut_ptr());
            let config = ffi::ghostty_config_new();
            let text = b"font-size = 12\nshell-integration = none\ncopy-on-select = true\nkeybind = ctrl+enter=text:\\r\n";
            ffi::ghostty_config_load_string(
                config,
                text.as_ptr().cast(),
                text.len(),
                c"test".as_ptr(),
            );
            ffi::ghostty_config_finalize(config);
            let runtime = ffi::ghostty_runtime_config_s {
                userdata: std::ptr::null_mut(),
                supports_selection_clipboard: true,
                wakeup_cb: Some(crate::ghostty::callbacks::wakeup_cb),
                action_cb: Some(crate::ghostty::callbacks::action_cb),
                read_clipboard_cb: Some(read_clipboard_cb),
                confirm_read_clipboard_cb: Some(confirm_read_clipboard_cb),
                write_clipboard_cb: Some(write_clipboard_cb),
                close_surface_cb: Some(crate::ghostty::callbacks::close_surface_cb),
                tmux_control_cb: None,
            };
            let ghostty = ffi::ghostty_app_new(&runtime, config);
            ffi::ghostty_config_free(config);
            assert!(!ghostty.is_null());
            crate::ghostty::callbacks::APP_PTR.store(ghostty as usize, Ordering::SeqCst);
            ghostty
        };
        let (left, left_cell) = create_surface(
            ghostty,
            None,
            Some(first.clone()),
            900001,
            SurfaceIoMode::Command(
                "/bin/sh -c 'printf \"\\033[2J\\033[HCMUXPRIMARY\\n\"; exec /bin/sh'".into(),
            ),
        );
        let (right, right_cell) = create_surface(
            ghostty,
            None,
            Some(second.clone()),
            900002,
            SurfaceIoMode::Exec,
        );
        let content = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        content.set_homogeneous(true);
        content.append(&left);
        content.append(&right);
        let window = gtk4::Window::builder()
            .title("cmux-clipboard-integration")
            .default_width(900)
            .default_height(400)
            .decorated(false)
            .child(&content)
            .build();
        window.present();
        pump_until(|| left_cell.borrow().is_some() && right_cell.borrow().is_some());
        settle();
        let window_id = std::process::Command::new("xdotool")
            .args(["search", "--name", "^cmux-clipboard-integration$"])
            .output()
            .unwrap();
        let id = String::from_utf8(window_id.stdout)
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .to_string();
        xdo(&["windowfocus", &id]);
        let display = gtk4::gdk::Display::default().unwrap();
        unsafe {
            let surface = left_cell.borrow().unwrap();
            let mut text: ffi::ghostty_text_s = std::mem::zeroed();
            if ffi::ghostty_surface_read_screen_clipboard_text(surface, 0, 20, 4096, &mut text) {
                if !text.text.is_null() {
                    eprintln!(
                        "clipboard fixture screen: {}",
                        String::from_utf8_lossy(std::slice::from_raw_parts(
                            text.text.cast(),
                            text.text_len
                        ))
                    );
                }
                ffi::ghostty_surface_free_text(surface, &mut text);
            }
        }
        // Select a printed word with real X11 mouse events. PRIMARY must update without Copy.
        xdo(&[
            "mousemove",
            "--window",
            &id,
            "45",
            "10",
            "click",
            "--repeat",
            "2",
            "--delay",
            "150",
            "1",
        ]);
        let selected = glib::MainContext::default()
            .block_on(display.primary_clipboard().read_text_future())
            .unwrap()
            .unwrap();
        assert_eq!(selected.as_str(), "CMUXPRIMARY");
        xdo(&["key", "--clearmodifiers", "ctrl+shift+c"]);
        let copied = glib::MainContext::default()
            .block_on(display.clipboard().read_text_future())
            .unwrap()
            .unwrap();
        assert_eq!(copied.as_str(), "CMUXPRIMARY");
        // Both terminals are live; paste must complete against the requesting
        // terminal without changing the other terminal's directory or input.
        display
            .clipboard()
            .set_text("printf standard > standard-result");
        xdo(&["key", "--clearmodifiers", "ctrl+shift+v"]);
        xdo(&["key", "Return"]);
        pump_until(|| first.join("standard-result").exists());
        assert!(!second.join("standard-result").exists());
        display
            .primary_clipboard()
            .set_text("printf primary > primary-result");
        xdo(&["mousemove", "--window", &id, "150", "70", "click", "2"]);
        xdo(&["key", "Return"]);
        pump_until(|| first.join("primary-result").exists());
        assert_eq!(
            std::fs::read_to_string(first.join("primary-result")).unwrap(),
            "primary"
        );
        // Exercise an explicitly configured Ghostty binding through GTK/X11 input.
        xdo(&[
            "type",
            "--clearmodifiers",
            "printf bound > ctrl-enter-result",
        ]);
        assert!(!first.join("ctrl-enter-result").exists());
        xdo(&["key", "--clearmodifiers", "ctrl+Return"]);
        pump_until(|| first.join("ctrl-enter-result").exists());
        assert_eq!(
            std::fs::read_to_string(first.join("ctrl-enter-result")).unwrap(),
            "bound"
        );
        assert!(!second.join("ctrl-enter-result").exists());
        crate::split_engine::destroy_terminal_area(&left);
        crate::split_engine::destroy_terminal_area(&right);
        window.destroy();
        crate::ghostty::callbacks::APP_PTR.store(0, Ordering::SeqCst);
        unsafe {
            ffi::ghostty_app_free(ghostty);
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
