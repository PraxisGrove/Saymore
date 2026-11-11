"""Check exported graphs and run CPU model-only smoke tests; invoke with -I."""
import os
from pathlib import Path
import sys
root = Path(sys.argv[1]).resolve()
os.environ['OMP_NUM_THREADS'] = '1'
os.environ['VECLIB_MAXIMUM_THREADS'] = '1'
os.environ['XDG_CACHE_HOME'] = str(root / 'export-env' / 'cache')
import gc
import hashlib
import json
import numpy as np
import onnx
import onnxruntime as ort
import soundfile as sf
import torch
import yaml
from funasr.frontends.wav_frontend import WavFrontend

torch.set_num_threads(1)
torch.set_num_interop_threads(1)
base = root / 'models' / 'onnx-fp32'
report = {'models': [], 'smoke': []}
def description(x):
    t = x.type.tensor_type
    return {'name': x.name, 'dtype': onnx.TensorProto.DataType.Name(t.elem_type), 'shape': [d.dim_param if d.dim_param else d.dim_value for d in t.shape.dim]}
for name in ('model.onnx', 'model_eb.onnx'):
    p = base / name
    onnx.checker.check_model(str(p))
    m = onnx.load(str(p))
    h = hashlib.sha256()
    with p.open('rb') as f:
        for chunk in iter(lambda: f.read(8 * 1024 * 1024), b''):
            h.update(chunk)
    report['models'].append({'path': str(p), 'size': p.stat().st_size, 'sha256': h.hexdigest(), 'opsets': {x.domain: x.version for x in m.opset_import}, 'inputs': [description(x) for x in m.graph.input], 'outputs': [description(x) for x in m.graph.output], 'checker': 'passed'})
    del m
    gc.collect()
options = ort.SessionOptions()
options.intra_op_num_threads = 1
options.inter_op_num_threads = 1
options.graph_optimization_level = ort.GraphOptimizationLevel.ORT_DISABLE_ALL
eb = ort.InferenceSession(str(base / 'model_eb.onnx'), options, providers=['CPUExecutionProvider'])
for count in (1, 3, 6):
    hotword = np.zeros((count, 10), dtype=np.int32)
    hotword[:, 0] = 1
    value = eb.run(None, {'hotword': hotword})[0]
    if not np.isfinite(value).all():
        raise RuntimeError('Non-finite embedder output')
    report['smoke'].append({'graph': 'model_eb', 'hotword_shape': list(hotword.shape), 'hw_embed_shape': list(value.shape), 'finite': True})
    if count == 3:
        bias = value[0, :, :][None, :, :].copy()
del eb
gc.collect()
config = yaml.safe_load((base / 'config.yaml').read_text())
frontend_conf = dict(config['frontend_conf'])
frontend_conf['cmvn_file'] = str(base / 'am.mvn')
frontend_conf['dither'] = 0.0
frontend = WavFrontend(**frontend_conf)
audio, sr = sf.read(str(root / 'models' / 'exact-ms' / 'asr_example_hotword.wav'), dtype='float32')
if sr != 16000 or audio.ndim != 1:
    raise RuntimeError('Unexpected example audio format')
with torch.inference_mode():
    features, lengths = frontend(torch.from_numpy(audio)[None, :], torch.tensor([len(audio)], dtype=torch.int64))
session = ort.InferenceSession(str(base / 'model.onnx'), options, providers=['CPUExecutionProvider'])
inputs = {'speech': features.numpy().astype(np.float32), 'speech_lengths': lengths.numpy().astype(np.int32), 'bias_embed': bias}
outputs = session.run(None, inputs)
report['smoke'].append({'graph': 'model', 'inputs': {k: {'dtype': str(v.dtype), 'shape': list(v.shape)} for k, v in inputs.items()}, 'outputs': {item.name: {'dtype': str(v.dtype), 'shape': list(v.shape), 'finite': bool(np.isfinite(v).all())} for item, v in zip(session.get_outputs(), outputs)}, 'token_num': outputs[1].tolist(), 'example': 'asr_example_hotword.wav'})
if not all(np.isfinite(v).all() for v in outputs):
    raise RuntimeError('Non-finite backbone output')
(base / 'onnx-validation.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps(report, indent=2), flush=True)
