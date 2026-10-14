/** GTK owns the borrowed GdkSurface. Resolve its Win32 HWND without taking ownership. */
#include <windows.h>
#include <gdk/gdk.h>
#include <gdk/win32/gdkwin32.h>

/** Read or request outer-window coordinates on the GTK thread; return zero on native failure. */
int cmux_window_position(GdkSurface *surface, int *x, int *y, int restore) {
    if (!surface || !x || !y || !GDK_IS_WIN32_SURFACE(surface)) return 0;
    HWND window = gdk_win32_surface_get_handle(surface);
    if (!window) return 0;
    if (restore) return SetWindowPos(window, NULL, *x, *y, 0, 0,
                                    SWP_NOACTIVATE | SWP_NOSIZE | SWP_NOZORDER) != 0;
    RECT rectangle;
    if (!GetWindowRect(window, &rectangle)) return 0;
    *x = rectangle.left;
    *y = rectangle.top;
    return 1;
}
