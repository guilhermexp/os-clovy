#!/usr/bin/python3
"""Source-only Chrome pipe relay for the deterministic private-host regression.

This file is never copied into a browser bundle. It forwards the owned fixture's
CDP bytes without injecting commands. A test HTTP image latch changes the page
between the host's field inventory and its first AX request.
"""
import http.client
import json
import os
from pathlib import Path
import select
import signal
import sys
import threading
import time

config = json.loads(Path(sys.argv[1]).read_text())
out = Path(config['evidence'])
read_child, write_parent = os.pipe2(os.O_CLOEXEC)
read_parent, write_child = os.pipe2(os.O_CLOEXEC)
actions = [(os.POSIX_SPAWN_DUP2, read_child, 3), (os.POSIX_SPAWN_DUP2, write_child, 4)]
for fd in (read_child, write_parent, read_parent, write_child):
    if fd not in (3, 4):
        actions.append((os.POSIX_SPAWN_CLOSE, fd))
# Preserve the parent's owned process group; never create another group/Job.
child = os.posix_spawn(config['chrome'], [config['chrome'], *sys.argv[2:]], os.environ, file_actions=actions)
os.close(read_child)
os.close(write_child)
closed = threading.Event()

def write_all(fd, data):
    data = memoryview(data)
    while data:
        count = os.write(fd, data)
        if not count:
            raise BrokenPipeError('Owned fixture pipe closed')
        data = data[count:]

def output():
    try:
        while True:
            data = os.read(read_parent, 65536)
            if not data:
                break
            write_all(4, data)
    except (BrokenPipeError, OSError):
        pass
    finally:
        closed.set()

thread = threading.Thread(target=output, daemon=True)
thread.start()
buffer = b''
latched = False
frozen = False
methods = []

def fixture_get(path):
    connection = http.client.HTTPConnection('127.0.0.1', config['proxyPort'], timeout=1)
    try:
        connection.request('GET', 'http://fixture.invalid/' + path)
        response = connection.getresponse()
        if response.status != 200:
            raise RuntimeError('Owned fixture latch rejected')
        return response.read(64).decode('ascii')
    finally:
        connection.close()

try:
    while not closed.is_set():
        if not select.select([3], [], [], .01)[0]:
            continue
        data = os.read(3, 65536)
        if not data:
            break
        buffer += data
        if len(buffer) > 16 * 1024 * 1024:
            raise RuntimeError('Fixture CDP packet exceeded wire limit')
        while b'\0' in buffer:
            packet, buffer = buffer.split(b'\0', 1)
            message = json.loads(packet)
            method = message.get('method', '')
            methods.append(method)
            if method == 'Page.setWebLifecycleState':
                frozen = message.get('params', {}).get('state') == 'frozen'
            if method == 'Accessibility.getFullAXTree' and not latched and Path(config['armed']).exists():
                latched = True
                fixture_get('_fixture/release')
                deadline = time.monotonic() + 1
                done = False
                while time.monotonic() < deadline:
                    if fixture_get('_fixture/status') == 'done':
                        done = True
                        break
                    time.sleep(.01)
                (out / 'race-latch.json').write_text(json.dumps({
                    'frozenAtAxDispatch': frozen,
                    'mutationDoneBeforeAx': done,
                    'injectedCdpCommands': 0,
                }, indent=2) + '\n')
            write_all(write_parent, packet + b'\0')
finally:
    (out / ('fixture-cdp-methods-'+str(os.getpid())+'.json')).write_text(json.dumps(methods, indent=2) + '\n')
    os.close(write_parent)
    if not closed.is_set():
        try:
            os.kill(child, signal.SIGTERM)
        except ProcessLookupError:
            pass
    os.waitpid(child, 0)
    os.close(read_parent)
