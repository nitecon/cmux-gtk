#include <gtk/gtk.h>
#include <gdk/x11/gdkx.h>
#include <X11/Xutil.h>

int cmux_window_position(GdkSurface *surface, int *x, int *y, int restore) {
    if (!GDK_IS_X11_SURFACE(surface)) return 0;
    Display *display = gdk_x11_display_get_xdisplay(gdk_surface_get_display(surface));
    Window window = gdk_x11_surface_get_xid(surface);
    Window root = DefaultRootWindow(display);
    if (restore) {
        XSizeHints hints = {0};
        long supplied = 0;
        XGetWMNormalHints(display, window, &hints, &supplied);
        hints.flags |= USPosition | PWinGravity;
        hints.x = *x;
        hints.y = *y;
        hints.win_gravity = StaticGravity;
        XSetWMNormalHints(display, window, &hints);
        XMoveWindow(display, window, *x, *y);
        XFlush(display);
        return 1;
    }
    Window child;
    return XTranslateCoordinates(display, window, root, 0, 0, x, y, &child);
}
