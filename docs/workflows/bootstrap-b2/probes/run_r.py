"""Run original R acceptance plus independently authored cases, no product edits."""
import os
from pathlib import Path
import subprocess
import tempfile
root = Path.cwd()
subprocess.run(['cargo','build','--offline','--manifest-path','core-s/Cargo.toml','--lib','--all-features'],check=True)
with tempfile.TemporaryDirectory(prefix='b2-r-review-') as tmp:
    source=Path(tmp)/'tests.rs'
    source.write_text((root/'core-s/tests/b2_runtime.rs').read_text()+'\n'+(root/'docs/workflows/bootstrap-b2/probes/r_extra.rs').read_text())
    binary=Path(tmp)/'tests'
    env=dict(os.environ,CARGO_MANIFEST_DIR=str(root/'core-s'))
    subprocess.run(['rustc','--edition','2024','--test',str(source),'--cfg','feature="pipeline"','--cfg','feature="image"','--extern','biomiya_core_s='+str(root/'core-s/target/debug/libbiomiya_core_s.rlib'),'-L','dependency='+str(root/'core-s/target/debug/deps'),'-o',str(binary)],env=env,check=True)
    subprocess.run([str(binary),'--nocapture'],check=True)
