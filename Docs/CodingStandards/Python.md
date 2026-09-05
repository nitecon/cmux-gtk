# Python

Use Python 3, four-space indentation, descriptive snake_case names and standard-library tools when sufficient. Document modules and functions with docstrings describing contracts rather than restating names. [PEP 8](https://peps.python.org/pep-0008/) and [PEP 257](https://peps.python.org/pep-0257/).

Integration scenarios use isolated directories, bounded waits, explicit process ownership and finally blocks for cleanup. Prefer argument lists to shell execution. Share socket/protocol and polling helpers. Report meaningful failure logs without dumping secrets. Assert externally observable behavior, not source fragments. Keep tests portable unless the scenario explicitly exercises Linux. Run tests in GitHub Actions only.
