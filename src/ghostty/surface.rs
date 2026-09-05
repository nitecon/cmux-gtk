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
    Manual {
        io_write_ctx: std::sync::Arc<crate::ssh::bridge::IoWriteContext>,
    },
}

unsafe extern "C" fn opengl_make_current(userdata: *mut std::ffi::c_void) -> bool {
    if userdata.is_null() {
        return false;
    }
    let area = userdata.cast();
    let context = gtk4::ffi::gtk_gl_area_get_context(area);
    if !context.is_null() && gtk4::gdk::ffi::gdk_gl_context_get_current() == context {
        return true;
    }
    gtk4::ffi::gtk_gl_area_make_current(area);
    gtk4::ffi::gtk_gl_area_get_error(area).is_null()
}

unsafe extern "C" fn opengl_clear_current(_userdata: *mut std::ffi::c_void) {
    // GtkGLArea owns the context for the full render callback. Clearing it
    // here breaks GTK's post-render compositing and libepoxy dispatch.
}

unsafe extern "C" fn opengl_get_proc_address(
    _userdata: *mut std::ffi::c_void,
    name: *const std::ffi::c_char,
) -> *mut std::ffi::c_void {
    extern "C" {
        fn glXGetProcAddressARB(name: *const u8) -> *mut std::ffi::c_void;
    }
    glXGetProcAddressARB(name.cast())
}

unsafe extern "C" fn opengl_swap_buffers(_userdata: *mut std::ffi::c_void) {
    // GtkGLArea presents its framebuffer after the render signal returns.
}

