import os
import sys as system
from collections import OrderedDict


class Account:
    def __init__(self, id):
        self.id = id

    def deposit(self, n):
        return os.path.join(str(n))


def top_level(x):
    return Account(x).deposit(1)
