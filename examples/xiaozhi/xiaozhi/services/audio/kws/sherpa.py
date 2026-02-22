import numpy as np
import sherpa_onnx

from xiaozhi.utils.file import get_model_file_path


class _SherpaOnnx:
    def start(self):
        self.keyword_spotter = sherpa_onnx.KeywordSpotter(
            provider="cpu",
            num_threads=1,
            max_active_paths=8,
            keywords_score=2.0,
            keywords_threshold=0.2,
            num_trailing_blanks=0,
            keywords_file=get_model_file_path("keywords.txt"),
            tokens=get_model_file_path("tokens.txt"),
            encoder=get_model_file_path("encoder.onnx"),
            decoder=get_model_file_path("decoder.onnx"),
            joiner=get_model_file_path("joiner.onnx"),
        )
        self.stream = self.keyword_spotter.create_stream()

    def kws(self, frames):
        samples = np.frombuffer(frames, dtype=np.int16)
        samples = samples.astype(np.float32) / 32768.0
        self.stream.accept_waveform(16000, samples)
        while self.keyword_spotter.is_ready(self.stream):
            self.keyword_spotter.decode_stream(self.stream)
            result = self.keyword_spotter.get_result(self.stream)
            if result:
                self.keyword_spotter.reset_stream(self.stream)
                return result.lower()


SherpaOnnx = _SherpaOnnx()
