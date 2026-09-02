#!/bin/sh
# Stable desktop-entry launcher for the packaged application binary. Backend
# selection remains under GTK and the user's environment; cmux configures the
# desktop OpenGL preference before GTK initializes.
exec /usr/bin/cmux-app.bin "$@"
