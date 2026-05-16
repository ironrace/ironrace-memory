import pytest
from src.example import Greeter

def test_greet_returns_hello():
    assert Greeter().greet("world") == "hello, world!"

class TestGreeter:
    def test_default_greeting(self):
        assert Greeter().greeting == "hello"
