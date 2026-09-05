"""Small result record for retained script-based integration scenarios."""


class TestResult:
    """A named scenario outcome that defaults to failure until explicitly passed."""

    def __init__(self, name: str):
        """Initialize the scenario name with no explanation and a failed outcome."""
        self.name = name
        self.passed = False
        self.message = ""

    def success(self, msg: str = ""):
        """Replace any prior outcome with success and the supplied explanation."""
        self.passed = True
        self.message = msg

    def failure(self, msg: str):
        """Replace any prior outcome with failure and the supplied explanation."""
        self.passed = False
        self.message = msg
