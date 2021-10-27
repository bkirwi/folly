#!/bin/bash
cd "$(dirname "$0")"

TERP="../../target/debug/examples/term"

# Unit tests
python regtest.py -i "$TERP" czech.z3.regtest
python regtest.py -i "$TERP" czech.z4.regtest
python regtest.py -i "$TERP" czech.z5.regtest
python regtest.py -i "$TERP" czech.z8.regtest
python regtest.py -i "$TERP" praxix.z5.regtest

# Game tests
python regtest.py -i "$TERP" curses.z3.regtest
python regtest.py -i "$TERP" minizork.z3.regtest
