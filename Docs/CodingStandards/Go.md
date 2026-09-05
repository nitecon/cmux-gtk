# Go

Use the module-declared toolchain, gofmt, explicit error returns and small packages. Document functions with adjacent Go comments stating behavior; exported comments begin with the symbol name. Wrap errors with operation context and preserve errors for inspection. [Effective Go](https://go.dev/doc/effective_go).

The remote daemon owns PTYs and child processes. Every goroutine needs a termination path; every opened resource needs one cleanup owner. Bound messages and output, propagate cancellation, and avoid locks around blocking network writes. Keep wire parsing separate from session lifecycle. Tests invoke real behavior, include disconnect and malformed-message cases, and run in GitHub Actions. Avoid abstractions for hypothetical remote backends.
