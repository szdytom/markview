import subprocess
import sys
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

from PIL import Image

from reconcile_render_golden import reconcile


class ReconcileGoldenTests(unittest.TestCase):
    def setUp(self):
        temporary = TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)

    def frame(self, name, pixels):
        path = self.root / f"{name}.png"
        Image.frombytes("RGBA", (len(pixels), 1), bytes(
            channel for pixel in pixels for channel in pixel
        )).save(path)
        return path

    def test_cli_makes_minimal_changes_and_preserves_inputs(self):
        baseline = self.frame("baseline", [(35, 41, 51, 255), (0, 255, 80, 10)])
        a = self.frame("a", [(35, 41, 51, 255), (0, 255, 80, 10)])
        b = self.frame("b", [(32, 38, 48, 255), (4, 251, 83, 14)])
        originals = [path.read_bytes() for path in (a, b)]
        result = subprocess.run(
            [sys.executable, str(Path(__file__).with_name("reconcile_render_golden.py")),
             str(baseline), str(a), str(b)],
            capture_output=True, text=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Adjusted 2 pixels, 7 channels", result.stdout)
        with Image.open(baseline) as image:
            self.assertEqual(image.tobytes(), bytes([34, 40, 50, 255, 2, 253, 81, 12]))
        adjusted = baseline.read_bytes()
        self.assertEqual(reconcile(baseline, [a, b]), (0, 0))
        self.assertEqual(baseline.read_bytes(), adjusted)
        self.assertEqual([path.read_bytes() for path in (a, b)], originals)

    def test_all_feasible_sample_pairs_choose_the_nearest_valid_value(self):
        cases = [
            (a, b, original)
            for a in range(256)
            for b in range(max(0, a - 4), min(256, a + 5))
            for original in (0, (a + b) // 2, 255)
        ]
        baseline = self.frame("baseline", [(v, v, v, v) for _, _, v in cases])
        a = self.frame("a", [(v, v, v, v) for v, _, _ in cases])
        b = self.frame("b", [(v, v, v, v) for _, v, _ in cases])
        reconcile(baseline, [a, b])
        with Image.open(baseline) as image:
            result = image.tobytes()
        for index, (a, b, original) in enumerate(cases):
            valid = [v for v in range(256) if abs(v - a) <= 2 and abs(v - b) <= 2]
            expected = min(valid, key=lambda v: abs(v - original))
            self.assertEqual(result[index * 4:index * 4 + 4], bytes([expected] * 4))

    def test_every_sample_constrains_the_reference(self):
        baseline = self.frame("baseline", [(0, 255, 100, 255)])
        actuals = [self.frame(str(i), [pixel]) for i, pixel in enumerate([
            (10, 20, 30, 100), (13, 23, 33, 103), (14, 24, 34, 104),
        ])]
        reconcile(baseline, actuals)
        with Image.open(baseline) as image:
            self.assertEqual(image.tobytes(), bytes([12, 22, 32, 102]))

    def test_incompatible_samples_leave_the_baseline_untouched(self):
        baseline = self.frame("baseline", [(0, 0, 0, 255)])
        a = self.frame("a", [(3, 3, 3, 250)])
        b = self.frame("b", [(3, 3, 3, 255)])
        original = baseline.read_bytes()
        with self.assertRaisesRegex(ValueError, "channel A: 250..255"):
            reconcile(baseline, [a, b])
        self.assertEqual(baseline.read_bytes(), original)

    def test_invalid_dimensions_and_missing_samples_do_not_write(self):
        baseline = self.frame("baseline", [(0, 0, 0, 255)])
        larger = self.frame("larger", [(0, 0, 0, 255)] * 2)
        original = baseline.read_bytes()
        for samples in ([], [baseline], [baseline, larger]):
            with self.assertRaises(ValueError):
                reconcile(baseline, samples)
            self.assertEqual(baseline.read_bytes(), original)


if __name__ == "__main__":
    unittest.main()
