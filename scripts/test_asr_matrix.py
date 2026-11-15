import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("matrix", Path(__file__).with_name("asr-matrix.py"))
matrix = importlib.util.module_from_spec(spec)
spec.loader.exec_module(matrix)


class MatrixTests(unittest.TestCase):
    def test_every_combination_has_independent_repetitions(self):
        rows = matrix.matrix(3, ("short", "long"))
        self.assertEqual(384, len(rows))
        self.assertEqual(len(rows), len(set(rows)))

    def test_decoder_mode_is_independent_of_submission_path(self):
        rows = list(matrix.coverage_matrix(3, ("short", "long")))
        self.assertEqual(768, len(rows))
        self.assertEqual(768, len(set(rows)))
        for model in matrix.MODELS:
            for mode in ("streaming", "offline"):
                feeds = {r[1] for r in rows if r[0] == model and r[-1] == mode}
                self.assertEqual({"capture", "buffered"}, feeds)
        supported = [r for r in rows if r[-1] == matrix.inference_mode(r[0])]
        self.assertEqual(384, len(supported))

    def test_missing_provider_cannot_be_reported_as_polishing(self):
        environment = {"SAYMORE_BENCH_SEACO_DIR": "/models"}
        self.assertIsNone(matrix.blocked("seaco", "disabled", environment))
        self.assertEqual("refinement_provider_not_configured",
                         matrix.blocked("seaco", "light", environment))
        self.assertEqual("model_directory_not_configured",
                         matrix.blocked("whisper", "disabled", environment))

    def test_generated_audio_has_reproducible_content_and_duration(self):
        with tempfile.TemporaryDirectory() as directory:
            first = matrix.fixtures(Path(directory))
            second = matrix.fixtures(Path(directory))
            self.assertEqual(first, second)
            short = first["short-zh"][0].read_bytes()
            long = first["segmented-zh"][0].read_bytes()
            self.assertEqual(60 * 16000 * 2 + len(short), len(long))
            self.assertTrue(long.endswith(short))


if __name__ == "__main__":
    unittest.main()
