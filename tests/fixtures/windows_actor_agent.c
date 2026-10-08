/** Native ConPTY provider fixture; ordinary tools record actual model input. */
#include <windows.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/** Run the Python relay below this native provider process, keeping prompt bytes out of shell arguments. */
static int relay(char **args, const char *prompt) {
    FILE *file = fopen(args[6], "wb");
    if (!file) return -1;
    fwrite(prompt, 1, strlen(prompt), file);
    fclose(file);
    char command[8192];
    int size = snprintf(command, sizeof(command), "\"%s\" \"%s\" --relay \"%s\" \"%s\" \"%s\" \"%s\" \"%s\"",
                        args[2], args[3], args[4], args[5], args[6], args[7], args[8]);
    if (size < 0 || size >= (int)sizeof(command)) return -1;
    WCHAR wide[8192];
    if (!MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, command, -1, wide, 8192)) return -1;
    STARTUPINFOW startup = {0};
    PROCESS_INFORMATION child = {0};
    startup.cb = sizeof(startup);
    if (!CreateProcessW(NULL, wide, NULL, NULL, TRUE, 0, NULL, NULL, &startup, &child)) return -1;
    DWORD status = 1;
    if (WaitForSingleObject(child.hProcess, 20000) != WAIT_OBJECT_0) TerminateProcess(child.hProcess, 1);
    GetExitCodeProcess(child.hProcess, &status);
    CloseHandle(child.hThread);
    CloseHandle(child.hProcess);
    return status == 0 ? 0 : -1;
}

/** Render the same bounded Codex composer used by the Linux native readiness fixture. */
static void render(void) {
    printf("\033[2J\033[HWindows actor fixture\r\n\033[48;2;65;69;76m\033[K\r\n"
           "\033[1m\xe2\x80\xba \033[22m\033[2mAsk Codex to do anything\033[22m\033[K\r\n"
           "\033[K\033[0m\r\n  GPT-6 · project · Context 77%% left\r\n"
           "  ? for shortcuts\r\n\033[?25h\033[3;3H");
    fflush(stdout);
}

/** Own a raw native console and relay complete bracketed-paste submissions to ordinary model tools. */
int main(int argc, char **argv) {
    if (argc != 9 || strcmp(argv[1], "exec") != 0) return 2;
    SetConsoleCP(CP_UTF8);
    SetConsoleOutputCP(CP_UTF8);
    HANDLE input = GetStdHandle(STD_INPUT_HANDLE), output = GetStdHandle(STD_OUTPUT_HANDLE);
    DWORD original_input, original_output;
    if (!GetConsoleMode(input, &original_input) || !GetConsoleMode(output, &original_output)) return 3;
    if (!SetConsoleMode(input, ENABLE_VIRTUAL_TERMINAL_INPUT) ||
        !SetConsoleMode(output, original_output | ENABLE_VIRTUAL_TERMINAL_PROCESSING)) return 4;
    if (relay(argv, "") < 0) return 5;
    printf("\033[?2004h");
    char bytes[4096], text[65536] = {0}, sequence[16] = {0};
    size_t length = 0, sequence_length = 0;
    int paste = 0;
    for (;;) {
        render();
        DWORD count = 0;
        if (!ReadFile(input, bytes, sizeof(bytes), &count, NULL) || !count) break;
        for (DWORD i = 0; i < count; ++i) {
            unsigned char value = (unsigned char)bytes[i];
            if (sequence_length || value == 27) {
                if (sequence_length + 1 >= sizeof(sequence)) { sequence_length = 0; continue; }
                sequence[sequence_length++] = (char)value;
                sequence[sequence_length] = 0;
                if (value == '~') {
                    if (strcmp(sequence, "\033[200~") == 0) paste = 1;
                    if (strcmp(sequence, "\033[201~") == 0) paste = 0;
                    sequence_length = 0;
                }
                continue;
            }
            if (!paste && (value == '\r' || value == '\n')) {
                if (length && relay(argv, text) < 0) return 6;
                length = 0; text[0] = 0;
            } else if (length + 1 < sizeof(text)) {
                text[length++] = (char)value; text[length] = 0;
            }
        }
    }
    SetConsoleMode(input, original_input);
    SetConsoleMode(output, original_output);
    return 0;
}
