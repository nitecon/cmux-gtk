/** Keep a native provider executor above the real client hook/tool subprocesses in CI. */
#include <errno.h>
#include <signal.h>
#include <sys/wait.h>
#include <unistd.h>

static pid_t child;
static void stop(int signal_number) { if (child > 0) kill(child, signal_number); }

int main(int argc, char **argv) {
    if (argc < 4) return 2; /* codex app-server python3 server.py ... */
    child = fork();
    if (child < 0) return 3;
    if (child == 0) { execvp(argv[2], argv + 2); _exit(4); }
    signal(SIGTERM, stop);
    signal(SIGINT, stop);
    int status = 0;
    while (waitpid(child, &status, 0) < 0) { if (errno != EINTR) return 5; }
    return WIFEXITED(status) ? WEXITSTATUS(status) : 6;
}
