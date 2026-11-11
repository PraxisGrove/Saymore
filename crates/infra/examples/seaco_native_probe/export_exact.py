"""Export exact verified SeACo using installed FunASR; python -I from project cwd."""
import os
from pathlib import Path
import sys

root = Path(sys.argv[1]).resolve()
for key, value in {
    'OMP_NUM_THREADS': '1', 'MKL_NUM_THREADS': '1', 'OPENBLAS_NUM_THREADS': '1',
    'VECLIB_MAXIMUM_THREADS': '1', 'NUMBA_NUM_THREADS': '1',
    'TORCH_FORCE_WEIGHTS_ONLY_LOAD': '1',
    'HF_HOME': str(root / 'export-env' / 'hf-cache'),
    'MODELSCOPE_CACHE': str(root / 'export-env' / 'ms-cache'),
    'XDG_CACHE_HOME': str(root / 'export-env' / 'cache'),
    'MPLCONFIGDIR': str(root / 'export-env' / 'mpl'),
}.items():
    os.environ[key] = value
import hashlib
import importlib.metadata
import json
import platform
import shutil
import subprocess
import torch

torch.set_num_threads(1)
torch.set_num_interop_threads(1)
torch.manual_seed(0)
source = root / 'models' / 'exact-ms'
manifest = json.loads((source / 'download-manifest.json').read_text())
for record in manifest['files']:
    p = Path(record['path'])
    h = hashlib.sha256()
    with p.open('rb') as f:
        for chunk in iter(lambda: f.read(8 * 1024 * 1024), b''):
            h.update(chunk)
    if h.hexdigest() != record['sha256']:
        raise RuntimeError(f'Integrity check failed: {p}')
if any(source.glob('*.py')) or (source / 'requirements.txt').exists():
    raise RuntimeError('Unexpected executable model content')
from funasr import AutoModel
print('Loading exact verified local model with weights_only enforced', flush=True)
model = AutoModel(model=str(source), device='cpu', ncpu=1, disable_update=True,
                  trust_remote_code=False, ignore_init_mismatch=False)
checkpoint = torch.load(source / 'model.pt', map_location='cpu', weights_only=True, mmap=True)
for key in ('state_dict', 'model_state_dict', 'model'):
    if key in checkpoint:
        checkpoint = checkpoint[key]
coverage = []
for name, tensor in model.model.state_dict().items():
    key = name if name in checkpoint else 'module.' + name
    if key not in checkpoint or checkpoint[key].shape != tensor.shape:
        raise RuntimeError(f'Checkpoint key missing/mismatched: {name}')
    if not torch.equal(tensor, checkpoint[key]):
        raise RuntimeError(f'Loaded tensor differs from exact checkpoint: {name}')
    coverage.append(name)
print(f'All {len(coverage)} model state tensors exactly match checkpoint', flush=True)
del checkpoint
out = root / 'models' / 'onnx-fp32'
out.mkdir(exist_ok=True)
(out / 'checkpoint-coverage.json').write_text(json.dumps({'checked_tensors': len(coverage), 'all_exact_match': True, 'keys': coverage}, indent=2) + '\n')
for name in ('config.yaml', 'configuration.json', 'am.mvn', 'tokens.json', 'seg_dict'):
    shutil.copy2(source / name, out / name)
print('Exporting FP32 opset14', flush=True)
model.export(output_dir=str(out), type='onnx', quantize=False, opset_version=14,
             device='cpu', max_seq_len=512)
versions = {name: importlib.metadata.version(name) for name in ('funasr', 'torch', 'torchaudio', 'onnx', 'onnxruntime', 'numpy')}
(root / 'export-env' / 'versions.json').write_text(json.dumps({'python': sys.version, 'platform': platform.platform(), 'packages': versions}, indent=2) + '\n')
(root / 'export-env' / 'requirements-freeze.txt').write_text(subprocess.check_output([sys.executable, '-I', '-m', 'pip', 'freeze'], text=True))
print('Export complete', flush=True)
