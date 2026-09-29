"""The runner must finish even when a child retains its output descriptor."""
import pathlib
import subprocess
import sys
import tempfile
import time
import unittest

RUNNER = pathlib.Path(__file__).with_name('run-host.py')


class HostLifecycle(unittest.TestCase):
    def run_host(self, source, timeout):
        with tempfile.TemporaryDirectory() as folder:
            start = time.monotonic()
            result = subprocess.run(
                [sys.executable, str(RUNNER), '--log', str(pathlib.Path(folder) / 'run.log'),
                 '--timeout', str(timeout), '--', sys.executable, '-c', source],
                capture_output=True, timeout=5)
            self.assertLess(time.monotonic() - start, 4)
            return result

    def test_failure_with_surviving_helper(self):
        result = self.run_host(
            "import subprocess,sys; child=subprocess.Popen([sys.executable,'-c',"
            "'import time; time.sleep(60)']); print(child.pid,flush=True); sys.exit(7)", 3)
        self.assertEqual(result.returncode, 7)
        helper = result.stdout.decode().strip()
        state = subprocess.run(['ps', '-o', 'stat=', '-p', helper], capture_output=True, text=True)
        self.assertTrue(not state.stdout.strip() or state.stdout.strip().startswith('Z'),
                        f'helper {helper} survived cleanup: {state.stdout}')

    def test_timeout(self):
        result = self.run_host('import time; time.sleep(60)', 0.2)
        self.assertEqual(result.returncode, 124)
        self.assertIn(b'exceeded', result.stderr)


if __name__ == '__main__':
    unittest.main()
