# Shell

Declare the interpreter. Use Bash only where its features are needed. Quote expansions, use arrays for argument lists, and preserve command exit status. Explain intentional failures explicitly rather than broadly appending `|| true`. Use cleanup traps for temporary resources. [Shell style guide](https://google.github.io/styleguide/shellguide.html).

Document each function with a preceding comment describing inputs, output, side effects and failures where meaningful. Keep shell limited to orchestration; put complex parsing in the application or Python. Do not duplicate release version parsing, paths or packaging rules. Keep installation idempotent, respect user-selected tools, and do not evaluate untrusted text as shell code. Exercise behavior in CI and run syntax/lint checks as appropriate.
