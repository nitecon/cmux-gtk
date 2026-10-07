/** Native foreground-agent fixture for CI only: editable prompts, bracketed paste and submitted input logs. */
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <termios.h>
#include <unistd.h>
#include <stdint.h>
#include <arpa/inet.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <sys/un.h>

static void state(const char *path, char *value, size_t size);
static int send_all(int descriptor, const void *data, size_t length) {
    const char *bytes = data;
    while (length) {
        ssize_t count = write(descriptor, bytes, length);
        if (count <= 0) return 0;
        bytes += count; length -= (size_t)count;
    }
    return 1;
}

/** Optional real-client hook relay: the shared executor owns all client subprocesses, never this frontend. */
static int hook(const char *prompt) {
    const char *endpoint = getenv("CMUX_FIXTURE_ACTOR_BROKER");
    const char *native_file = getenv("CMUX_FIXTURE_NATIVE_FILE");
    if (!endpoint || !native_file) return 0;
    char native[257];
    state(native_file, native, sizeof(native));
    struct sockaddr_un address = { .sun_family = AF_UNIX };
    if (strlen(endpoint) >= sizeof(address.sun_path)) return -1;
    strcpy(address.sun_path, endpoint);
    int connection = socket(AF_UNIX, SOCK_STREAM, 0);
    if (connection < 0 || connect(connection, (struct sockaddr *)&address, sizeof(address)) != 0) {
        if (connection >= 0) close(connection);
        return -1;
    }
    struct timeval timeout = { .tv_sec = 12 };
    setsockopt(connection, SOL_SOCKET, SO_RCVTIMEO, &timeout, sizeof(timeout));
    const char *parts[] = {native, prompt ? prompt : ""};
    char kind = prompt ? 'U' : 'S';
    int good = send_all(connection, &kind, 1);
    for (int i = 0; i < 2 && good; ++i) {
        uint32_t length = htonl((uint32_t)strlen(parts[i]));
        good = send_all(connection, &length, sizeof(length))
            && send_all(connection, parts[i], strlen(parts[i]));
    }
    char blocked = 0;
    if (!good || read(connection, &blocked, 1) != 1) blocked = -1;
    close(connection);
    return blocked;
}

/** Read caller-owned state on each tick; absence defaults to an idle, editable prompt. */
static void state(const char *path, char *value, size_t size) {
    strcpy(value, "idle");
    FILE *file = fopen(path, "r");
    if (file) {
        if (!fgets(value, (int)size, file)) strcpy(value, "idle");
        fclose(file);
        value[strcspn(value, "\r\n")] = '\0';
    }
}

/** Emit a deterministic provider prompt and live caret without exposing fixture controls to the application. */
static void render(const char *mode, const char *marker, const char *draft) {
    if (strcmp(marker, "›") == 0) {
        printf("\033[2J\033[H%s\r\n", strcmp(mode, "busy") == 0 ?
               "Working · esc to interrupt" : "Gateway fixture: permission checks preserved");
        printf("\033[48;2;65;69;76m\033[K\r\n\033[1m%s \033[22m", marker);
        if (*draft) printf("%s", draft);
        else printf("\033[2mAsk Codex to do anything\033[22m");
        printf("\033[K\r\n%s\033[K\033[0m\r\n", strcmp(mode, "multiline") == 0 ? "second draft line" : "");
        printf("  GPT-6.1-Sol high · ~/project · Context 77%% left\r\n");
        printf("%s\r\n", strcmp(mode, "permission") == 0 ?
               "Allow once · ? for shortcuts" : "  ← for agents · ? for shortcuts");
        printf("\033[?25h\033[3;%zuH", 3 + strlen(draft));
        fflush(stdout);
        return;
    }
    printf("\033[2J\033[HGateway fixture\r\n");
    printf("%s %s\r\n", marker, draft);
    printf("%s\r\n", strcmp(mode, "busy") == 0 ? "esc to interrupt" :
           strcmp(mode, "permission") == 0 ? "Allow once · ? for shortcuts" : "? for shortcuts");
    printf("\033[?25h\033[2;%zuH", 3 + strlen(draft));
    fflush(stdout);
}

/** Own raw stdin until exit; log only submissions outside bracketed paste, retaining exact message delimiters. */
int main(int argc, char **argv) {
    if (argc != 4) return 2;
    struct termios original, raw;
    if (tcgetattr(STDIN_FILENO, &original) != 0) return 3;
    raw = original;
    cfmakeraw(&raw);
    if (tcsetattr(STDIN_FILENO, TCSANOW, &raw) != 0) return 4;
    const char *marker = strstr(argv[0], "claude") ? "❯" : "›";
    char input[65536] = {0}, mode[64], previous_mode[64] = {0}, sequence[16] = {0};
    size_t length = 0, sequence_length = 0;
    int paste = 0;
    printf("\033[?2004h\033]7;file://localhost%s\007", argv[3]);
    if (hook(NULL) < 0) return 6;
    for (;;) {
        state(argv[1], mode, sizeof(mode));
        if (strcmp(mode, "exit") == 0 || strcmp(mode, "shell") == 0) break;
        /* Incremental mode keeps untouched rows, like a TUI that trusts its previous frame. */
        if (strcmp(mode, "incremental") != 0 || strcmp(mode, previous_mode) != 0)
            render(mode, marker, length < 128 ? input : "");
        strcpy(previous_mode, mode);
        struct pollfd descriptor = {STDIN_FILENO, POLLIN, 0};
        int ready = poll(&descriptor, 1, 250);
        if (ready < 0) break;
        if (!ready) continue;
        char bytes[4096];
        ssize_t count = read(STDIN_FILENO, bytes, sizeof(bytes));
        if (count <= 0) break;
        for (ssize_t i = 0; i < count; ++i) {
            unsigned char c = (unsigned char)bytes[i];
            if (sequence_length || c == 27) {
                if (sequence_length + 1 >= sizeof(sequence)) { sequence_length = 0; continue; }
                sequence[sequence_length++] = (char)c;
                sequence[sequence_length] = '\0';
                if (c == '~') {
                    if (strcmp(sequence, "\033[200~") == 0) paste = 1;
                    if (strcmp(sequence, "\033[201~") == 0) paste = 0;
                    sequence_length = 0;
                }
                continue;
            }
            if (!paste && c == 21) { length = 0; input[0] = '\0'; continue; }
            if (!paste && (c == '\r' || c == '\n')) {
                if (hook(input) < 0) { tcsetattr(STDIN_FILENO, TCSANOW, &original); return 6; }
                FILE *log = fopen(argv[2], "a");
                if (!log) { tcsetattr(STDIN_FILENO, TCSANOW, &original); return 5; }
                fprintf(log, "%s\n===SUBMITTED===\n", input); fclose(log);
                length = 0; input[0] = '\0';
                if (strcmp(mode, "incremental") == 0) {
                    /* Repaint only the response heading; preserve composer, footer and caret. */
                    printf("\033[1;1H\033[2KGateway fixture: submission received\033[3;3H");
                    fflush(stdout);
                }
                continue;
            }
            if (length + 1 < sizeof(input)) { input[length++] = (char)c; input[length] = '\0'; }
        }
    }
    printf("\033[?2004l\033[2J\033[H"); fflush(stdout);
    tcsetattr(STDIN_FILENO, TCSANOW, &original);
    if (strcmp(mode, "shell") == 0) execl("/bin/bash", "bash", "--noprofile", "--norc", (char *)NULL);
    return 0;
}
