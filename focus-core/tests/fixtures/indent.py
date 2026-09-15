# Python formatted by ruff. Its indentation cannot be worked out from the
# text - a dedent is the only thing saying a block has ended, and stripping
# the indentation throws that away - so the test types it back in with the
# indents it means and requires that nothing moves.
import os


class Thing:
    def __init__(self, name):
        self.name = name
        self.parts = [
            "one",
            "two",
        ]

    def describe(self, verbose):
        if verbose and self.name:
            for part in self.parts:
                print(part)
        elif verbose:
            print("no name")
        else:
            print(os.path.basename(self.name))
        try:
            total = sum(len(part) for part in self.parts)
        except TypeError:
            total = 0
        finally:
            self.total = total
        while total > 0:
            total -= 1
        return total
