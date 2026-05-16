"""Example module for Python AST tests."""

CONSTANT_X = 42

class Greeter:
    """A simple greeter."""

    greeting: str = "hello"

    def greet(self, name: str) -> str:
        """Return a greeting for *name*."""
        return f"{self.greeting}, {name}!"

async def async_op(x: int) -> int:
    return x + 1

def _private(): ...
