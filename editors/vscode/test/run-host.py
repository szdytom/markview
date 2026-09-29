"""Bound the real editor run and reap its process group, including desktop helpers."""
import argparse
import os
import signal
import subprocess
import sys
import time


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--log', required=True)
    parser.add_argument('--timeout', type=float, default=900)
    parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ['--'] else args.command
    def interrupted(signum, frame):
        raise KeyboardInterrupt

    signal.signal(signal.SIGTERM, interrupted)
    # A regular file cannot be held open like a pipe by a surviving grandchild.
    with open(args.log, 'wb') as output, open(args.log, 'rb') as reader:
        child = subprocess.Popen(command, stdout=output, stderr=subprocess.STDOUT,
                                 start_new_session=True)
        deadline = time.monotonic() + args.timeout
        try:
            while child.poll() is None and time.monotonic() < deadline:
                sys.stdout.buffer.write(reader.read())
                sys.stdout.buffer.flush()
                time.sleep(0.1)
            status = child.poll()
            if status is None:
                print(f'Test host exceeded {args.timeout:g}s', file=sys.stderr)
                status = 124
            # Detect a leaked engine before cleanup could conceal it.
            engine = os.environ.get('MARKVIEW_TEST_BINARY')
            if status == 0 and engine:
                for _ in range(30):
                    processes = subprocess.check_output(['ps', '-axo', 'command='], text=True)
                    if not any(line.startswith(engine + ' serve ') for line in processes.splitlines()):
                        break
                    time.sleep(0.2)
                else:
                    print('FAIL an engine outlived the window', file=sys.stderr)
                    status = 1
        finally:
            try:
                os.killpg(child.pid, signal.SIGTERM)
                time.sleep(0.5)
                child.poll()
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            child.wait()
            sys.stdout.buffer.write(reader.read())
            sys.stdout.buffer.flush()
        return status


if __name__ == '__main__':
    sys.exit(main())
