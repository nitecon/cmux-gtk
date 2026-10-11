/** Actions-only foreground process: record exact terminal key bytes with Kitty release reporting. */
#include <stdio.h>
#include <termios.h>
#include <unistd.h>

/** Own raw stdin and a caller-owned log until termination; fail on setup or recording errors. */
int main(int argc, char **argv) {
    if (argc != 2) return 2;
    FILE *log = fopen(argv[1], "wb");
    if (!log) return 3;
    struct termios original, raw;
    if (tcgetattr(STDIN_FILENO, &original) != 0) { fclose(log); return 4; }
    raw = original;
    cfmakeraw(&raw);
    if (tcsetattr(STDIN_FILENO, TCSANOW, &raw) != 0) { fclose(log); return 5; }
    /* Disambiguate keys, report releases and encode all keys, as requested by a TUI. */
    printf("\033[>11uCMUX key recorder ready\r\n");
    fflush(stdout);
    char bytes[4096];
    ssize_t count;
    int result = 0;
    while ((count = read(STDIN_FILENO, bytes, sizeof(bytes))) > 0) {
        if (fwrite(bytes, 1, (size_t)count, log) != (size_t)count || fflush(log) != 0) {
            result = 6;
            break;
        }
    }
    printf("\033[<u");
    fflush(stdout);
    tcsetattr(STDIN_FILENO, TCSANOW, &original);
    fclose(log);
    return result;
}
