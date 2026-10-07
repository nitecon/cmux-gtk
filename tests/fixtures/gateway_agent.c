/** Native foreground-agent fixture for CI only: editable prompts, bracketed paste and submitted input logs. */
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <termios.h>
#include <unistd.h>

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
    char input[65536] = {0}, mode[64], sequence[16] = {0};
    size_t length = 0, sequence_length = 0;
    int paste = 0;
    printf("\033[?2004h\033]7;file://localhost%s\007", argv[3]);
    for (;;) {
        state(argv[1], mode, sizeof(mode));
        if (strcmp(mode, "exit") == 0 || strcmp(mode, "shell") == 0) break;
        render(mode, marker, length < 128 ? input : "");
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
                FILE *log = fopen(argv[2], "a");
                if (!log) { tcsetattr(STDIN_FILENO, TCSANOW, &original); return 5; }
                fprintf(log, "%s\n===SUBMITTED===\n", input); fclose(log);
                length = 0; input[0] = '\0';
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
