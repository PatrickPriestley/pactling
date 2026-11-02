#!/usr/bin/env python3
"""Transparent stdio tee: forwards bytes unchanged; it does not implement MCP."""
import json
import os
import subprocess
import sys
import threading

binary, capture = sys.argv[1:]
process = subprocess.Popen([binary, 'mcp'], stdin=subprocess.PIPE, stdout=subprocess.PIPE)
lock = threading.Lock()

def record(direction, line):
    with lock, open(capture, 'a') as output:
        output.write(json.dumps({'direction': direction, 'message': json.loads(line)}) + '\n')

def forward_input():
    for line in sys.stdin.buffer:
        record('client_to_server', line)
        process.stdin.write(line)
        process.stdin.flush()
    process.stdin.close()

threading.Thread(target=forward_input, daemon=True).start()
for line in process.stdout:
    record('server_to_client', line)
    sys.stdout.buffer.write(line)
    sys.stdout.buffer.flush()
sys.exit(process.wait())