struct SurfaceInit {
    ghostty_app: ffi::ghostty_app_t,
    inherited_config: Option<ffi::ghostty_surface_config_s>,
    working_directory: Option<std::path::PathBuf>,
    pane_id: u64,
    io_mode: SurfaceIoMode,
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
    let surface = unsafe {
        let platform = ffi::ghostty_platform_u {
            opengl: ffi::ghostty_platform_opengl_s {
                userdata: area.as_ptr() as *mut std::ffi::c_void,
                make_current: Some(opengl_make_current),
                clear_current: Some(opengl_clear_current),
                get_proc_address: Some(opengl_get_proc_address),
                swap_buffers: Some(opengl_swap_buffers),
            },
        };
        let mut config = init
            .inherited_config
            .unwrap_or_else(|| ffi::ghostty_surface_config_new());
        config.platform_tag = ffi::ghostty_platform_e_GHOSTTY_PLATFORM_OPENGL;
        config.platform = platform;
        config.userdata = std::ptr::null_mut();
        config.scale_factor = scale;

        let working_directory_c = init
            .working_directory
            .as_ref()
            .and_then(|path| std::ffi::CString::new(path.to_string_lossy().as_bytes()).ok());
        if let Some(ref cwd) = working_directory_c {
            config.working_directory = cwd.as_ptr();
        }
        if let SurfaceIoMode::Manual { ref io_write_ctx } = init.io_mode {
            config.io_mode = ffi::ghostty_surface_io_mode_e_GHOSTTY_SURFACE_IO_MANUAL;
            config.io_write_cb = Some(crate::ssh::bridge::ssh_io_write_cb);
            config.io_write_userdata =
                std::sync::Arc::into_raw(io_write_ctx.clone()) as *mut std::ffi::c_void;
        }
        eprintln!(
            "cmux: initializing Ghostty surface at {}x{} logical pixels",
            logical_width, logical_height
        );
        if let Some(size) = crate::preferences::saved_font_size() {
            config.font_size = size;
        }
        let surface = ffi::ghostty_surface_new(init.ghostty_app, &config);
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

    if let Ok(mut registry) = crate::ghostty::callbacks::SURFACE_REGISTRY.lock() {
        registry.insert(surface as usize, init.pane_id);
    }
    *cell.borrow_mut() = Some(surface);
    area.grab_focus();
    crate::ghostty::callbacks::SURFACE_PTR.store(surface as usize, Ordering::SeqCst);
    if let Ok(mut areas) = crate::ghostty::callbacks::GL_AREA_REGISTRY.lock() {
        areas.push(crate::ghostty::callbacks::GtkGLAreaPtr(
            area.as_ptr() as *mut gtk4::ffi::GtkGLArea
        ));
    }
    if let Ok(mut registry) = crate::ghostty::callbacks::GL_TO_SURFACE.lock() {
        registry.insert(area.as_ptr() as usize, surface as usize);
    }
    area.queue_render();
    Some(surface)
}

/// Creates and returns a GtkGLArea with a Ghostty terminal surface wired up.
/// Initializes ghostty_app_t, then defers ghostty_surface_t creation until the
/// GtkGLArea is realized and has received a non-zero allocation.
pub fn create_surface(
    _app: &gtk4::Application,
    ghostty_app: ffi::ghostty_app_t,
    inherited_config: Option<ffi::ghostty_surface_config_s>,
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
    let surface_init = Rc::new(SurfaceInit {
        ghostty_app,
        inherited_config,
        working_directory,
        pane_id,
        io_mode,
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
    // kills the timer via an async cancel race (see restore_active_pane_focus).
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
            if let Ok(areas) = crate::ghostty::callbacks::GL_AREA_REGISTRY.lock() {
                for area_ptr in areas.iter() {
                    let area: glib::translate::Borrowed<gtk4::GLArea> =
                        unsafe { glib::translate::from_glib_borrow(area_ptr.0) };
                    if area.is_realized() {
                        area.queue_render();
                        area.queue_draw(); // Gap 1B: repaints CSS border
                    }
                }
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
        let area = gl_area.clone();
        move |gesture, _n_press, _x, _y| {
            let _ = area.activate_action(
                "win.focus-pane",
                Some(&pane_id.to_variant()),
            );
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
        let gl_area_for_focus = gl_area.clone();
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
                gl_area_for_focus.queue_render();
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

pub(crate) unsafe extern "C" fn read_clipboard_cb(
    _userdata: *mut std::ffi::c_void,
    clipboard_type: crate::ghostty::ffi::ghostty_clipboard_e,
    request: *mut std::ffi::c_void,
) -> bool {
    use gtk4::prelude::*;
    use std::sync::atomic::Ordering;

    let surface_ptr = crate::ghostty::callbacks::SURFACE_PTR.load(Ordering::SeqCst);
    if surface_ptr == 0 {
        return false;
    }
    let surface = surface_ptr as ffi::ghostty_surface_t;

    let display = match gtk4::gdk::Display::default() {
        Some(d) => d,
        None => return false,
    };
    let clipboard = if clipboard_type == ffi::ghostty_clipboard_e_GHOSTTY_CLIPBOARD_SELECTION {
        display.primary_clipboard()
    } else {
        display.clipboard()
    };

    // Read clipboard text synchronously using GLib event loop.
    // gtk4::glib::MainContext::block_on runs the async future on the current (main) thread.
    // This is safe here because read_clipboard_cb is called from the GLib main thread.
    let text_result = glib::MainContext::default().block_on(clipboard.read_text_future());

    let c_text = match text_result {
        Ok(Some(ref s)) => match std::ffi::CString::new(s.as_str()) {
            Ok(text) => text,
            Err(_) => return false,
        },
        _ => return false,
    };

    unsafe {
        ffi::ghostty_surface_complete_clipboard_request(surface, c_text.as_ptr(), request, true);
    }
    true
}

pub(crate) unsafe extern "C" fn confirm_read_clipboard_cb(
    _userdata: *mut std::ffi::c_void,
    value: *const std::os::raw::c_char,
    surface_ptr: *mut std::ffi::c_void,
    _request_type: crate::ghostty::ffi::ghostty_clipboard_request_e,
) {
    // Phase 1: auto-confirm all clipboard reads without a dialog (per D-09).
    // surface_ptr (arg3) is the ghostty_surface_t — passed back to complete_clipboard_request.
    // _request_type is informational only; we always confirm.
    // complete_clipboard_request's 3rd arg (*mut c_void) is NULL for non-request-based calls.
    unsafe {
        crate::ghostty::ffi::ghostty_surface_complete_clipboard_request(
            surface_ptr as crate::ghostty::ffi::ghostty_surface_t,
            value,
            std::ptr::null_mut(), // no pending request object in confirm path
            true,
        );
    }
}

pub(crate) unsafe extern "C" fn write_clipboard_cb(
    _userdata: *mut std::ffi::c_void,
    clipboard_type: crate::ghostty::ffi::ghostty_clipboard_e,
    content: *const crate::ghostty::ffi::ghostty_clipboard_content_s,
    _len: usize,
    _confirm: bool,
) {
    use gtk4::prelude::*;

    if content.is_null() {
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
