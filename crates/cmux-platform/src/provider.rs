//! Provider invocation roles shared by Linux and Windows process inspection.

/// Utility commands are not model executors, even when the executable has the provider name.
pub(crate) fn executor_role(provider: &str, command: &[u8]) -> bool {
    let args: Vec<_> = command
        .split(|b| *b == 0)
        .filter(|arg| !arg.is_empty())
        .collect();
    if provider == "claude" {
        return true; // Native Claude and its verified official Node entrypoint own execution.
    }
    if args
        .iter()
        .any(|a| matches!(*a, b"--help" | b"-h" | b"--version" | b"-V"))
    {
        return false;
    }
    let mut index = 1;
    while let Some(arg) = args.get(index) {
        if matches!(
            *arg,
            b"-c"
                | b"--config"
                | b"-m"
                | b"--model"
                | b"-p"
                | b"--profile"
                | b"--remote"
                | b"--remote-auth-token-env"
                | b"-C"
                | b"--cd"
                | b"-s"
                | b"--sandbox"
                | b"-a"
                | b"--ask-for-approval"
                | b"--local-provider"
                | b"--enable"
                | b"--disable"
                | b"--add-dir"
                | b"--code-mode-host"
                | b"--listen"
        ) {
            if args
                .get(index + 1)
                .is_none_or(|value| value.starts_with(b"-"))
            {
                return false;
            }
            index += 2;
            continue;
        }
        if !arg.starts_with(b"-") {
            return match *arg {
                b"app-server" => !args.iter().any(|a| {
                    matches!(
                        *a,
                        b"generate-json-schema" | b"generate-ts" | b"proxy" | b"daemon"
                    )
                }),
                b"exec" | b"e" | b"review" => true,
                b"resume" | b"fork" => args.contains(&b"--no-daemon".as_slice()),
                _ => false,
            };
        }
        index += 1;
    }
    args.contains(&b"--no-daemon".as_slice())
}
